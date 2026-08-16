use super::remote::{candidate, PNG};
use serde::Serialize;
use shacs_core::generated_media::{
    RemoteOutputDecision, RemoteOutputEvaluationContext, RemoteOutputPolicy, RemoteRejectionReason,
    UreqGuardedRemoteTransport,
};
use shacs_security::NetworkGuard;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

#[derive(Debug, Serialize)]
pub struct RemotePolicyMatrix {
    pub initial_guard: bool,
    pub redirect_guard: bool,
    pub scheme: bool,
    pub byte_cap: bool,
    pub mime_cap: bool,
}

pub fn run() -> Result<RemotePolicyMatrix, Box<dyn Error>> {
    let strict_guard = NetworkGuard::default();
    let strict_transport =
        UreqGuardedRemoteTransport::new(strict_guard.clone(), Duration::from_millis(200));
    let mut initial_guard = true;
    for url in [
        "http://10.0.0.1/image.png",
        "http://169.254.169.254/image.png",
        "http://127.0.0.1/image.png",
    ] {
        initial_guard &= rejected_reason(RemoteOutputPolicy::download(1024, 0).evaluate(
            candidate(url)?,
            evaluation(Some(&strict_guard), &strict_transport),
        )) == Some(RemoteRejectionReason::TargetPolicy);
    }
    let mut scheme = true;
    for url in [
        "https://user@example.com/image.png",
        "file:///tmp/image.png",
    ] {
        scheme &= rejected_reason(RemoteOutputPolicy::download(1024, 0).evaluate(
            candidate(url)?,
            evaluation(Some(&strict_guard), &strict_transport),
        )) == Some(RemoteRejectionReason::InvalidUrl);
    }
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(1));
    let redirect_guard = redirect_guard(&guard, &transport)?;
    let byte_cap = response_rejected(
        &guard,
        &transport,
        vec![0; 9],
        "image/png",
        8,
        RemoteRejectionReason::ByteLimit,
    )?;
    let mime_cap = response_rejected(
        &guard,
        &transport,
        PNG.to_vec(),
        "image/jpeg",
        1024,
        RemoteRejectionReason::MimeMismatch,
    )?;
    Ok(RemotePolicyMatrix {
        initial_guard,
        redirect_guard,
        scheme,
        byte_cap,
        mime_cap,
    })
}

fn redirect_guard(
    guard: &NetworkGuard,
    transport: &UreqGuardedRemoteTransport,
) -> Result<bool, Box<dyn Error>> {
    let mut all_rejected = true;
    for location in [
        "http://10.0.0.1/private",
        "http://169.254.169.254/private",
        "http://localhost/private",
    ] {
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let server = RawResponseFixture::start(response.into_bytes())?;
        let decision = RemoteOutputPolicy::download(1024, 3).evaluate(
            candidate(&server.url())?,
            evaluation(Some(guard), transport),
        );
        server.finish()?;
        all_rejected &= rejected_reason(decision) == Some(RemoteRejectionReason::TargetPolicy);
    }
    Ok(all_rejected)
}

fn response_rejected(
    guard: &NetworkGuard,
    transport: &UreqGuardedRemoteTransport,
    body: Vec<u8>,
    content_type: &str,
    limit: usize,
    expected: RemoteRejectionReason,
) -> Result<bool, Box<dyn Error>> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let server = RawResponseFixture::start([header.into_bytes(), body].concat())?;
    let decision = RemoteOutputPolicy::download(limit, 0).evaluate(
        candidate(&server.url())?,
        evaluation(Some(guard), transport),
    );
    server.finish()?;
    Ok(rejected_reason(decision) == Some(expected))
}

fn evaluation<'a>(
    guard: Option<&'a NetworkGuard>,
    transport: &'a UreqGuardedRemoteTransport,
) -> RemoteOutputEvaluationContext<'a> {
    RemoteOutputEvaluationContext::new(guard, transport, SystemTime::UNIX_EPOCH)
}

fn rejected_reason(decision: RemoteOutputDecision) -> Option<RemoteRejectionReason> {
    match decision {
        RemoteOutputDecision::Rejected(rejection) => Some(rejection.reason()),
        RemoteOutputDecision::ReadyToPersist(_) | RemoteOutputDecision::Reference(_) => None,
    }
}

struct RawResponseFixture {
    address: std::net::SocketAddr,
    worker: JoinHandle<Result<(), String>>,
}

impl RawResponseFixture {
    fn start(response: Vec<u8>) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            read_request(&mut stream).map_err(|error| error.to_string())?;
            stream
                .write_all(&response)
                .map_err(|error| error.to_string())
        });
        Ok(Self { address, worker })
    }

    fn url(&self) -> String {
        format!("http://{}/start", self.address)
    }

    fn finish(self) -> Result<(), String> {
        self.worker
            .join()
            .map_err(|_| "raw fixture worker panicked".to_owned())?
    }
}

fn read_request(stream: &mut TcpStream) -> Result<(), std::io::Error> {
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
    Ok(())
}
