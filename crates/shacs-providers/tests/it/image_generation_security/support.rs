use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

pub const EXPECTED_RESPONSE_BODY_LIMIT: usize = 32 * 1024 * 1024;

pub fn assert_no_provider_secret(rendered: &str) {
    for forbidden in [
        "provider.example",
        "query-secret",
        "signed-secret",
        "cookie-secret",
        "created-secret",
        "nested-secret",
        "credential-secret",
        "usage-secret",
        "body-token",
        "body-signature",
        "body-secret",
        "provider-secret-code",
        "Set-Cookie",
        "Location",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "provider secret leaked: {rendered}"
        );
    }
}

type ServerHandle = thread::JoinHandle<Result<(), String>>;

pub fn serve_oversized_json(body_len: usize) -> Result<(String, ServerHandle), Box<dyn Error>> {
    serve_oversized_json_status(200, body_len)
}

pub fn serve_oversized_json_status(
    status: u16,
    body_len: usize,
) -> Result<(String, ServerHandle), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        read_request(&mut stream)?;
        let header = format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
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
