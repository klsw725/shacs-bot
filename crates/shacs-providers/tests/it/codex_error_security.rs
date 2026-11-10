use serde_json::json;
use shacs_providers::{
    CodexClient, CodexHttpStreamResponse, CodexHttpTransport, CodexRequestParts,
    GenerationSettings, ProviderClient, ProviderConfig, ProviderError, ProviderRequest,
    UreqCodexHttpTransport,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

const ERROR_SECRET: &str = "ghp_codex_error_secret";
const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
const MODEL_NOT_FOUND_TAIL: &str = "is not available for this ChatGPT account (model_not_found). Set agents.defaults.model to a model the account offers, or run shacs-bot provider codex login again to pick one.";

fn model_not_found_body() -> String {
    json!({
        "error": {
            "type": "invalid_request_error",
            "code": "model_not_found",
            "message": format!("The model is gone {ERROR_SECRET}")
        },
        "detail": format!("Provider detail {ERROR_SECRET}")
    })
    .to_string()
}

fn request_for(model: &str) -> ProviderRequest {
    ProviderRequest {
        messages: vec![json!({"role": "user", "content": "hello"})],
        tools: Vec::new(),
        model: model.to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    }
}

#[test]
fn oversized_codex_error_body_is_bounded_and_payload_free() -> Result<(), Box<dyn Error>> {
    // Given
    let (base_url, handle) = serve_oversized_error(RESPONSE_LIMIT + 1)?;
    let transport = UreqCodexHttpTransport::new(base_url);

    // When
    let error = transport
        .post_json_stream(CodexRequestParts {
            path: "/codex/responses".to_owned(),
            headers: BTreeMap::new(),
            body: json!({"stream": true}),
        })
        .expect_err("oversized Codex error must fail at the transport boundary");

    // Then
    handle.join().map_err(|_| "fixture server panicked")??;
    match error {
        ProviderError::Api {
            status: Some(503),
            message,
            retryable: true,
            headers,
            body: None,
        } if message == "codex_error_response_body_too_large" && headers.is_empty() => Ok(()),
        other => Err(format!("unexpected Codex bound error: {other:?}").into()),
    }
}

type ServerHandle = thread::JoinHandle<Result<(), String>>;

fn serve_oversized_error(body_len: usize) -> Result<(String, ServerHandle), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        read_request(&mut stream)?;
        let header = format!(
            "HTTP/1.1 503 Fixture\r\nContent-Type: application/json\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(header.as_bytes())
            .map_err(|error| error.to_string())?;
        let chunk = vec![b'x'; 64 * 1024];
        let mut remaining = body_len;
        while remaining > 0 {
            let length = remaining.min(chunk.len());
            if stream.write_all(&chunk[..length]).is_err() {
                break;
            }
            remaining -= length;
        }
        Ok(())
    });
    Ok((format!("http://{address}"), handle))
}

fn read_request(stream: &mut TcpStream) -> Result<(), String> {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 512];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 || chunk[..read].windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}

