use crate::support::{artifact_count, persist_image, CountingClient, PNG};
use shacs_core::generated_media::{
    ArtifactImageOperationRequest, ArtifactStore, ImageOperationService,
};
use std::error::Error;

#[test]
fn valid_mask_invokes_transport_once_and_returns_complete_unpublished_lineage(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let mask = persist_image(&store, "mask", PNG)?;
    let client = CountingClient::new();
    let service = ImageOperationService::new(&store, &client);

    // When
    let candidate = service.execute(ArtifactImageOperationRequest::mask(
        "replace sky",
        source,
        Some(mask),
    ))?;

    // Then
    assert_eq!(client.calls(), 1);
    assert_eq!(candidate.local_image().image().byte_len, PNG.len());
    assert_eq!(candidate.source_artifact_ids().len(), 2);
    assert_eq!(candidate.source_artifact_ids()[0].as_str(), "source");
    assert_eq!(candidate.source_artifact_ids()[1].as_str(), "mask");
    assert_eq!(artifact_count(root.path())?, 2);
    Ok(())
}

#[test]
fn edit_and_variation_preserve_the_single_source_id() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::new();
    let service = ImageOperationService::new(&store, &client);

    let edit = service.execute(ArtifactImageOperationRequest::edit(
        "add hat",
        source.clone(),
    ))?;
    let variation = service.execute(ArtifactImageOperationRequest::variation(source))?;

    assert_eq!(client.calls(), 2);
    assert_eq!(edit.source_artifact_ids()[0].as_str(), "source");
    assert_eq!(variation.source_artifact_ids()[0].as_str(), "source");
    assert_eq!(artifact_count(root.path())?, 1);
    Ok(())
}
