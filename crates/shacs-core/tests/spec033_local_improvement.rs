#[path = "spec033_local_improvement_support.rs"]
mod support;

use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    CurrentGateEvidence, CurrentSpec030Receipts, LocalArtifactOwner, LocalGateSource,
    LocalImprovementBlock, LocalImprovementProposal, LocalImprovementRuntime,
    LocalImprovementStore,
};
use std::fs;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Barrier};
use support::{proposal, write_snapshot, Gates, Verifier};

struct MutatingGates {
    target: std::path::PathBuf,
}

impl LocalGateSource for MutatingGates {
    fn current_receipts(
        &self,
        proposal: &LocalImprovementProposal,
        target_digest: &str,
    ) -> Result<CurrentSpec030Receipts, LocalImprovementBlock> {
        fs::write(&self.target, br#"{"external":true}"#).map_err(|_| LocalImprovementBlock::Io)?;
        let evidence = |kind: &str| {
            CurrentGateEvidence::new(kind, &proposal.snapshot().provenance_digest, target_digest)
        };
        CurrentSpec030Receipts::try_new(
            evidence("hook"),
            evidence("confirmation"),
            evidence("process"),
            evidence("sandbox"),
            Some(evidence("credential")),
        )
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[test]
fn latest_status_uses_recording_order_not_proposal_id_order(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let snapshot = root.path().join("snapshot.json");
    let store_path = root.path().join("store.json");
    write_snapshot(&snapshot)?;
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(&store_path)?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    runtime.propose(proposal("proposal:z-old", &digest(b"old"), &snapshot)?)?;
    let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&store_path)?)?;
    legacy
        .as_object_mut()
        .expect("store document")
        .remove("next_sequence");
    legacy["records"]["proposal:z-old"]
        .as_object_mut()
        .expect("stored record")
        .remove("recorded_sequence");
    fs::write(&store_path, serde_json::to_vec_pretty(&legacy)?)?;
    let restarted = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(&store_path)?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    restarted.propose(proposal("proposal:a-new", &digest(b"new"), &snapshot)?)?;

    // When
    let status = LocalImprovementStore::open(store_path)?
        .latest_status()
        .expect("latest status");

    // Then
    assert_eq!(status.proposal.proposal_id(), "proposal:a-new");
    Ok(())
}

#[test]
fn local_apply_is_atomic_and_restart_loads_proposal_and_receipt(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, br#"{"enabled":false}"#)?;
    write_snapshot(&snapshot)?;
    let expected = digest(&fs::read(&target)?);
    let store_path = root.path().join("improvement-store.json");
    let gates = Arc::new(Gates::new());
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(&store_path)?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        gates,
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    runtime.propose(proposal("proposal:apply", &expected, &snapshot)?)?;

    let receipt = runtime.apply("proposal:apply")?;

    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&target)?)?,
        serde_json::json!({"enabled": true})
    );
    assert!(!receipt.owner_evidence_id().is_empty());
    let restarted = LocalImprovementStore::open(&store_path)?;
    assert!(restarted.proposal("proposal:apply").is_some());
    assert!(restarted.apply_receipt("proposal:apply").is_some());
    Ok(())
}

#[test]
fn invalid_snapshot_stale_target_and_missing_gate_evidence_never_mutate(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    let original = br#"{"enabled":false}"#;
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let mut snapshot_value: serde_json::Value = serde_json::from_slice(&fs::read(&snapshot)?)?;
    snapshot_value["snapshot_id"] = serde_json::json!("tampered");
    let invalid = support::proposal("proposal:invalid", &digest(original), &snapshot);
    fs::write(&snapshot, serde_json::to_vec(&snapshot_value)?)?;
    assert!(invalid.is_ok());
    assert!(support::proposal("proposal:tampered", &digest(original), &snapshot).is_err());

    write_snapshot(&snapshot)?;
    let gates = Arc::new(Gates::new());
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(root.path().join("store.json"))?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        gates.clone(),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    runtime.propose(proposal(
        "proposal:stale",
        &digest(b"different"),
        &snapshot,
    )?)?;
    assert!(matches!(
        runtime.apply("proposal:stale"),
        Err(LocalImprovementBlock::StaleTarget { .. })
    ));
    assert_eq!(gates.calls.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read(&target)?, original);

    runtime.propose(proposal("proposal:gated", &digest(original), &snapshot)?)?;
    gates.complete.store(false, Ordering::SeqCst);
    assert_eq!(
        runtime.apply("proposal:gated"),
        Err(LocalImprovementBlock::MissingGateEvidence)
    );
    assert_eq!(fs::read(&target)?, original);
    Ok(())
}