#[test]
fn ordinary_codex_error_body_is_replaced_by_fixed_safe_facts() -> Result<(), Box<dyn Error>> {
    // Given
    let client = CodexClient::new(ProviderConfig::default(), |_request: CodexRequestParts| {
        Ok(CodexHttpStreamResponse {
            status: 503,
            headers: BTreeMap::from([("x-should-retry".to_owned(), "true".to_owned())]),
            body: format!("provider failed with {ERROR_SECRET}"),
        })
    });

    // When
    let response = client.chat(ProviderRequest {
        messages: vec![json!({"role": "user", "content": "draw"})],
        tools: Vec::new(),
        model: "gpt-5.6".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;

    // Then
    let rendered = format!("{response:?} {}", serde_json::to_string(&response)?);
    assert!(!rendered.contains(ERROR_SECRET));
    assert_eq!(
        response.content.as_deref(),
        Some("HTTP 503: provider error")
    );
    assert_eq!(response.error_status_code, Some(503));
    assert_eq!(response.error_should_retry, Some(true));
    Ok(())
}

#[test]
fn a_known_codex_error_code_gets_our_fixed_sentence_without_provider_text(
) -> Result<(), Box<dyn Error>> {
    let oversized = format!("{}{}", model_not_found_body(), " ".repeat(64 * 1024));
    let cases = [
        (
            "gpt-5.4",
            model_not_found_body(),
            format!("HTTP 400: the model 'gpt-5.4' {MODEL_NOT_FOUND_TAIL}"),
        ),
        (
            "gpt-5.4",
            json!({"error": {"code": "invalid_value", "message": ERROR_SECRET}}).to_string(),
            "HTTP 400: provider error".to_owned(),
        ),
        ("gpt-5.4", oversized, "HTTP 400: provider error".to_owned()),
        (
            "odd model name",
            model_not_found_body(),
            format!("HTTP 400: the configured model {MODEL_NOT_FOUND_TAIL}"),
        ),
    ];
    for (model, body, expected) in cases {
        // Given
        let client = CodexClient::new(
            ProviderConfig::default(),
            move |_request: CodexRequestParts| {
                Ok(CodexHttpStreamResponse {
                    status: 400,
                    headers: BTreeMap::new(),
                    body: body.clone(),
                })
            },
        );

        // When
        let response = client.chat(request_for(model))?;

        // Then
        let rendered = format!("{response:?} {}", serde_json::to_string(&response)?);
        assert!(!rendered.contains(ERROR_SECRET), "{model}: {rendered}");
        assert!(!rendered.contains("The model is gone"), "{model}");
        assert_eq!(response.content.as_deref(), Some(expected.as_str()));
        assert_eq!(response.error_status_code, Some(400));
    }
    Ok(())
}

#[test]
fn the_http_transport_keeps_only_a_known_error_code_and_the_client_explains_it(
) -> Result<(), Box<dyn Error>> {
    // Given: a server that answers both requests with model_not_found and
    // provider text holding a secret.
    let (base_url, handle) = serve_json_error(400, model_not_found_body(), 2)?;
    let transport = UreqCodexHttpTransport::new(base_url.clone());
    let client = CodexClient::new(
        ProviderConfig::default(),
        UreqCodexHttpTransport::new(base_url),
    );

    // When
    let raw = transport.post_json_stream(CodexRequestParts {
        path: "/codex/responses".to_owned(),
        headers: BTreeMap::new(),
        body: json!({"model": "gpt-5.4", "stream": true}),
    })?;
    let response = client.chat(request_for("openai/gpt-5.4"))?;

    // Then
    handle.join().map_err(|_| "fixture server panicked")??;
    let raw_rendered = format!("{raw:?} {}", serde_json::to_string(&raw)?);
    assert!(!raw_rendered.contains(ERROR_SECRET), "{raw_rendered}");
    assert!(
        !raw_rendered.contains("The model is gone"),
        "{raw_rendered}"
    );
    assert_eq!(raw.body, r#"{"error":{"code":"model_not_found"}}"#);
    assert_eq!(
        response.content.as_deref(),
        Some(format!("HTTP 400: the model 'gpt-5.4' {MODEL_NOT_FOUND_TAIL}").as_str())
    );
    assert!(!format!("{response:?}").contains(ERROR_SECRET));
    Ok(())
}

fn serve_json_error(
    status: u16,
    body: String,
    requests: usize,
) -> Result<(String, ServerHandle), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        for _ in 0..requests {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            read_request_with_body(&mut stream)?;
            let response = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    });
    Ok((format!("http://{address}"), handle))
}

/// Reads the headers and the `Content-Length` body, so the client is not cut
/// off while it is still sending.
fn read_request_with_body(stream: &mut TcpStream) -> Result<(), String> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..read]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase();
        let length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + length {
            return Ok(());
        }
    }
}
