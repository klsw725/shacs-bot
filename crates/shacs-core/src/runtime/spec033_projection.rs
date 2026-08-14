use super::{GoalSurfaceError, LocalImprovementStore};
use shacs_eval::completion_boundary::{EvaluatorBoundaryRecord, EvaluatorRoute};
use shacs_projection::{
    Spec033EvaluatorFact, Spec033EvaluatorRoute, Spec033EvidenceSource, Spec033Owner,
    Spec033OwnerFact, Spec033ReplayFact, Spec033ReplayStatus, Spec033RollbackCandidateFact,
    Spec033SelfImprovementFact, Spec033Snapshot, Spec033VerifyFact,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::Session;
use std::path::{Path, PathBuf};

mod automation_projection;

pub(super) fn replay_receipt_root(data_dir: &Path) -> PathBuf {
    data_dir.join("replay-receipts")
}

pub(super) fn latest_evaluator_fact(session: &Session) -> Option<Spec033EvaluatorFact> {
    let record: EvaluatorBoundaryRecord = serde_json::from_value(
        session
            .metadata
            .get(super::GOAL_EVALUATOR_BOUNDARY_METADATA_KEY)?
            .as_array()?
            .last()?
            .clone(),
    )
    .ok()?;
    Some(Spec033EvaluatorFact {
        verdict: serde_json::to_value(record.output.verdict_kind)
            .ok()?
            .as_str()?
            .to_owned(),
        route: evaluator_route(record.route),
    })
}

pub(super) fn latest_evaluator_request_id(session: &Session) -> Option<String> {
    latest_evaluator_record(session).map(|record| record.input.request_id)
}

fn latest_evaluator_record(session: &Session) -> Option<EvaluatorBoundaryRecord> {
    serde_json::from_value(
        session
            .metadata
            .get(super::GOAL_EVALUATOR_BOUNDARY_METADATA_KEY)?
            .as_array()?
            .last()?
            .clone(),
    )
    .ok()
}

pub(super) fn populate_durable_facts(
    snapshot: &mut Spec033Snapshot,
    workspace: &Path,
    data_dir: &Path,
    session_id: &str,
) -> Result<(), GoalSurfaceError> {
    automation_projection::populate(snapshot, data_dir, session_id);
    populate_replay(snapshot, data_dir);
    populate_improvement(snapshot, workspace);
    Ok(())
}

fn populate_replay(snapshot: &mut Spec033Snapshot, data_dir: &Path) {
    let root = replay_receipt_root(data_dir);
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let receipt = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            std::fs::read(entry.path())
                .ok()
                .map(|bytes| (entry.path(), bytes))
        })
        .filter_map(|(path, bytes)| {
            serde_json::from_slice::<super::RecordedTrajectoryReplayReceipt>(&bytes)
                .ok()
                .map(|receipt| (path, receipt))
        })
        .max_by(|(left_path, left), (right_path, right)| {
            left.result
                .completed_at_ms
                .cmp(&right.result.completed_at_ms)
                .then(left.result.started_at_ms.cmp(&right.result.started_at_ms))
                .then(left_path.cmp(right_path))
        });
    let Some((path, receipt)) = receipt else {
        return;
    };
    let status = match receipt.result.status {
        shacs_eval::evaluator::ReplayRunStatus::Passed => Spec033ReplayStatus::Passed,
        shacs_eval::evaluator::ReplayRunStatus::Failed => Spec033ReplayStatus::Failed,
        shacs_eval::evaluator::ReplayRunStatus::Blocked => Spec033ReplayStatus::Blocked,
    };
    snapshot.replay = Spec033OwnerFact::available(
        Spec033Owner::Replay,
        Spec033EvidenceSource::DurableStore,
        Spec033ReplayFact {
            receipt_id: format!("replay-receipt:{}", receipt.result.run_id),
            correlation_id: receipt.correlation_id,
            trajectory_id: receipt.trajectory_id.clone(),
            status,
        },
        vec![format!(
            "replay_receipt:{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        )],
    );
    snapshot.diagnostics.trajectory_id =
        shacs_projection::Spec033DiagnosticLink::available(&receipt.trajectory_id);
    snapshot.diagnostics.execution_snapshot_id =
        shacs_projection::Spec033DiagnosticLink::available(&receipt.snapshot_id);
    snapshot.diagnostics.execution_snapshot_digest =
        shacs_projection::Spec033DiagnosticLink::available(&receipt.snapshot_digest);
}

fn populate_improvement(snapshot: &mut Spec033Snapshot, workspace: &Path) {
    let path = workspace.join(".shacs-self-improvement/store.json");
    if !path.exists() {
        return;
    }
    let Ok(store) = LocalImprovementStore::open(path) else {
        return;
    };
    let Some(status) = store.latest_status() else {
        return;
    };
    let proposal_id = status.proposal.proposal_id();
    let evidence = vec![format!("self_improvement:{proposal_id}")];
    snapshot.self_improvement = Spec033OwnerFact::available(
        Spec033Owner::SelfImprovement,
        Spec033EvidenceSource::DurableStore,
        Spec033SelfImprovementFact {
            proposal_id: proposal_id.to_owned(),
            applied: status.applied,
            rolled_back: status.rolled_back,
        },
        evidence.clone(),
    );
    if let Some(passed) = status.verification_passed {
        snapshot.verify = Spec033OwnerFact::available(
            Spec033Owner::Verify,
            Spec033EvidenceSource::DurableStore,
            Spec033VerifyFact {
                proposal_id: proposal_id.to_owned(),
                passed,
            },
            evidence.clone(),
        );
    }
    if let Some(candidate) = status.rollback_candidate {
        snapshot.rollback_candidate = Spec033OwnerFact::available(
            Spec033Owner::RollbackCandidate,
            Spec033EvidenceSource::DurableStore,
            Spec033RollbackCandidateFact {
                proposal_id: proposal_id.to_owned(),
                verify_failure_ref: candidate.verify_failure_ref().to_owned(),
            },
            evidence,
        );
    }
}

fn evaluator_route(route: EvaluatorRoute) -> Spec033EvaluatorRoute {
    match route {
        EvaluatorRoute::Notify => Spec033EvaluatorRoute::Notify,
        EvaluatorRoute::Suppress => Spec033EvaluatorRoute::Suppress,
        EvaluatorRoute::Continue => Spec033EvaluatorRoute::Continue,
        EvaluatorRoute::Escalate => Spec033EvaluatorRoute::Escalate,
        EvaluatorRoute::Verify => Spec033EvaluatorRoute::Verify,
        EvaluatorRoute::RollbackCandidate => Spec033EvaluatorRoute::RollbackCandidate,
    }
}
