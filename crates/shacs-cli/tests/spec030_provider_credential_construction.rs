use serde_json::json;
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::ProviderChatCompletionAdapter;
use shacs_config::{
    AuthStore, Config, ConfigBundle, ConfigContext, LocalAuthStore, ProviderAuth, ProviderConfig,
};
use shacs_projection::{CredentialSource, CredentialStatus};
use shacs_providers::{GenerationSettings, ProviderRequest};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

#[test]
fn spec030_cli_provider_builder_uses_local_auth_for_transport_and_shared_status(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let auth_path = root.path().join("auth.json");
    let mut auth = AuthStore::default();
    auth.providers.insert(
        "openai".to_owned(),
        ProviderAuth::api_key("cli-local-distinct"),
    );
    LocalAuthStore::new(&auth_path).save(&auth)?;
    let (api_base, capture) = serve_chat_response()?;
    let mut config = Config::default();
    config.agents.defaults.provider = "openai".to_owned();
    config.agents.defaults.model = "gpt-4o".to_owned();
    config.providers.insert(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("cli-literal-distinct".to_owned()),
            api_base: Some(api_base),
            ..ProviderConfig::default()
        },
    );
    let adapter = ProviderChatCompletionAdapter::from_bundle(ConfigBundle {
        config,
        context: ConfigContext {
            config_path: root.path().join("config.json"),
            data_dir: root.path().to_path_buf(),
            workspace,
        },
        migrations: Vec::new(),
    })?;

    adapter.complete_chat(ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role": "user", "content": "hello"})],
            tools: Vec::new(),
            model: "gpt-4o".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: Some("gpt-4o".to_owned()),
        session_key: "api:test".to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    })?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;
    let credential = adapter.trusted_runtime_projection().credential().clone();

    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer cli-local-distinct"));
    assert!(!captured.contains("cli-literal-distinct"));
    assert_eq!(credential.status, CredentialStatus::Resolved);
    assert_eq!(credential.source, Some(CredentialSource::LocalAuthStore));
    Ok(())
}

#[test]
fn spec030_cli_provider_builder_reaches_configured_command_source() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let (api_base, capture) = serve_chat_response()?;
    let config: Config = serde_json::from_value(json!({
        "agents": {"defaults": {"provider": "openai", "model": "gpt-4o"}},
        "providers": {"openai": {
            "apiKey": "literal-must-not-run",
            "apiBase": api_base,
            "credentialSource": {
                "schemaVersion": 1,
                "localAuth": false,
                "command": "printf command-production-value"
            }
        }}
    }))?;
    let adapter = ProviderChatCompletionAdapter::from_bundle(ConfigBundle {
        config,
        context: ConfigContext {
            config_path: root.path().join("config.json"),
            data_dir: root.path().to_path_buf(),
            workspace,
        },
        migrations: Vec::new(),
    })?;

    adapter.complete_chat(chat_invocation())?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;
    let credential = adapter.trusted_runtime_projection().credential().clone();

    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer command-production-value"));
    assert!(!captured.contains("literal-must-not-run"));
    assert_eq!(credential.source, Some(CredentialSource::Command));
    Ok(())
}

fn chat_invocation() -> ChatCompletionInvocation {
    ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role": "user", "content": "hello"})],
            tools: Vec::new(),
            model: "gpt-4o".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: Some("gpt-4o".to_owned()),
        session_key: "api:test".to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    }
}

type Capture = thread::JoinHandle<Result<String, String>>;

fn serve_chat_response() -> Result<(String, Capture), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let request = read_request(&mut stream)?;
        let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"ok"}}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(request)
    });
    Ok((format!("http://{address}/v1"), handle))
}

fn read_request(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 512];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let request = String::from_utf8(bytes.clone()).map_err(|error| error.to_string())?;
            let length = request
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let header_end = bytes
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .ok_or("HTTP header terminator missing")?;
            if bytes.len() >= header_end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}
