use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactWriteRequest, CommittedArtifact,
    GeneratedArtifactDefinition, GeneratedArtifactMetadata, GeneratedArtifactRecord,
    GeneratedMediaKind, GenerationOperation, GenerationOptionsSummary, MediaLineageId,
    MediaRootRelativePath, ProjectionDisclosure, ProviderMediaBytes, ProviderMediaCandidate,
    ProviderMediaCandidateId, ProviderMediaLifecycleEvent, ProviderMediaOrigin,
    ProviderRemoteMedia, RetentionPolicy, SafeModelId, SafeProviderId,
};
use std::collections::BTreeMap;
use std::error::Error;

fn write_request() -> Result<ArtifactWriteRequest, Box<dyn Error>> {
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new("candidate-safe-record")?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", b"safe png payload".to_vec()),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new("generated-safe-record")?,
        generated_definition(),
        "2026-08-15T00:00:00Z",
    );
    Ok(ArtifactWriteRequest::new(candidate, metadata))
}

#[test]
fn artifact_record() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;

    // When
    let record = store.persist(write_request()?)?;
    let serialized = serde_json::to_string_pretty(&record)?;

    // Then
    assert!(record
        .media_root_relative_path
        .as_str()
        .starts_with("artifacts/"));
    for forbidden in ["/Users/", "data:", "http", "safe png payload"] {
        assert!(
            !serialized.contains(forbidden),
            "unsafe record: {serialized}"
        );
    }
    assert!(serialized.contains("sha256"));
    assert!(serialized.contains("generated"));
    Ok(())
}

#[test]
fn relative_path_rejects_absolute_and_traversal_input() {
    // Given / When
    let absolute = MediaRootRelativePath::new("/Users/example/private.png");
    let traversal = MediaRootRelativePath::new("artifacts/../private.png");

    // Then
    assert!(absolute.is_err());
    assert!(traversal.is_err());
}

#[test]
fn persisted_fact_strings_reject_paths_urls_credentials_and_raw_markers() {
    // Given
    let oversized = "x".repeat(257);
    let unsafe_values = [
        "/Users/example/private",
        "C:\\Users\\example\\private",
        "https://provider.example/output",
        "model?token=secret",
        "data:image/png;base64,AAAA",
        "raw_payload",
        oversized.as_str(),
    ];

    // When / Then
    for value in unsafe_values {
        assert!(
            SafeProviderId::new(value).is_err(),
            "provider accepted {value}"
        );
        assert!(SafeModelId::new(value).is_err(), "model accepted {value}");
    }
    let unsafe_options = BTreeMap::from([
        (
            "size".to_owned(),
            "https://provider.example/output".to_owned(),
        ),
        ("raw_payload".to_owned(), "hidden".to_owned()),
    ]);
    assert!(GenerationOptionsSummary::new(unsafe_options).is_err());
}

#[test]
fn artifact_store_rejects_unsafe_provider_before_publication() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new("unsafe-provider-candidate")?,
        ProviderMediaOrigin::new("https://provider.example?token=secret", "model"),
        ProviderMediaBytes::new("image/png", b"safe png payload".to_vec()),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new("unsafe-provider-artifact")?,
        generated_definition(),
        "2026-08-15T00:00:00Z",
    );

    // When
    let result = store.persist(ArtifactWriteRequest::new(candidate, metadata));

    // Then
    assert!(result.is_err());
    assert!(!root
        .path()
        .join("artifacts/unsafe-provider-artifact")
        .exists());
    Ok(())
}

#[test]
fn final_lifecycle_requires_store_committed_proof() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let committed = store.persist(write_request()?)?;
    let constructor: fn(MediaLineageId, &CommittedArtifact) -> ProviderMediaLifecycleEvent =
        ProviderMediaLifecycleEvent::final_artifact;

    // When
    let event = constructor(MediaLineageId::new("lineage-one")?, &committed);
    let serialized = serde_json::to_string(&event)?;

    // Then
    assert!(serialized.contains("\"status\":\"final\""));
    assert!(serialized.contains("generated-safe-record"));
    Ok(())
}

#[test]
fn fabricated_record_cannot_issue_committed_proof() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    store.persist(write_request()?)?;
    let record_path = root
        .path()
        .join("artifacts/generated-safe-record/record.json");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&record_path)?)?;
    value["mediaRootRelativePath"] = serde_json::json!("artifacts/nonexistent/payload.png");
    value["sha256"] = serde_json::json!("0".repeat(64));
    std::fs::write(&record_path, serde_json::to_vec_pretty(&value)?)?;

    // When
    let result = store.read(&ArtifactId::new("generated-safe-record")?);

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn generated_record_rejects_inbound_provenance() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let record = ArtifactStore::open(root.path())?.persist(write_request()?)?;
    let mut value = serde_json::to_value(&record)?;
    value["provenance"]["kind"] = serde_json::json!("inbound_attachment");

    // When
    let result = serde_json::from_value::<GeneratedArtifactRecord>(value);

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn candidate_debug_and_lifecycle_observations_are_payload_free() -> Result<(), Box<dyn Error>> {
    // Given
    let bytes = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new("candidate-bytes")?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", b"raw-secret-bytes".to_vec()),
    );
    let remote = ProviderMediaCandidate::remote(
        ProviderMediaCandidateId::new("candidate-url")?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderRemoteMedia::new("image/png", "https://provider.example/signed?token=secret"),
    );
    let event = ProviderMediaLifecycleEvent::partial(MediaLineageId::new("lineage-one")?, 1);

    // When
    let rendered = format!("{bytes:?} {remote:?}");
    let observation = serde_json::to_string(&event)?;

    // Then
    assert!(!rendered.contains("raw-secret-bytes"));
    assert!(!rendered.contains("provider.example"));
    assert!(!rendered.contains("token=secret"));
    assert!(!observation.contains("payload"));
    assert!(!observation.contains("bytes"));
    assert!(!observation.contains("http"));
    Ok(())
}

fn generated_definition() -> GeneratedArtifactDefinition {
    GeneratedArtifactDefinition::new(
        GeneratedMediaKind::Image,
        GenerationOperation::Generate,
        ArtifactHandlingPolicy::new(
            RetentionPolicy::UserManaged,
            ProjectionDisclosure::RawContentPossibleElsewhere,
        ),
    )
}
