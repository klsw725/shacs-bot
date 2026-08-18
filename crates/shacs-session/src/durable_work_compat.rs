use crate::durable_event::{DurableEventPayload, DurableEventRecord, WORK_TERMINAL};
use crate::durable_work::{
    project_terminal_item, prune_terminal_items, DurableWorkReducerError, DurableWorkReplayState,
    ReplayWorkItem, ReplayWorkState, WorkPayloadRef, WorkTerminal, WorkTerminalKind,
};

const EVENT_SCHEMA_VERSION: u32 = 1;
const EVENT_PAYLOAD_TYPE: &str = "durable_work";
const WORK_KIND: &str = "automation.owner_request";
const WORK_PAYLOAD_TYPE: &str = "shacs.automation_owner_request.v1";
const TERMINAL_OUTCOME: &str = "no_notification";

pub(crate) fn apply_legacy_v1_owner_terminal(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<bool, DurableWorkReducerError> {
    if event.schema_version != EVENT_SCHEMA_VERSION || event.kind != WORK_TERMINAL {
        return Ok(false);
    }
    let DurableEventPayload::Inline { payload_type, data } = &event.payload else {
        return Ok(false);
    };
    if payload_type != EVENT_PAYLOAD_TYPE {
        return Ok(false);
    }
    let Ok(payload) = serde_json::from_value::<WorkTerminal>(data.clone()) else {
        return Ok(false);
    };
    if payload.terminal_kind != WorkTerminalKind::Succeeded
        || payload.outcome_ref != TERMINAL_OUTCOME
        || payload.facts.is_some()
    {
        return Ok(false);
    }
    let Some(item) = state.items.get_mut(&payload.work_id) else {
        return Ok(false);
    };
    if !legacy_owner_request_matches(item, event) {
        return Ok(false);
    }
    project_terminal_item(item, event, payload);
    prune_terminal_items(state);
    Ok(true)
}

fn legacy_owner_request_matches(item: &ReplayWorkItem, event: &DurableEventRecord) -> bool {
    let WorkPayloadRef::Inline {
        payload_type, data, ..
    } = &item.payload_ref
    else {
        return false;
    };
    let Some(effect_id) = item.effect_id.as_deref() else {
        return false;
    };
    item.work_kind == WORK_KIND
        && payload_type == WORK_PAYLOAD_TYPE
        && data.get("route").and_then(serde_json::Value::as_str) == Some("suppress")
        && data
            .get("correlation_id")
            .and_then(serde_json::Value::as_str)
            == Some(effect_id)
        && item.dedupe_hint.as_deref() == Some(effect_id)
        && item.session_key == event.session_id
        && item.turn_id == event.turn_id
        && item.state == ReplayWorkState::Pending
        && item.attempt == 0
        && item.updated_sequence == item.enqueued_sequence
        && item.next_wake_at_ms.is_none()
        && item.lease_id.is_none()
        && item.lease_owner_ref.is_none()
        && item.lease_expires_at_ms.is_none()
        && item.cancellation_requested_sequence.is_none()
        && item.terminal_kind.is_none()
        && item.terminal_facts.is_none()
        && item.terminal_sequence.is_none()
        && item.terminal_at.is_none()
}