#[test]
fn independent_runtimes_serialize_apply_by_target() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, br#"{"enabled":false}"#)?;
    write_snapshot(&snapshot)?;
    let store_path = root.path().join("store.json");
    let first_owner = Arc::new(LocalArtifactOwner::new(root.path())?);
    let first = Arc::new(LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(&store_path)?),
        first_owner.clone(),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    ));
    first.propose(proposal(
        "proposal:race",
        &digest(&fs::read(&target)?),
        &snapshot,
    )?)?;
    let second_owner = Arc::new(LocalArtifactOwner::new(root.path())?);
    let second = Arc::new(LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(&store_path)?),
        second_owner.clone(),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    ));
    let barrier = Arc::new(Barrier::new(3));
    let handles = [first, second]
        .into_iter()
        .map(|runtime| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                runtime.apply("proposal:race")
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("apply thread"))
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        first_owner.mutation_count() + second_owner.mutation_count(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(target)?)?,
        serde_json::json!({"enabled": true})
    );
    Ok(())
}

#[test]
fn external_target_mutation_during_gates_blocks_apply_before_replace(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    let original = br#"{"enabled":false}"#;
    let external = br#"{"external":true}"#;
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let store = Arc::new(LocalImprovementStore::open(root.path().join("store.json"))?);
    let runtime = LocalImprovementRuntime::new(
        store.clone(),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        Arc::new(MutatingGates {
            target: target.clone(),
        }),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    runtime.propose(proposal(
        "proposal:external-race",
        &digest(original),
        &snapshot,
    )?)?;

    let result = runtime.apply("proposal:external-race");

    assert!(matches!(
        result,
        Err(LocalImprovementBlock::StaleTarget { .. })
    ));
    assert_eq!(fs::read(&target)?, external);
    assert!(store.apply_receipt("proposal:external-race").is_none());
    Ok(())
}

#[test]
fn verification_failure_only_records_candidate_and_rollback_requires_fresh_truth(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    let original = br#"{"enabled":false}"#;
    fs::write(&target, original)?;
    write_snapshot(&snapshot)?;
    let gates = Arc::new(Gates::new());
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(root.path().join("store.json"))?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        gates.clone(),
        Arc::new(Verifier {
            passes: false.into(),
        }),
    );
    runtime.propose(proposal("proposal:rollback", &digest(original), &snapshot)?)?;
    runtime.apply("proposal:rollback")?;
    let applied = fs::read(&target)?;

    assert!(!runtime.verify("proposal:rollback")?.passed());
    assert!(runtime.rollback_candidate("proposal:rollback").is_some());
    assert_eq!(fs::read(&target)?, applied);
    gates.current.store(false, Ordering::SeqCst);
    assert_eq!(
        runtime.rollback("proposal:rollback"),
        Err(LocalImprovementBlock::StaleGateEvidence)
    );
    assert_eq!(fs::read(&target)?, applied);
    gates.current.store(true, Ordering::SeqCst);
    fs::write(&target, br#"{"external":true}"#)?;
    assert!(matches!(
        runtime.rollback("proposal:rollback"),
        Err(LocalImprovementBlock::StaleTarget { .. })
    ));
    fs::write(&target, &applied)?;

    let receipt = runtime.rollback("proposal:rollback")?;

    assert!(!receipt.owner_evidence_id().is_empty());
    assert_eq!(fs::read(&target)?, original);
    Ok(())
}
