use serde_json::json;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactPublicationMetadata, ArtifactPublisher,
    ArtifactStore, ProjectionDisclosure, RemoteOutputEvaluationContext, RemoteOutputPolicy,
    RemotePublicationOutcome, RemoteReferenceExpiry, RetentionPolicy, UreqGuardedRemoteTransport,
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

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nremote-fixture";

#[test]
fn guarded_ready_to_persist_commits_one_relative_artifact() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || serve_once(listener));
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));
    let decision = RemoteOutputPolicy::download(1024, 0).evaluate(
        remote_candidate(&format!(
            "http://{address}/private/image.png?token=raw-secret"
        ))?,
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH),
    );
    server.join().map_err(|_| "fixture server panicked")??;

    // When
    let outcome = ArtifactPublisher::new(&store)
        .publish_remote(decision, publication_metadata("remote-persisted")?)?;

    // Then
    let RemotePublicationOutcome::Persisted(artifact) = outcome else {
        return Err("guarded remote bytes were not persisted".into());
    };
    assert!(!artifact.media_root_relative_path.as_path().is_absolute());
    let committed = store.read(&artifact.artifact_id)?;
    assert_eq!(store.read_payload(&committed)?, PNG);
    assert_eq!(artifact_count(root.path())?, 1);
    assert_no_staging(root.path())?;
    let durable =
        std::fs::read_to_string(root.path().join("artifacts/remote-persisted/record.json"))?;
    assert_safe_projection(&durable);
    Ok(())
}

#[test]
fn reference_and_rejected_outcomes_keep_safe_facts_without_files() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let expiry = RemoteReferenceExpiry::new(now + Duration::from_secs(60), now)?;
    let reference = RemoteOutputPolicy::reference(expiry).evaluate(
        remote_candidate("http://127.0.0.1/private/image.png?token=raw-secret")?,
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, now),
    );
    let rejected = RemoteOutputPolicy::reject().evaluate(
        remote_candidate("https://provider.example/private/image.png?token=raw-secret")?,
        RemoteOutputEvaluationContext::new(None, &transport, now),
    );

    // When
    let reference_outcome = ArtifactPublisher::new(&store)
        .publish_remote(reference, publication_metadata("reference-unused")?)?;
    let rejected_outcome = ArtifactPublisher::new(&store)
        .publish_remote(rejected, publication_metadata("rejected-unused")?)?;

    // Then
    assert!(matches!(
        &reference_outcome,
        RemotePublicationOutcome::Reference(_)
    ));
    assert!(matches!(
        &rejected_outcome,
        RemotePublicationOutcome::Rejected(_)
    ));
    assert_eq!(artifact_count(root.path())?, 0);
    let projected = format!(
        "{} {}",
        serde_json::to_string(&reference_outcome)?,
        serde_json::to_string(&rejected_outcome)?
    );
    assert!(projected.contains("127.0.0.1"));
    assert_safe_projection(&projected);
    assert_no_staging(root.path())?;
    Ok(())
}

fn publication_metadata(id: &str) -> Result<ArtifactPublicationMetadata, Box<dyn Error>> {
    Ok(ArtifactPublicationMetadata::try_new(
        ArtifactId::new(id)?,
        ArtifactHandlingPolicy::new(
            RetentionPolicy::UserManaged,
            ProjectionDisclosure::RawContentPossibleElsewhere,
        ),
        "2026-08-15T00:00:00Z",
    )?)
}

fn remote_candidate(url: &str) -> Result<ProviderRemoteMediaCandidate, Box<dyn Error>> {
    let mut result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({
                "choices": [{"message": {"images": [{
                    "mime_type": "image/png",
                    "image_url": {"url": url}
                }]}}]
            }),
        },
        "fixture-model",
    )?;
    result
        .remote_images
        .pop()
        .ok_or_else(|| "remote fixture candidate missing".into())
}

fn artifact_count(root: &std::path::Path) -> Result<usize, Box<dyn Error>> {
    Ok(std::fs::read_dir(root.join("artifacts"))?.count())
}

fn assert_no_staging(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(root.join("artifacts"))? {
        assert!(!entry?.file_name().to_string_lossy().starts_with(".stage-"));
    }
    Ok(())
}

fn assert_safe_projection(rendered: &str) {
    for forbidden in [
        "/Users/",
        "https://",
        "http://",
        "?token=",
        "raw-secret",
        "base64",
        "Bearer ",
        "providerBody",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "leaked {forbidden}: {rendered}"
        );
    }
}

fn serve_once(listener: TcpListener) -> Result<(), std::io::Error> {
    let (mut stream, _) = listener.accept()?;
    read_request(&mut stream)?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        PNG.len()
    )?;
    stream.write_all(PNG)
}

fn read_request(stream: &mut TcpStream) -> Result<(), std::io::Error> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}
