#[path = "spec033_local_improvement_support.rs"]
mod support;

use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    LocalArtifactOwner, LocalImprovementBlock, LocalImprovementRuntime, LocalImprovementStore,
};
use std::fs;
use std::sync::Arc;
use support::{proposal, write_snapshot, Gates, Verifier};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn runtime(
    root: &std::path::Path,
) -> Result<LocalImprovementRuntime<Gates, Verifier>, LocalImprovementBlock> {
    Ok(LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(root.join("store.json"))?),
        Arc::new(LocalArtifactOwner::new(root)?),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    ))
}

fn write_journal(
    root: &std::path::Path,
    phase: &str,
    original: &[u8],
    candidate: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let state = root.join(".shacs-self-improvement");
    fs::create_dir_all(&state)?;
    let value = serde_json::json!({
        "schema_version": 1,
        "proposal_id": "proposal:recovery",
        "target_ref": "settings.json",
        "before_digest": digest(original),
        "after_digest": digest(candidate),
        "checkpoint": original,
        "replacement": candidate,
        "receipt": {
            "operation": "apply",
            "receipt": {
                "owner_evidence_id": "owner:recovered",
                "gate_evidence_ids": ["hook", "confirmation", "process", "sandbox", "credential"]
            }
        },
        "phase": phase
    });
    fs::write(
        state.join("transaction.json"),
        serde_json::to_vec_pretty(&value)?,
    )?;
    Ok(())
}

#[test]
fn intent_is_inspectable_before_target_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let original = br#"{"enabled":false}"#;
    let candidate = br#"{"enabled":true}"#;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let active = runtime(root.path())?;
    active.propose(proposal("proposal:recovery", &digest(original), &snapshot)?)?;
    write_journal(root.path(), "intent_durable", original, candidate)?;

    let journal: serde_json::Value = serde_json::from_slice(&fs::read(
        root.path().join(".shacs-self-improvement/transaction.json"),
    )?)?;

    assert_eq!(journal["checkpoint"], serde_json::json!(original));
    assert_eq!(fs::read(target)?, original);
    Ok(())
}

#[test]
fn restart_discards_pre_mutation_intent_without_inventing_apply(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let original = br#"{"enabled":false}"#;
    let candidate = br#"{"enabled":true}"#;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let active = runtime(root.path())?;
    active.propose(proposal("proposal:recovery", &digest(original), &snapshot)?)?;
    write_journal(root.path(), "staged", original, candidate)?;

    let restarted = runtime(root.path())?;
    assert_eq!(
        restarted.verify("proposal:recovery"),
        Err(LocalImprovementBlock::NotApplied)
    );
    assert_eq!(fs::read(target)?, original);
    Ok(())
}

#[test]
fn restart_commits_applied_unverified_receipt_after_target_replacement(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let original = br#"{"enabled":false}"#;
    let candidate = br#"{"enabled":true}"#;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let active = runtime(root.path())?;
    active.propose(proposal("proposal:recovery", &digest(original), &snapshot)?)?;
    write_journal(root.path(), "target_replaced", original, candidate)?;
    fs::write(&target, candidate)?;

    let restarted = runtime(root.path())?;
    assert_eq!(
        restarted.apply("proposal:recovery"),
        Err(LocalImprovementBlock::AlreadyApplied)
    );
    let reopened = LocalImprovementStore::open(root.path().join("store.json"))?;
    assert!(reopened.apply_receipt("proposal:recovery").is_some());
    assert!(reopened.rollback_candidate("proposal:recovery").is_none());
    assert_eq!(fs::read(target)?, candidate);
    Ok(())
}

#[test]
fn restart_blocks_when_target_matches_neither_transaction_digest(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let original = br#"{"enabled":false}"#;
    let candidate = br#"{"enabled":true}"#;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let active = runtime(root.path())?;
    active.propose(proposal("proposal:recovery", &digest(original), &snapshot)?)?;
    write_journal(root.path(), "target_replaced", original, candidate)?;
    fs::write(&target, br#"{"external":true}"#)?;

    let restarted = runtime(root.path())?;
    assert_eq!(
        restarted.apply("proposal:recovery"),
        Err(LocalImprovementBlock::RecoveryRequired)
    );
    let reopened = LocalImprovementStore::open(root.path().join("store.json"))?;
    assert!(reopened.apply_receipt("proposal:recovery").is_none());
    assert!(root
        .path()
        .join(".shacs-self-improvement/transaction.json")
        .exists());
    Ok(())
}
