use serde::Serialize;
use serde_json::json;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactPublicationMetadata, ArtifactPublisher,
    ArtifactStore, ProjectionDisclosure, RemoteOutputDecision, RemoteOutputEvaluationContext,
    RemoteOutputPolicy, RemotePublicationOutcome, RemoteReferenceExpiry, RetentionPolicy,
    Sha256Digest, UreqGuardedRemoteTransport,
};
use shacs_providers::{
    parse_openrouter_image_generation_response, ImageGenerationHttpResponse,
    ProviderRemoteMediaCandidate,
};
use shacs_security::NetworkGuard;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime};

pub(super) const PNG: &[u8] = b"\x89PNG\r\n\x1a\nspec034-sequential-remote";

#[derive(Debug, Serialize)]
pub struct RemoteResult {
    pub outcomes: [&'static str; 3],
    pub credential_headers_absent: bool,
    pub persisted_hash_consistent: bool,
    pub private_target_rejected: bool,
    pub guard_absence_rejected: bool,
    pub policy_matrix: super::remote_matrix::RemotePolicyMatrix,
    #[serde(skip)]
    pub scan_output: String,
}

pub fn run(store: &ArtifactStore) -> Result<RemoteResult, Box<dyn Error>> {
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || serve_once(listener));
    let ready = RemoteOutputPolicy::download(1024, 0).evaluate(
        candidate(&format!(
            "http://{address}/image.png?token=untrusted-fixture-secret"
        ))?,
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH),
    );
    let request = server.join().map_err(|_| "remote fixture panicked")??;
    let persisted =
        ArtifactPublisher::new(store).publish_remote(ready, metadata("remote-persisted")?)?;
    let persisted_hash_consistent = match &persisted {
        RemotePublicationOutcome::Persisted(reference) => {
            let record = store.read(&reference.artifact_id)?;
            Sha256Digest::from_bytes(&store.read_payload(&record)?) == record.sha256
        }
        RemotePublicationOutcome::Reference(_) | RemotePublicationOutcome::Rejected(_) => false,
    };

    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let expiry = RemoteReferenceExpiry::new(now + Duration::from_secs(60), now)?;
    let reference = RemoteOutputPolicy::reference(expiry).evaluate(
        candidate("http://127.0.0.1/image.png?token=untrusted-fixture-secret")?,
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, now),
    );
    let reference =
        ArtifactPublisher::new(store).publish_remote(reference, metadata("remote-reference")?)?;
    let rejected = RemoteOutputPolicy::reject().evaluate(
        candidate("https://provider.example/image.png?token=untrusted-fixture-secret")?,
        RemoteOutputEvaluationContext::new(None, &transport, now),
    );
    let rejected =
        ArtifactPublisher::new(store).publish_remote(rejected, metadata("remote-rejected")?)?;
    let strict_guard = NetworkGuard::default();
    let private_target_rejected = matches!(
        RemoteOutputPolicy::reference(expiry).evaluate(
            candidate("http://127.0.0.1/private.png")?,
            RemoteOutputEvaluationContext::new(Some(&strict_guard), &transport, now),
        ),
        RemoteOutputDecision::Rejected(_)
    );
    let guard_absence_rejected = matches!(
        RemoteOutputPolicy::download(1024, 0).evaluate(
            candidate("https://provider.example/image.png")?,
            RemoteOutputEvaluationContext::new(None, &transport, now),
        ),
        RemoteOutputDecision::Rejected(_)
    );
    if !matches!(reference, RemotePublicationOutcome::Reference(_))
        || !matches!(rejected, RemotePublicationOutcome::Rejected(_))
    {
        return Err("remote outcomes were collapsed".into());
    }
    let scan_output = serde_json::to_string(&(&persisted, &reference, &rejected))?;
    let request = request.to_ascii_lowercase();
    let clean = ["authorization:", "cookie:", "proxy-authorization:"]
        .iter()
        .all(|header| !request.contains(header));
    Ok(RemoteResult {
        outcomes: ["persisted", "reference", "rejected"],
        credential_headers_absent: clean,
        persisted_hash_consistent,
        private_target_rejected,
        guard_absence_rejected,
        policy_matrix: super::remote_matrix::run()?,
        scan_output,
    })
}

fn metadata(id: &str) -> Result<ArtifactPublicationMetadata, Box<dyn Error>> {
    Ok(ArtifactPublicationMetadata::try_new(
        ArtifactId::new(id)?,
        ArtifactHandlingPolicy::new(
            RetentionPolicy::UserManaged,
            ProjectionDisclosure::RawContentPossibleElsewhere,
        ),
        "2026-08-15T00:00:00Z",
    )?)
}

pub(super) fn candidate(url: &str) -> Result<ProviderRemoteMediaCandidate, Box<dyn Error>> {
    let mut parsed = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"choices": [{"message": {"images": [{
                "mime_type": "image/png",
                "image_url": {"url": url}
            }]}}]}),
        },
        "fixture-model",
    )?;
    parsed
        .remote_images
        .pop()
        .ok_or_else(|| "remote provider fixture returned no candidate".into())
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
    loop {
        let mut chunk = [0_u8; 512];
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
