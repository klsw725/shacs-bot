#[cfg(unix)]
use crate::support::{persist_image, PNG};
#[cfg(unix)]
use shacs_core::generated_media::{
    ArtifactId, ArtifactReadStage, ArtifactStore, ArtifactStoreError,
};
#[cfg(unix)]
use std::error::Error;

#[cfg(unix)]
#[test]
fn parent_swap_between_validation_and_open_is_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    persist_image(&store, "source", PNG)?;
    let artifact_id = ArtifactId::new("source")?;
    let source_dir = root.path().join("artifacts/source");
    let moved_dir = root.path().join("moved-source");
    let mut swapped = false;

    // When
    let result = store.read_with_observer(&artifact_id, |stage| {
        assert_eq!(stage, ArtifactReadStage::BeforeArtifactDirectoryOpen);
        std::fs::rename(&source_dir, &moved_dir).expect("move source directory");
        std::fs::remove_file(moved_dir.join("record.json")).expect("remove moved record");
        std::fs::create_dir(moved_dir.join("record.json")).expect("plant nonregular record");
        symlink(&moved_dir, &source_dir).expect("replace source directory with symlink");
        swapped = true;
    });

    // Then
    assert!(swapped);
    assert!(
        matches!(result, Err(ArtifactStoreError::SymlinkRejected)),
        "unexpected result: {result:?}"
    );
    Ok(())
}
