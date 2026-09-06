use super::*;
use crate::generated_media::{
    ArtifactHandlingPolicy, GeneratedArtifactDefinition, GeneratedArtifactMetadata,
    GeneratedMediaKind, GenerationOperation, ProjectionDisclosure, ProviderMediaBytes,
    ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaOrigin, RetentionPolicy,
};
use std::error::Error;

#[test]
fn parent_sync_failure_after_rename_reports_unknown_commit_status() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let request = request("sync-failure")?;

    // When
    let result = store.persist_transaction(
        request,
        |_| TransactionDecision::Continue,
        |artifacts| {
            let final_dir = artifacts.join("sync-failure");
            assert!(final_dir.join("payload.png").is_file());
            assert!(final_dir.join("record.json").is_file());
            Err(std::io::Error::other("injected parent sync failure"))
        },
    );

    // Then
    assert!(matches!(
        result,
        Err(ArtifactStoreError::CommitStatusUnknown(
            ArtifactTransactionStage::Renamed
        ))
    ));
    let reopened = ArtifactStore::open(root.path())?;
    let record = reopened.read(&ArtifactId::new("sync-failure")?)?;
    assert_eq!(reopened.read_payload(&record)?, b"transactional png");
    Ok(())
}

fn request(id: &str) -> Result<ArtifactWriteRequest, Box<dyn Error>> {
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new(format!("candidate-{id}"))?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", b"transactional png".to_vec()),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new(id)?,
        GeneratedArtifactDefinition::new(
            GeneratedMediaKind::Image,
            GenerationOperation::Generate,
            ArtifactHandlingPolicy::new(
                RetentionPolicy::UserManaged,
                ProjectionDisclosure::RawContentPossibleElsewhere,
            ),
        ),
        "2026-08-15T00:00:00Z",
    );
    Ok(ArtifactWriteRequest::new(candidate, metadata))
}
