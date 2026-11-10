//! Spec 052 PRD 040 acceptance 3: logins and imports save the provider's
//! credential declaration fingerprint with the credential, from the
//! declaration the runtime reads, and leave other providers alone.

use super::{
    import_codex_token, import_copilot_token, import_provider_api_key,
    local_provider_credential_runtime, save_codex_login_token, CliError, CodexImportTokenOptions,
    CodexTokenResponse, CopilotImportTokenOptions, ProviderApiKeyImportOptions, TokenSource,
};
use serde_json::{json, Value};
use shacs_config::{
    load_auth_store, load_config_with_env, save_auth_store_to_path, AuthStore, CredentialFamily,
    CredentialFingerprint, CredentialSourceDeclaration, LoadOptions, ProcessEnv, ProviderAuth,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::ProviderClientResolutionRequest;
use shacs_projection::{CredentialSource, Spec030ProjectionProvider};
use shacs_providers::ProviderRegistry;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

const TOKEN: &str = "spec052-prd040-saved-credential";
const OTHER: &str = "groq";

/// A variable name this process does not have, so `${NAME}` resolves only
/// from `.env.json` whatever environment the tests run in.
fn unset_name(base: &str) -> String {
    (0..)
        .map(|attempt| format!("{base}_{}_{attempt}", std::process::id()))
        .find(|name| std::env::var_os(name).is_none())
        .unwrap_or_else(|| base.to_owned())
}

/// A data folder whose auth store already holds another provider's key and
/// fingerprint, and an old credential and fingerprint for `provider`.
struct Data {
    root: tempfile::TempDir,
}

impl Data {
    fn new(provider: &str, mut config: Value) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        config["agents"]["defaults"]["workspace"] = json!(root.path().join("workspace"));
        fs::write(
            root.path().join("config.json"),
            serde_json::to_vec_pretty(&config)?,
        )?;
        let mut auth = AuthStore::default();
        auth.providers
            .insert(OTHER.to_owned(), ProviderAuth::api_key("other-key"));
        auth.set_fingerprint(OTHER, CredentialFingerprint::from_descriptor("other"));
        auth.providers
            .insert(provider.to_owned(), ProviderAuth::api_key("old-credential"));
        auth.set_fingerprint(provider, CredentialFingerprint::from_descriptor("old"));
        save_auth_store_to_path(&auth, &root.path().join("auth.json"))?;
        Ok(Self { root })
    }

    fn config(&self) -> PathBuf {
        self.root.path().join("config.json")
    }

    fn auth(&self) -> Result<AuthStore, Box<dyn Error>> {
        Ok(load_auth_store(&self.root.path().join("auth.json"))?)
    }
}

type Save = fn(&Path) -> Result<(), CliError>;

fn codex_login(config: &Path) -> Result<(), CliError> {
    save_codex_login_token(
        Some(config.to_path_buf()),
        CodexTokenResponse {
            access: TOKEN.to_owned(),
            refresh: None,
            expires: None,
            account_id: None,
        },
        |_| Err("offline".to_owned()),
    )
    .map(drop)
}

fn codex_import(config: &Path) -> Result<(), CliError> {
    import_codex_token(CodexImportTokenOptions {
        config_path: Some(config.to_path_buf()),
        token_source: TokenSource::Literal(TOKEN.to_owned()),
        account_id: None,
        select: false,
    })
    .map(drop)
}

fn copilot_import(config: &Path) -> Result<(), CliError> {
    import_copilot_token(CopilotImportTokenOptions {
        config_path: Some(config.to_path_buf()),
        token_source: TokenSource::Literal(TOKEN.to_owned()),
        select: false,
    })
    .map(drop)
}

fn anthropic_import(config: &Path) -> Result<(), CliError> {
    import_provider_api_key(ProviderApiKeyImportOptions {
        config_path: Some(config.to_path_buf()),
        provider: "anthropic".to_owned(),
        token_source: TokenSource::Literal(TOKEN.to_owned()),
    })
    .map(drop)
}

fn default_declaration(provider: &str, family: CredentialFamily) -> CredentialFingerprint {
    let environment = ProviderRegistry::new()
        .find_by_name(provider)
        .and_then(|spec| spec.env_key);
    CredentialSourceDeclaration::compatibility(family, environment).fingerprint()
}

