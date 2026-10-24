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
