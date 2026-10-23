use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactTransactionStage,
    ArtifactWriteRequest, GeneratedArtifactDefinition, GeneratedArtifactMetadata,
    GeneratedMediaKind, GenerationOperation, ProjectionDisclosure, ProviderMediaBytes,
    ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaOrigin, ProviderRemoteMedia,
    RetentionPolicy, TransactionDecision,
};
use std::error::Error;
use std::sync::{Arc, Barrier, Mutex};

fn request(id: &str) -> Result<ArtifactWriteRequest, Box<dyn Error>> {
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new(format!("candidate-{id}"))?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", b"transactional png".to_vec()),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new(id)?,
        generated_definition(),
        "2026-08-15T00:00:00Z",
    );
    Ok(ArtifactWriteRequest::new(candidate, metadata))
}

#[test]
fn transaction_orders_syncs_before_atomic_publish() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observer = Arc::clone(&observed);

    // When
    store.persist_with_observer(request("ordered")?, move |stage| {
        observer.lock().expect("observer lock").push(stage);
        TransactionDecision::Continue
    })?;

    // Then
    assert_eq!(
        *observed.lock().expect("observer lock"),
        vec![
            ArtifactTransactionStage::PayloadSynced,
            ArtifactTransactionStage::RecordSynced,
            ArtifactTransactionStage::StagingDirectorySynced,
            ArtifactTransactionStage::Renamed,
            ArtifactTransactionStage::ParentDirectorySynced,
        ]
    );
    Ok(())
}

#[test]
fn crash_stage_matrix_leaves_complete_tree_or_no_visible_artifact() -> Result<(), Box<dyn Error>> {
    let stages = [
        ArtifactTransactionStage::PayloadSynced,
        ArtifactTransactionStage::RecordSynced,
        ArtifactTransactionStage::StagingDirectorySynced,
        ArtifactTransactionStage::Renamed,
        ArtifactTransactionStage::ParentDirectorySynced,
    ];

    for stage in stages {
        // Given
        let root = tempfile::tempdir()?;
        let store = ArtifactStore::open(root.path())?;

        // When
        let result = store.persist_with_observer(request("crash-case")?, |reached| {
            if reached == stage {
                TransactionDecision::Interrupt
            } else {
                TransactionDecision::Continue
            }
        });

        // Then
        let final_dir = root.path().join("artifacts/crash-case");
        match stage {
            ArtifactTransactionStage::PayloadSynced
            | ArtifactTransactionStage::RecordSynced
            | ArtifactTransactionStage::StagingDirectorySynced => {
                assert!(matches!(
                    result,
                    Err(shacs_core::generated_media::ArtifactStoreError::Interrupted(
                        reached
                    )) if reached == stage
                ));
                assert!(!final_dir.exists());
            }
            ArtifactTransactionStage::Renamed | ArtifactTransactionStage::ParentDirectorySynced => {
                assert!(matches!(
                    result,
                    Err(shacs_core::generated_media::ArtifactStoreError::CommitStatusUnknown(
                        reached
                    )) if reached == stage
                ));
                let reopened = ArtifactStore::open(root.path())?;
                let record = reopened.read(&ArtifactId::new("crash-case")?)?;
                assert_eq!(reopened.read_payload(&record)?, b"transactional png");
            }
        }
        assert_no_staging(root.path())?;
    }
    Ok(())
}

#[test]
fn open_cleans_stale_staging_before_persisting() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let stale = root.path().join("artifacts/.stage-stale");
    std::fs::create_dir_all(&stale)?;
    std::fs::write(stale.join("partial"), b"partial")?;

    // When
    let store = ArtifactStore::open(root.path())?;
    store.persist(request("after-stale")?)?;

    // Then
    assert_no_staging(root.path())?;
    Ok(())
}

#[test]
fn concurrent_same_id_publishes_once() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Arc::new(ArtifactStore::open(root.path())?);
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let request = request("same-id").map_err(|error| error.to_string())?;
                store.persist(request).map_err(|error| error.to_string())
            })
        })
        .collect::<Vec<_>>();

    // When
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("writer thread"))
        .collect::<Vec<_>>();

    // Then
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let record = store.read(&ArtifactId::new("same-id")?)?;
    assert_eq!(store.read_payload(&record)?, b"transactional png");
    assert_no_staging(root.path())?;
    Ok(())
}

#[test]
fn remote_candidate_is_rejected_without_exposing_url() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let candidate = ProviderMediaCandidate::remote(
        ProviderMediaCandidateId::new("remote-candidate")?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderRemoteMedia::new("image/png", "https://provider.example/signed?token=secret"),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new("remote-artifact")?,
        generated_definition(),
        "2026-08-15T00:00:00Z",
    );

    // When
    let error = store
        .persist(ArtifactWriteRequest::new(candidate, metadata))
        .expect_err("remote candidate requires guarded policy");

    // Then
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("provider.example"));
    assert!(!rendered.contains("token=secret"));
    assert!(!root.path().join("artifacts/remote-artifact").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn store_rejects_symlink_root_and_artifact_directory() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    // Given
    let parent = tempfile::tempdir()?;
    let target = parent.path().join("target");
    let root_link = parent.path().join("root-link");
    std::fs::create_dir(&target)?;
    symlink(&target, &root_link)?;
    let regular_root = parent.path().join("regular-root");
    std::fs::create_dir(&regular_root)?;
    symlink(&target, regular_root.join("artifacts"))?;

    // When
    let linked_root = ArtifactStore::open(&root_link);
    let linked_artifacts = ArtifactStore::open(&regular_root);

    // Then
    assert!(linked_root.is_err());
    assert!(linked_artifacts.is_err());
    Ok(())
}

fn assert_no_staging(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let artifacts = root.join("artifacts");
    if artifacts.exists() {
        for entry in std::fs::read_dir(artifacts)? {
            let name = entry?.file_name();
            assert!(!name.to_string_lossy().starts_with(".stage-"));
        }
    }
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
