use serde::Serialize;
use shacs_api::TRUSTED_RUNTIME_PATH;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Serialize)]
struct ProbeOutput {
    schema1: ProbeResponse,
    schema2: ProbeResponse,
}

#[derive(Serialize)]
struct ProbeResponse {
    status: u16,
    body: serde_json::Value,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("spec030 API probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let address = owner_address()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let output = runtime.block_on(probe_http(address))?;
    let json = serde_json::to_string(&output).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

async fn probe_http(address: SocketAddr) -> Result<ProbeOutput, String> {
    let schema1 = request(address, &format!("{TRUSTED_RUNTIME_PATH}?schema_version=1")).await?;
    let schema2 = request(address, &format!("{TRUSTED_RUNTIME_PATH}?schema_version=2")).await?;
    Ok(ProbeOutput { schema1, schema2 })
}

async fn request(address: SocketAddr, path: &str) -> Result<ProbeResponse, String> {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|error| error.to_string())?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|error| error.to_string())?;
    parse_response(&response)
}

fn parse_response(response: &[u8]) -> Result<ProbeResponse, String> {
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "API response headers are incomplete".to_owned())?;
    let headers = std::str::from_utf8(&response[..separator]).map_err(|error| error.to_string())?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "API response status is missing".to_owned())?
        .parse::<u16>()
        .map_err(|error| error.to_string())?;
    let body =
        serde_json::from_slice(&response[separator + 4..]).map_err(|error| error.to_string())?;
    Ok(ProbeResponse { status, body })
}

fn owner_address() -> Result<SocketAddr, String> {
    let mut arguments = std::env::args().skip(1);
    match (
        arguments.next().as_deref(),
        arguments.next(),
        arguments.next(),
    ) {
        (Some("--address"), Some(address), None) => {
            let parsed = address
                .parse::<SocketAddr>()
                .map_err(|error| error.to_string())?;
            if !parsed.ip().is_loopback() || parsed.port() == 0 {
                return Err("owner address must be a concrete loopback socket".to_owned());
            }
            Ok(parsed)
        }
        _ => Err("usage: spec030-api-probe --address <loopback-address>".to_owned()),
    }
}
