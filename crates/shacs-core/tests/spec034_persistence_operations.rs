#[path = "spec034_image_edit/support.rs"]
mod support;

use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactPublicationMetadata, ArtifactPublisher,
    ArtifactStore, GenerationOptionsSummary, ImageOperationService, ProjectionDisclosure,
    RetentionPolicy,
};
use shacs_core::generated_media::{ArtifactImageOperationRequest, GenerationOperation};
use std::collections::BTreeMap;
use std::error::Error;
use support::{artifact_count, persist_image, CountingClient, PNG};

#[test]
fn final_edit_and_variation_candidates_publish_one_complete_record_each(
) -> Result<(), Box<dyn Error>> {
    for (artifact_id, operation) in [
        ("published-edit", GenerationOperation::Edit),
        ("published-variation", GenerationOperation::Variation),
    ] {
        // Given
        let root = tempfile::tempdir()?;
        let store = ArtifactStore::open(root.path())?;
        let source = persist_image(&store, "source", PNG)?;
        let client = CountingClient::new();
        let service = ImageOperationService::new(&store, &client);
        let request = match operation {
            GenerationOperation::Edit => {
                ArtifactImageOperationRequest::edit("add hat", source.clone())
            }
            GenerationOperation::Variation => {
                ArtifactImageOperationRequest::variation(source.clone())
            }
            GenerationOperation::Generate => return Err("invalid operation fixture".into()),
        };
        let candidate = service.execute(request)?;
        assert_eq!(client.calls(), 1);
        let metadata =
            publication_metadata(artifact_id)?.with_options(GenerationOptionsSummary::new(
                BTreeMap::from([("quality".to_owned(), "high".to_owned())]),
            )?);

        // When
        let committed = ArtifactPublisher::new(&store).publish_operation(candidate, metadata)?;

        // Then
        assert_eq!(artifact_count(root.path())?, 2);
        assert_eq!(committed.provenance.operation, operation);
        assert_eq!(committed.provenance.source_artifact_ids.len(), 1);
        assert_eq!(
            committed.provenance.source_artifact_ids[0].as_str(),
            "source"
        );
        assert_eq!(committed.retention, RetentionPolicy::UserManaged);
        assert_eq!(
            committed.disclosure,
            ProjectionDisclosure::RawContentPossibleElsewhere
        );
        assert!(!committed.media_root_relative_path.as_path().is_absolute());
        assert_eq!(store.read_payload(&committed)?, PNG);
    }
    Ok(())
}

#[test]
fn fresh_final_candidate_cannot_replace_an_existing_artifact_id() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::new();
    let service = ImageOperationService::new(&store, &client);
    let edit = service.execute(ArtifactImageOperationRequest::edit(
        "add hat",
        source.clone(),
    ))?;
    ArtifactPublisher::new(&store).publish_operation(edit, publication_metadata("stable-id")?)?;
    let variation = service.execute(ArtifactImageOperationRequest::variation(source))?;

    // When
    let replacement = ArtifactPublisher::new(&store)
        .publish_operation(variation, publication_metadata("stable-id")?);

    // Then
    assert!(replacement.is_err());
    let committed = store.read(&ArtifactId::new("stable-id")?)?;
    assert_eq!(committed.provenance.operation, GenerationOperation::Edit);
    assert_eq!(store.read_payload(&committed)?, PNG);
    assert_eq!(artifact_count(root.path())?, 2);
    for entry in std::fs::read_dir(root.path().join("artifacts"))? {
        assert!(!entry?.file_name().to_string_lossy().starts_with(".stage-"));
    }
    Ok(())
}

#[test]
fn misleading_provider_success_without_final_candidate_publishes_nothing(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::misleading_success();
    let service = ImageOperationService::new(&store, &client);

    // When
    let result = service.execute(ArtifactImageOperationRequest::edit("add hat", source));

    // Then
    assert!(result.is_err());
    assert_eq!(client.calls(), 1);
    assert_eq!(artifact_count(root.path())?, 1);
    Ok(())
}

#[test]
fn malformed_publication_metadata_is_rejected_before_any_artifact_is_written(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::new();
    let service = ImageOperationService::new(&store, &client);
    let candidate = service.execute(ArtifactImageOperationRequest::edit("add hat", source))?;

    // When
    let metadata = ArtifactPublicationMetadata::try_new(
        ArtifactId::new("invalid-metadata")?,
        ArtifactHandlingPolicy::new(
            RetentionPolicy::UserManaged,
            ProjectionDisclosure::RawContentPossibleElsewhere,
        ),
        "/Users/private/path?token=secret",
    );

    // Then
    assert!(metadata.is_err());
    drop(candidate);
    assert_eq!(artifact_count(root.path())?, 1);
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
