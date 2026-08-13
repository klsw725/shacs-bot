use serde_json::json;
use shacs_app::app::{AppId, AppLifecycleState, AppRegistryStore};
use shacs_app::app_authoring::AppAuthoringStore;
use shacs_app::app_authoring_flow::{
    ApplyError, AuthoringFlowStore, ProposalKind, VerificationOutcome,
};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn new_draft_proposal_apply_and_install_handoff_is_non_executing() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let draft = AppAuthoringStore::new(&data_dir).init_app("notes.app")?;
    let store = AuthoringFlowStore::new(&data_dir);

    // When
    let proposal = store.propose_new(&draft.draft_path, "create with token=raw-secret")?;
    let checkpoint = store.checkpoint(&proposal)?;
    let pending = store.apply(&checkpoint)?;
    let handoff = store.verify(pending, VerificationOutcome::Passed)?;

    // Then
    assert_eq!(proposal.kind, ProposalKind::Install);
    assert!(proposal.user_intent.redacted);
    assert_eq!(
        handoff.registry_entry.lifecycle_state,
        AppLifecycleState::Installed
    );
    assert!(handoff.registry_entry.grant_reference.is_none());
    assert!(handoff.registry_entry.process_snapshots.is_empty());
    assert!(!data_dir.join("runtime").exists());
    assert!(!read_tree(&data_dir)?.contains("raw-secret"));
    Ok(())
}

#[test]
fn existing_app_proposal_records_snapshot_and_diff_before_update() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let installed = write_bundle(&data_dir, "clock.app", "1.0.0", "old")?;
    let original = AppRegistryStore::new(&data_dir).install_local_bundle(&installed)?;
    let candidate = write_candidate(root.path(), "clock.app", "2.0.0", "new")?;

    // When
    let store = AuthoringFlowStore::new(&data_dir);
    let proposal = store.propose_update(&candidate, "update clock")?;
    let checkpoint = store.checkpoint(&proposal)?;
    let handoff = store.verify(store.apply(&checkpoint)?, VerificationOutcome::Passed)?;

    // Then
    assert_eq!(proposal.kind, ProposalKind::Update);
    assert_eq!(
        proposal.installed_digest.as_deref(),
        Some(original.digest.as_str())
    );
    assert!(!proposal.diff.is_empty());
    assert!(checkpoint.snapshot_path.is_some());
    assert_eq!(fs::read(installed.join("entry.md"))?, b"new");
    assert_eq!(handoff.registry_entry.version, "2.0.0");
    assert!(!handoff.runtime_authorization_created);
    assert!(!handoff.executable_activation_created);
    assert!(!handoff.process_started);
    Ok(())
}

#[test]
fn stale_revision_and_changed_installed_digest_are_rejected_before_mutation(
) -> Result<(), Box<dyn Error>> {
    // Given a stale new-app draft
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let candidate = write_candidate(root.path(), "stale.app", "1.0.0", "one")?;
    let store = AuthoringFlowStore::new(&data_dir);
    let proposal = store.propose_new(&candidate, "install stale")?;
    fs::write(candidate.join("entry.md"), "two")?;

    // When
    let stale_error = store
        .checkpoint(&proposal)
        .expect_err("revision must be stale");

    // Then
    assert!(matches!(stale_error, ApplyError::StaleRevision { .. }));
    assert!(!data_dir.join("apps/stale.app.shacsapp").exists());

    // Given an installed digest changed after checkpoint
    let installed = write_bundle(&data_dir, "edit.app", "1.0.0", "old")?;
    AppRegistryStore::new(&data_dir).install_local_bundle(&installed)?;
    let update = write_candidate(root.path(), "edit.app", "2.0.0", "new")?;
    let proposal = store.propose_update(&update, "update edit")?;
    let checkpoint = store.checkpoint(&proposal)?;
    let mut registry = AppRegistryStore::new(&data_dir).load()?;
    registry
        .entries
        .get_mut(&AppId::parse("edit.app")?)
        .ok_or("entry")?
        .digest = "sha256:changed".into();
    AppRegistryStore::new(&data_dir).save(&registry)?;

    // When
    let digest_error = store.apply(&checkpoint).expect_err("digest CAS must fail");

    // Then
    assert!(matches!(
        digest_error,
        ApplyError::InstalledDigestChanged { .. }
    ));
    assert_eq!(fs::read(installed.join("entry.md"))?, b"old");
    Ok(())
}

