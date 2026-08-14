use super::{
    run_local_replay, ExecutionSnapshot, RecordedBoundaryRequirement, RecordedTrajectoryStore,
    RecordedTrajectoryStoreError, RuntimeReplayInput,
};
use shacs_eval::evaluator::{EvidenceKind, EvidenceRef, RedactionStatus, ReplayRunRecord};
use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordedTrajectoryReplayReceipt {
    pub schema: String,
    pub correlation_id: String,
    pub redaction_status: RedactionStatus,
    pub trajectory_id: String,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    pub result: ReplayRunRecord,
    pub compared_recorded_outcomes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedTrajectoryReplayError {
    TrajectoryMissing,
    InvalidTrajectory,
    ArtifactDigestMismatch,
    SnapshotMismatch,
    SourceMutation,
    LiveBoundaryRequired,
}

pub fn replay_recorded_trajectory(
    store: &RecordedTrajectoryStore,
    trajectory_id: &str,
    run_id: &str,
) -> Result<RecordedTrajectoryReplayReceipt, RecordedTrajectoryReplayError> {
    let started_at_ms = unix_time_ms();
    let record = store.read(trajectory_id).map_err(map_store_read_error)?;
    match record.boundary_requirement {
        RecordedBoundaryRequirement::RecordedOnly => {}
        RecordedBoundaryRequirement::LiveDestructive => {
            return Err(RecordedTrajectoryReplayError::LiveBoundaryRequired);
        }
    }
    let snapshot_bytes = store
        .read_artifact(&record.snapshot)
        .map_err(map_artifact_error)?;
    let snapshot_json = std::str::from_utf8(&snapshot_bytes)
        .map_err(|_| RecordedTrajectoryReplayError::SnapshotMismatch)?;
    let snapshot = ExecutionSnapshot::parse_json(snapshot_json)
        .map_err(|_| RecordedTrajectoryReplayError::SnapshotMismatch)?;
    for source in &record.sources {
        let _bytes = store.read_source(source).map_err(map_artifact_error)?;
        let matches_snapshot = snapshot.context_sources.iter().any(|snapshot_source| {
            snapshot_source.source_ref == source.source_id
                && snapshot_source.content_digest == source.digest
        });
        if !matches_snapshot {
            return Err(RecordedTrajectoryReplayError::SourceMutation);
        }
    }
    if snapshot.context_sources.len() != record.sources.len() {
        return Err(RecordedTrajectoryReplayError::SourceMutation);
    }

    let selected_case_ids = vec![record.owner_outcome.case_id.clone()];
    let dataset_id = record.owner_outcome.dataset_id.clone();
    let dataset = vec![record.owner_outcome];
    let outcome = run_local_replay(RuntimeReplayInput {
        run_id: run_id.to_owned(),
        dataset_id,
        dataset: &dataset,
        selected_case_ids: &selected_case_ids,
        started_at_ms,
        completed_at_ms: unix_time_ms(),
        diagnostics_ref: EvidenceRef {
            kind: EvidenceKind::ReplayRecord,
            id: trajectory_id.to_owned(),
            digest: record.record_digest,
            summary: "recorded trajectory replay".to_owned(),
            redaction_status: RedactionStatus::AlreadySafe,
            owner_spec: Some("033".to_owned()),
            locator: Some(format!("trajectory:{trajectory_id}")),
            retention_hint: Some("local".to_owned()),
        },
    });
    Ok(RecordedTrajectoryReplayReceipt {
        schema: "spec033.replay_receipt.v1".to_owned(),
        correlation_id: run_id.to_owned(),
        redaction_status: RedactionStatus::AlreadySafe,
        trajectory_id: trajectory_id.to_owned(),
        snapshot_id: snapshot.snapshot_id,
        snapshot_digest: snapshot.provenance_digest,
        compared_recorded_outcomes: outcome.run_record.case_results.len(),
        result: outcome.run_record,
    })
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn map_store_read_error(error: RecordedTrajectoryStoreError) -> RecordedTrajectoryReplayError {
    match error {
        RecordedTrajectoryStoreError::Io(io_error)
            if io_error.kind() == std::io::ErrorKind::NotFound =>
        {
            RecordedTrajectoryReplayError::TrajectoryMissing
        }
        RecordedTrajectoryStoreError::DigestMismatch => {
            RecordedTrajectoryReplayError::ArtifactDigestMismatch
        }
        RecordedTrajectoryStoreError::InvalidId
        | RecordedTrajectoryStoreError::InvalidRecord
        | RecordedTrajectoryStoreError::Io(_)
        | RecordedTrajectoryStoreError::Json(_) => RecordedTrajectoryReplayError::InvalidTrajectory,
    }
}

fn map_artifact_error(error: RecordedTrajectoryStoreError) -> RecordedTrajectoryReplayError {
    match error {
        RecordedTrajectoryStoreError::DigestMismatch => {
            RecordedTrajectoryReplayError::ArtifactDigestMismatch
        }
        RecordedTrajectoryStoreError::InvalidId
        | RecordedTrajectoryStoreError::InvalidRecord
        | RecordedTrajectoryStoreError::Io(_)
        | RecordedTrajectoryStoreError::Json(_) => RecordedTrajectoryReplayError::InvalidTrajectory,
    }
}

impl Display for RecordedTrajectoryReplayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RecordedTrajectoryReplayError {}
