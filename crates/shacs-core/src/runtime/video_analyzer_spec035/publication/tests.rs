use super::*;
use crate::runtime::video_analyzer_spec035::store_tests::canonical_projection;
use crate::runtime::video_analyzer_spec035::Spec035MediaProjectionTransactionStage;
use std::error::Error;

#[test]
fn unknown_commit_reopens_exact_projection_as_reconciled() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let publisher = VideoAnalyzerSpec035Publisher::new(store.clone(), None);
    let projection = canonical_projection()?;

    // When
    let status = publisher.publish_projection_with(&projection, |store, projection| {
        store.publish_with_parent_sync(projection, |_| {
            Err(std::io::Error::other("injected parent sync failure"))
        })
    })?;

    // Then
    assert_eq!(
        status,
        VideoAnalyzerSpec035PublicationStatus::Reconciled
    );
    assert_eq!(store.read()?.as_ref(), Some(&projection));
    Ok(())
}

#[test]
fn unreadable_unknown_commit_remains_explicitly_unknown() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let publisher = VideoAnalyzerSpec035Publisher::new(store, None);
    let projection = canonical_projection()?;

    // When
    let status = publisher.publish_projection_with(&projection, |_store, _projection| {
        Err(Spec035MediaProjectionStoreError::CommitStatusUnknown(
            Spec035MediaProjectionTransactionStage::Renamed,
        ))
    })?;

    // Then
    assert_eq!(
        status,
        VideoAnalyzerSpec035PublicationStatus::CommitStatusUnknown
    );
    Ok(())
}