#[test]
fn each_login_and_import_saves_the_current_declaration_fingerprint() -> TestResult {
    let cases: [(&str, Save, &str, CredentialFamily, &str); 4] = [
        (
            "provider codex login",
            codex_login,
            "openai_codex",
            CredentialFamily::OAuth,
            "gpt-5.4",
        ),
        (
            "provider codex import-token",
            codex_import,
            "openai_codex",
            CredentialFamily::OAuth,
            "gpt-5.4",
        ),
        (
            "provider copilot import-token",
            copilot_import,
            "github_copilot",
            CredentialFamily::OAuth,
            "gpt-4o",
        ),
        (
            "provider anthropic import-key",
            anthropic_import,
            "anthropic",
            CredentialFamily::ApiKey,
            "claude-opus-4-5",
        ),
    ];
    for (name, save, provider, family, model) in cases {
        // Given
        let data = Data::new(provider, json!({}))?;
        let other_before = data.auth()?;

        // When
        save(&data.config())?;

        // Then
        let auth = data.auth()?;
        assert_eq!(
            auth.fingerprint(provider),
            Some(default_declaration(provider, family)),
            "{name}"
        );
        assert_eq!(
            auth.providers
                .get(provider)
                .map(|entry| entry.access.as_str()),
            Some(TOKEN),
            "{name}"
        );
        assert_eq!(
            auth.providers.get(OTHER),
            other_before.providers.get(OTHER),
            "{name}"
        );
        assert_eq!(
            auth.fingerprint(OTHER),
            other_before.fingerprint(OTHER),
            "{name}"
        );

        // The runtime built as the gateway builds it takes the new credential
        // from the auth store; an empty declared variable does not hide it.
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(data.config()),
                workspace_override: None,
                resolve_env: true,
                write_back_migrations: false,
            },
            &ProcessEnv,
        )?;
        let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
        let runtime = local_provider_credential_runtime(&bundle, facts.clone())
            .with_environment("ANTHROPIC_API_KEY", "");
        runtime.resolve_provider_client(ProviderClientResolutionRequest {
            registry: &ProviderRegistry::new(),
            requested_provider: provider,
            model,
            providers: &bundle.config.providers,
        })?;
        let credential = LocalSpec030ProjectionProvider::new(facts)
            .projection()
            .credential()
            .clone();
        assert_eq!(
            credential.source,
            Some(CredentialSource::LocalAuthStore),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn the_fingerprint_comes_from_the_declaration_as_the_runtime_resolves_it() -> TestResult {
    let declared = |environment: &str| {
        CredentialSourceDeclaration {
            family: CredentialFamily::ApiKey,
            environment: Some(environment.to_owned()),
            local_auth: true,
            command: None,
        }
        .fingerprint()
    };
    let resolved_name = unset_name("SHACS_SPEC052_PRD040_DECLARATION");
    let missing = format!("${{{}}}", unset_name("SHACS_SPEC052_PRD040_MISSING"));
    let cases = [
        (
            "a declared variable name",
            json!({"providers": {"anthropic": {
                "credentialSource": {"schemaVersion": 1, "environment": "CUSTOM_ANTHROPIC_KEY"}
            }}}),
            declared("CUSTOM_ANTHROPIC_KEY"),
        ),
        (
            "a name resolved from .env.json, with unrelated names missing",
            json!({"providers": {
                "anthropic": {
                    "apiKey": missing,
                    "credentialSource": {
                        "schemaVersion": 1,
                        "environment": format!("${{{resolved_name}}}")
                    }
                },
                OTHER: {"apiKey": missing}
            }}),
            declared("RESOLVED_ANTHROPIC_KEY"),
        ),
        (
            "a declaration field that cannot be resolved",
            json!({"providers": {"anthropic": {
                "credentialSource": {"schemaVersion": 1, "environment": missing}
            }}}),
            declared(&missing),
        ),
    ];
    for (name, config, expected) in cases {
        // Given
        let data = Data::new("anthropic", config)?;
        fs::write(
            data.root.path().join(".env.json"),
            serde_json::to_vec(&json!({resolved_name.as_str(): "RESOLVED_ANTHROPIC_KEY"}))?,
        )?;

        // When
        anthropic_import(&data.config())?;

        // Then
        assert_eq!(
            data.auth()?.fingerprint("anthropic"),
            Some(expected),
            "{name}"
        );
    }
    Ok(())
}
