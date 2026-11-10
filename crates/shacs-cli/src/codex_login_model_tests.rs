//! Spec 052 PRD 060 acceptance 1 and 2: the Codex login keeps a model the
//! account offers, otherwise takes the account's highest-priority listed
//! model, and the model list cannot echo the token or the account ID into
//! config or output.

use super::{
    choose_codex_model, codex_account_models, format_codex_login_outcome, save_codex_login_token,
    CliError, CodexAccountModel, CodexAuthHttpResponse, CodexAuthTransport, CodexModelCheck,
    CodexTokenResponse, CODEX_DEFAULT_MODEL,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::error::Error;
use std::fs;

type TestResult = Result<(), Box<dyn Error>>;
type Models = Result<Vec<CodexAccountModel>, String>;
type GetCalls = Vec<(String, Vec<(String, String)>)>;

const ACCESS: &str = "access-secret-1234";
const ACCOUNT: &str = "acct-5678";

fn model(slug: &str, listed: bool, priority: i64) -> CodexAccountModel {
    CodexAccountModel {
        slug: slug.to_owned(),
        listed,
        priority,
    }
}

#[test]
fn the_login_model_follows_the_account_list() {
    // Given
    let offered = || -> Models {
        Ok(vec![
            model("gpt-reserve", false, 1),
            model("gpt-6-astra", true, 2),
            model("gpt-6-tie", true, 2),
            model("gpt-6-sol", true, 4),
        ])
    };
    let unread = || -> Models { Err("HTTP 500".to_owned()) };
    let none_listed = || -> Models { Ok(vec![model("gpt-reserve", false, 1)]) };
    let cases = [
        (
            Some("gpt-6-sol"),
            offered(),
            "gpt-6-sol",
            CodexModelCheck::KeptInAccountList,
        ),
        (
            Some("openai_codex/gpt-6-sol"),
            offered(),
            "openai_codex/gpt-6-sol",
            CodexModelCheck::KeptInAccountList,
        ),
        (
            Some("gpt-reserve"),
            offered(),
            "gpt-reserve",
            CodexModelCheck::KeptInAccountList,
        ),
        (
            Some("gpt-5.4"),
            offered(),
            "gpt-6-astra",
            CodexModelCheck::FromAccountList,
        ),
        (
            None,
            offered(),
            "gpt-6-astra",
            CodexModelCheck::FromAccountList,
        ),
        (
            Some("gpt-5.4"),
            unread(),
            "gpt-5.4",
            CodexModelCheck::KeptUnchecked("HTTP 500".to_owned()),
        ),
        (
            None,
            unread(),
            CODEX_DEFAULT_MODEL,
            CodexModelCheck::DefaultUnchecked("HTTP 500".to_owned()),
        ),
        (
            None,
            none_listed(),
            CODEX_DEFAULT_MODEL,
            CodexModelCheck::DefaultUnchecked("the account offers no listed model".to_owned()),
        ),
    ];
    for (configured, models, expected_model, expected_check) in cases {
        // When
        let (chosen, check) = choose_codex_model(configured, models);

        // Then
        assert_eq!(chosen, expected_model, "{configured:?}");
        assert_eq!(check, expected_check, "{configured:?}");
    }
}

struct ModelListTransport {
    gets: RefCell<GetCalls>,
    response: CodexAuthHttpResponse,
}

impl CodexAuthTransport for ModelListTransport {
    fn post_form(
        &self,
        _url: &str,
        _fields: &[(String, String)],
    ) -> Result<CodexAuthHttpResponse, CliError> {
        Err(CliError::InvalidArguments(
            "unexpected form post".to_owned(),
        ))
    }

    fn post_json(&self, _url: &str, _body: Value) -> Result<CodexAuthHttpResponse, CliError> {
        Err(CliError::InvalidArguments(
            "unexpected JSON post".to_owned(),
        ))
    }

    fn get_json(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<CodexAuthHttpResponse, CliError> {
        self.gets
            .borrow_mut()
            .push((url.to_owned(), headers.to_vec()));
        Ok(self.response.clone())
    }
}

#[test]
fn the_login_saves_a_listed_model_and_never_an_echo_of_its_secrets() -> TestResult {
    // Given: another provider is configured, and the list's first entries
    // echo the account ID and the token.
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&json!({
            "agents": {"defaults": {
                "provider": "anthropic",
                "model": "claude-opus-4-5",
                "workspace": root.path().join("workspace")
            }},
            "providers": {"openai_codex": {"apiBase": "http://codex.test/backend-api/"}}
        }))?,
    )?;
    let listed = |slug: &str, priority: i64| json!({"slug": slug, "visibility": "list", "priority": priority});
    let transport = ModelListTransport {
        gets: RefCell::new(Vec::new()),
        response: CodexAuthHttpResponse {
            status: 200,
            body: json!({"models": [
                listed(ACCOUNT, 0),
                listed(ACCESS, 1),
                listed(&format!("m-{ACCOUNT}-x"), 2),
                listed(&format!("model-{ACCESS}-x"), 3),
                listed("gpt-6.1-sol", 4),
                listed("gpt-6-sol", 5)
            ]}),
        },
    };

    // When
    let outcome = save_codex_login_token(
        Some(config_path.clone()),
        CodexTokenResponse {
            access: ACCESS.to_owned(),
            refresh: None,
            expires: None,
            account_id: Some(ACCOUNT.to_owned()),
        },
        |api_base| codex_account_models(&transport, api_base, ACCESS, Some(ACCOUNT)),
    )?;
    let output = format_codex_login_outcome(outcome.clone());

    // Then
    let gets = transport.gets.borrow();
    assert_eq!(gets.len(), 1);
    assert_eq!(
        gets[0].0,
        "http://codex.test/backend-api/codex/models?client_version=1.0.0"
    );
    assert!(gets[0]
        .1
        .contains(&("Authorization".to_owned(), format!("Bearer {ACCESS}"))));
    assert!(gets[0]
        .1
        .contains(&("ChatGPT-Account-Id".to_owned(), ACCOUNT.to_owned())));
    assert_eq!(outcome.selected_model, "gpt-6.1-sol");
    let config: Value = serde_json::from_slice(&fs::read(&config_path)?)?;
    assert_eq!(config["agents"]["defaults"]["provider"], "openai_codex");
    assert_eq!(config["agents"]["defaults"]["model"], "gpt-6.1-sol");
    assert!(
        output
            .contains("Model check: picked the highest-priority model your ChatGPT account lists"),
        "{output}"
    );
    let config_text = fs::read_to_string(&config_path)?;
    for secret in [ACCESS, ACCOUNT] {
        assert!(!output.contains(secret), "{output}");
        assert!(!config_text.contains(secret), "{config_text}");
    }
    Ok(())
}
