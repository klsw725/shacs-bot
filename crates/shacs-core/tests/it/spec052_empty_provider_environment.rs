//! Spec 052 PRD 040 acceptance 2: with an empty `ANTHROPIC_API_KEY` in the
//! runtime's environment, a real request to a local fake Anthropic server
//! carries the key saved in the auth store.

use serde_json::json;
use shacs_config::{
    save_auth_store_to_path, AuthStore, ProviderAuth, ProviderConfig, ProvidersConfig,
};
use shacs_core::runtime::trusted_runtime::{Spec030FactStore, WorkspaceTrustObservation};
use shacs_core::runtime::{ProviderClientResolutionRequest, ProviderCredentialRuntime};
use shacs_providers::{GenerationSettings, ProviderRegistry, ProviderRequest};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

type Capture = JoinHandle<Result<String, String>>;

const SAVED_KEY: &str = "spec052-saved-anthropic-key";
const MESSAGE: &str = r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;

/// Answers one request on a local port the way the Anthropic messages API
/// does and hands back the request as received.
fn serve_one_message() -> Result<(String, Capture), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let request = read_request(&mut stream)?;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{MESSAGE}",
            MESSAGE.len()
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(request)
    });
    Ok((format!("http://{address}"), handle))
}

fn read_request(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        let Some(head_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
        let length = header(&head, "content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        if bytes.len() >= head_end + 4 + length {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}

#[test]
fn an_empty_anthropic_variable_does_not_hide_the_saved_key() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let mut auth = AuthStore::default();
    auth.providers
        .insert("anthropic".to_owned(), ProviderAuth::api_key(SAVED_KEY));
    save_auth_store_to_path(&auth, &auth_path)?;
    let (api_base, capture) = serve_one_message()?;
    let runtime = ProviderCredentialRuntime::new(
        &auth_path,
        root.path(),
        Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
    )
    .with_environment("ANTHROPIC_API_KEY", "");
    let providers = ProvidersConfig::from([(
        "anthropic".to_owned(),
        ProviderConfig {
            api_base: Some(api_base),
            ..ProviderConfig::default()
        },
    )]);

    // When
    let resolved = runtime.resolve_provider_client(ProviderClientResolutionRequest {
        registry: &ProviderRegistry::new(),
        requested_provider: "anthropic",
        model: "claude-opus-4-5",
        providers: &providers,
    })?;
    resolved.client.chat(ProviderRequest {
        messages: vec![json!({"role": "user", "content": "hello"})],
        tools: Vec::new(),
        model: "claude-opus-4-5".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    let request = capture.join().map_err(|_| "capture thread panicked")??;

    // Then
    assert_eq!(header(&request, "x-api-key"), Some(SAVED_KEY), "{request}");
    Ok(())
}
