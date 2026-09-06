use serde_json::json;
use shacs_core::generated_media::{
    RemoteOutputDecision, RemoteOutputEvaluationContext, RemoteOutputPolicy,
    UreqGuardedRemoteTransport,
};
use shacs_providers::{parse_openrouter_image_generation_response, ImageGenerationHttpResponse};
use shacs_security::NetworkGuard;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";

fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || serve_once(listener));
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));
    let mut provider_result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"choices": [{"message": {"images": [{
                "mime_type": "image/png",
                "image_url": {"url": format!("http://{address}/image.png")}
            }]}}]}),
        },
        "fixture-model",
    )?;
    let candidate = provider_result
        .remote_images
        .pop()
        .ok_or("provider fixture did not produce remote media")?;
    let context = RemoteOutputEvaluationContext::new(
        Some(&guard),
        &transport,
        std::time::SystemTime::UNIX_EPOCH,
    );
    let decision = RemoteOutputPolicy::download(1024, 0).evaluate(candidate, context);
    let request = server
        .join()
        .map_err(|_| "fixture server panicked")??
        .to_ascii_lowercase();
    let headers_clean = [
        "authorization:",
        "cookie:",
        "proxy-authorization:",
        "x-openrouter-",
    ]
    .iter()
    .all(|header| !request.contains(header));
    let RemoteOutputDecision::ReadyToPersist(ready) = decision else {
        return Err("guarded fixture did not produce persistence-ready bytes".into());
    };
    println!(
        "{{\"outcome\":\"ready_to_persist\",\"peerBound\":{},\"byteLen\":{},\"mimeType\":\"{}\",\"credentialHeadersAbsent\":{}}}",
        ready.evidence().connected_peer().is_loopback(),
        ready.evidence().byte_len(),
        ready.evidence().mime_type(),
        headers_clean
    );
    Ok(())
}

fn serve_once(listener: TcpListener) -> Result<String, std::io::Error> {
    let (mut stream, _) = listener.accept()?;
    let request = read_request(&mut stream)?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        PNG.len()
    )?;
    stream.write_all(PNG)?;
    Ok(request)
}

fn read_request(stream: &mut TcpStream) -> Result<String, std::io::Error> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
