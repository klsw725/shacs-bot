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

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nspec034-persistence";

fn main() -> Result<(), Box<dyn Error>> {
    let persisted_root = tempfile::tempdir()?;
    let reference_root = tempfile::tempdir()?;
    let rejected_root = tempfile::tempdir()?;
    let persisted_path = persisted_root.path().to_path_buf();
    let reference_path = reference_root.path().to_path_buf();
    let rejected_path = rejected_root.path().to_path_buf();

    let persisted_store = ArtifactStore::open(&persisted_path)?;
    let reference_store = ArtifactStore::open(&reference_path)?;
    let rejected_store = ArtifactStore::open(&rejected_path)?;
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let transport = UreqGuardedRemoteTransport::new(guard.clone(), Duration::from_secs(2));

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || serve_once(listener));
    let persisted = RemoteOutputPolicy::download(1024, 0).evaluate(
        remote_candidate(&format!("http://{address}/image.png?token=fixture-secret"))?,
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, SystemTime::UNIX_EPOCH),
    );
    server.join().map_err(|_| "fixture server panicked")??;
    let persisted = ArtifactPublisher::new(&persisted_store)
        .publish_remote(persisted, metadata("fixture-persisted")?)?;
    let RemotePublicationOutcome::Persisted(artifact) = persisted else {
        return Err("guarded download was not persisted".into());
    };
    let committed = persisted_store.read(&artifact.artifact_id)?;
    let payload = persisted_store.read_payload(&committed)?;
    let file_exists = persisted_path
        .join(artifact.media_root_relative_path.as_path())
        .is_file();

    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
    let expiry = RemoteReferenceExpiry::new(now + Duration::from_secs(60), now)?;
    let reference = RemoteOutputPolicy::reference(expiry).evaluate(
        remote_candidate("http://127.0.0.1/image.png?token=fixture-secret")?,
        RemoteOutputEvaluationContext::new(Some(&guard), &transport, now),
    );
    let reference = ArtifactPublisher::new(&reference_store)
        .publish_remote(reference, metadata("fixture-reference")?)?;
    let rejected = RemoteOutputPolicy::reject().evaluate(
        remote_candidate("https://provider.example/image.png?token=fixture-secret")?,
        RemoteOutputEvaluationContext::new(None, &transport, now),
    );
    let rejected = ArtifactPublisher::new(&rejected_store)
        .publish_remote(rejected, metadata("fixture-rejected")?)?;

    let reference_files = artifact_count(&reference_path)?;
    let rejected_files = artifact_count(&rejected_path)?;
    let staging_count = staging_count(&persisted_path)?
        + staging_count(&reference_path)?
        + staging_count(&rejected_path)?;
    let reference_projection = serde_json::to_string(&reference)?;
    let rejected_projection = serde_json::to_string(&rejected)?;
    for projected in [&reference_projection, &rejected_projection] {
        if projected.contains("http://")
            || projected.contains("https://")
            || projected.contains("?token=")
            || projected.contains("fixture-secret")
        {
            return Err("remote projection leaked untrusted URL material".into());
        }
    }

    drop(persisted_store);
    drop(reference_store);
    drop(rejected_store);
    persisted_root.close()?;
    reference_root.close()?;
    rejected_root.close()?;
    let roots_cleaned =
        !persisted_path.exists() && !reference_path.exists() && !rejected_path.exists();

    println!(
        "{}",
        json!({
            "persisted": {
                "relativeRef": artifact.media_root_relative_path.as_str(),
                "digest": artifact.sha256.as_str(),
                "fileExists": file_exists,
                "payloadHashMatches": shacs_core::generated_media::Sha256Digest::from_bytes(&payload)
                    == artifact.sha256,
                "recordExists": true,
            },
            "referenceFileCount": reference_files,
            "rejectedFileCount": rejected_files,
            "referenceOutcome": reference,
            "rejectedOutcome": rejected,
            "stagingCount": staging_count,
            "tempRootsCleaned": roots_cleaned,
        })
    );
    Ok(())
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

fn remote_candidate(url: &str) -> Result<ProviderRemoteMediaCandidate, Box<dyn Error>> {
    let mut result = parse_openrouter_image_generation_response(
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
    result
        .remote_images
        .pop()
        .ok_or_else(|| "remote fixture candidate missing".into())
}

fn artifact_count(root: &std::path::Path) -> Result<usize, Box<dyn Error>> {
    Ok(std::fs::read_dir(root.join("artifacts"))?.count())
}

fn staging_count(root: &std::path::Path) -> Result<usize, Box<dyn Error>> {
    Ok(std::fs::read_dir(root.join("artifacts"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".stage-"))
        .count())
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
