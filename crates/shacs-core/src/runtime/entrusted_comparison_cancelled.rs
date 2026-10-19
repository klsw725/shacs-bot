use std::path::Path;

use shacs_session::durable_event::{DurableEventPayload, DurableEventRecord};
use shacs_session::durable_work::{
    DurableWorkPayloadStore, DurableWorkReplayState, WorkCancellation,
};

use super::{
    DurableDispatchError, ObservationBlock, ObservationDecision, ObservationRecord,
    ObservationResult, ObservationWake,
};

pub(super) fn from_event(
    root: &Path,
    event: &DurableEventRecord,
    state: &DurableWorkReplayState,
) -> Result<ObservationDecision, DurableDispatchError> {
    let DurableEventPayload::Inline { data, .. } = &event.payload else {
        return Err(DurableDispatchError::InvalidWork(
            "cancellation requires inline facts".to_owned(),
        ));
    };
    let cancelled: WorkCancellation = serde_json::from_value(data.clone())?;
    let item = state
        .items
        .get(&cancelled.work_id)
        .ok_or_else(|| DurableDispatchError::MissingWork(cancelled.work_id.clone()))?;
    let runtime = root
        .parent()
        .ok_or_else(|| DurableDispatchError::InvalidWork("missing runtime root".to_owned()))?;
    let payloads = DurableWorkPayloadStore::new(runtime.join("work-payloads"));
    let wake: ObservationWake = serde_json::from_value(payloads.read_json(&item.payload_ref)?)?;
    let reason = match cancelled.reason.as_str() {
        "observation:GoalInactive" => ObservationBlock::GoalInactive,
        "observation:StaleLineage" => ObservationBlock::StaleLineage,
        "observation:ConsentMissing" => ObservationBlock::ConsentMissing,
        _ => ObservationBlock::Cancelled,
    };
    let evaluated_at_ms = chrono::DateTime::parse_from_rfc3339(&event.recorded_at)
        .ok()
        .and_then(|time| u64::try_from(time.timestamp_millis()).ok())
        .ok_or_else(|| {
            DurableDispatchError::InvalidWork("invalid cancellation timestamp".to_owned())
        })?;
    let mut decision = super::entrusted_comparison_evaluate::compare(
        &ObservationRecord {
            wake,
            observed_at_ms: evaluated_at_ms,
            result: ObservationResult::Blocked { reason },
            execution_snapshot_ref: None,
            next_wake_at_ms: None,
            decision: None,
        },
        None,
        None,
    );
    decision.basis = super::ObservationDecisionBasis::CancellationFact {
        event_id: event.event_id.clone(),
    };
    Ok(decision)
}
