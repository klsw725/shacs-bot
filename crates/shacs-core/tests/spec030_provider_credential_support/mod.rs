use serde_json::json;
use shacs_providers::{GenerationSettings, ProviderRequest};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

pub type Capture = thread::JoinHandle<Result<String, String>>;

pub fn serve_chat_responses(count: usize) -> Result<(String, Capture), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let mut requests = String::new();
        for _ in 0..count {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            requests.push_str(&read_request(&mut stream)?);
            let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"ok"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .map_err(|error| error.to_string())?;
        }
        Ok(requests)
    });
    Ok((format!("http://{address}/v1"), handle))
}

pub fn request() -> ProviderRequest {
    ProviderRequest {
        messages: vec![json!({"role": "user", "content": "hello"})],
        tools: Vec::new(),
        model: "gpt-4o".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    }
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
        if complete(&bytes)? {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn complete(bytes: &[u8]) -> Result<bool, String> {
    let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
        return Ok(false);
    };
    let headers =
        String::from_utf8(bytes[..header_end].to_vec()).map_err(|error| error.to_string())?;
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    Ok(bytes.len() >= header_end + 4 + length)
}