#[test]
fn verify_failure_and_interrupted_apply_leave_recoverable_evidence() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let installed = write_bundle(&data_dir, "recover.app", "1.0.0", "old")?;
    AppRegistryStore::new(&data_dir).install_local_bundle(&installed)?;
    let candidate = write_candidate(root.path(), "recover.app", "2.0.0", "new")?;
    let store = AuthoringFlowStore::new(&data_dir);
    let proposal = store.propose_update(&candidate, "update and recover")?;
    let checkpoint = store.checkpoint(&proposal)?;

    // When apply returns, interruption evidence is already durable
    let pending = store.apply(&checkpoint)?;
    let interrupted = store.inspect_recovery(&checkpoint.checkpoint_id)?;

    // Then
    assert!(interrupted.recovery_required);
    assert_eq!(fs::read(installed.join("entry.md"))?, b"new");

    // When verification fails and recovery runs
    let failure = store
        .verify(
            pending,
            VerificationOutcome::Failed {
                reason: "invalid entry".into(),
            },
        )
        .expect_err("verification must fail");
    assert!(matches!(failure, ApplyError::VerificationFailed { .. }));
    let recovered = store.recover(&checkpoint.checkpoint_id)?;

    // Then
    assert!(!recovered.recovery_required);
    assert_eq!(fs::read(installed.join("entry.md"))?, b"old");
    Ok(())
}

#[test]
fn completed_handoff_cannot_be_rolled_back_and_recovery_restores_registry(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let installed = write_bundle(&data_dir, "complete.app", "1.0.0", "old")?;
    let original = AppRegistryStore::new(&data_dir).install_local_bundle(&installed)?;
    let candidate = write_candidate(root.path(), "complete.app", "2.0.0", "new")?;
    let store = AuthoringFlowStore::new(&data_dir);
    let proposal = store.propose_update(&candidate, "complete update")?;
    let checkpoint = store.checkpoint(&proposal)?;
    let handoff = store.verify(store.apply(&checkpoint)?, VerificationOutcome::Passed)?;

    assert!(matches!(
        store.recover(&handoff.checkpoint_id),
        Err(ApplyError::RecoveryNotRequired(_))
    ));
    assert_eq!(fs::read(installed.join("entry.md"))?, b"new");

    let rollback_candidate = write_candidate(root.path(), "complete.app", "3.0.0", "next")?;
    let rollback_proposal = store.propose_update(&rollback_candidate, "rollback update")?;
    let rollback_checkpoint = store.checkpoint(&rollback_proposal)?;
    let pending = store.apply(&rollback_checkpoint)?;
    store
        .verify(
            pending,
            VerificationOutcome::Failed {
                reason: "failed".to_owned(),
            },
        )
        .expect_err("verification fails");
    store.recover(&rollback_checkpoint.checkpoint_id)?;
    let restored = AppRegistryStore::new(&data_dir)
        .inspect(&original.app_id)?
        .ok_or("restored entry")?;
    assert_eq!(restored.version, "2.0.0");
    assert_ne!(restored.digest, original.digest);
    Ok(())
}

#[test]
fn update_rejects_registry_clean_but_mutated_installed_tree() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let installed = write_bundle(&data_dir, "mutated.app", "1.0.0", "old")?;
    AppRegistryStore::new(&data_dir).install_local_bundle(&installed)?;
    let candidate = write_candidate(root.path(), "mutated.app", "2.0.0", "new")?;
    let store = AuthoringFlowStore::new(&data_dir);
    let proposal = store.propose_update(&candidate, "detect mutation")?;
    fs::write(installed.join("entry.md"), "tampered")?;

    assert!(matches!(
        store.checkpoint(&proposal),
        Err(ApplyError::InstalledDigestChanged { .. })
    ));

    fs::write(installed.join("entry.md"), "old")?;
    let proposal = store.propose_update(&candidate, "detect undeclared mutation")?;
    fs::write(installed.join("undeclared.txt"), "tampered")?;
    assert!(matches!(
        store.checkpoint(&proposal),
        Err(ApplyError::InstalledDigestChanged { .. })
    ));
    Ok(())
}

fn write_bundle(
    data_dir: &Path,
    app_id: &str,
    version: &str,
    body: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let bundle = data_dir.join("apps").join(format!("{app_id}.shacsapp"));
    write_app(&bundle, app_id, version, body)?;
    Ok(bundle)
}

fn write_candidate(
    root: &Path,
    app_id: &str,
    version: &str,
    body: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let candidate = root.join(format!("candidate-{app_id}-{version}"));
    write_app(&candidate, app_id, version, body)?;
    Ok(candidate)
}

fn write_app(path: &Path, app_id: &str, version: &str, body: &str) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path)?;
    fs::write(path.join("entry.md"), body)?;
    fs::write(path.join("README.md"), body)?;
    fs::write(
        path.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "id": app_id, "version": version, "entry": "entry.md"
        }))?,
    )?;
    Ok(())
}

fn read_tree(root: &Path) -> Result<String, Box<dyn Error>> {
    let mut text = String::new();
    for entry in walk(root)? {
        if entry.is_file() {
            text.push_str(&String::from_utf8_lossy(&fs::read(entry)?));
        }
    }
    Ok(text)
}

fn walk(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    if root.exists() {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                paths.extend(walk(&path)?);
            } else {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}
