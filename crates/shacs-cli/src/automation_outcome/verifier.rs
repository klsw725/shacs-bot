use super::owner_support::{open_dispatcher, ROUTE_WORK_KIND};
use shacs_channels::InboundMessage;
use shacs_core::runtime::DurableWorkDispatcher;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::WorkTerminalKind;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn consume_verification_requests(
    dispatcher: &mut DurableWorkDispatcher,
    data_dir: &Path,
) -> Result<(), String> {
    let recovery = evaluate_durable_recovery(
        data_dir.join("runtime/durable-events"),
        data_dir.join("runtime/durable-checkpoints"),
    );
    if !recovery.writable {
        return Err("automation verifier durable store is not writable".to_owned());
    }
    let Some(state) = recovery.state else {
        return Ok(());
    };
    for item in state.work.items.values().filter(|item| {
        item.work_kind == ROUTE_WORK_KIND
            && !item.state.is_terminal()
            && item.work_id.starts_with("automation-route-verify-")
    }) {
        let payload = dispatcher
            .read_payload_json(item)
            .map_err(|error| error.to_string())?;
        let correlation = required(&payload, "correlation_id")?;
        let result_ref = required(&payload, "result_ref")?;
        let (channel, chat_id) = item
            .session_key
            .split_once(':')
            .ok_or_else(|| "automation verifier target is unsupported".to_owned())?;
        let mut metadata = serde_json::Map::new();
        metadata.insert("automation_route".to_owned(), serde_json::json!("verify"));
        metadata.insert("correlation_id".to_owned(), serde_json::json!(correlation));
        metadata.insert("result_ref".to_owned(), serde_json::json!(result_ref));
        let message = InboundMessage::new(
            channel,
            "automation-verifier",
            chat_id,
            "Verify the correlated automation result and report evidence.",
        )
        .with_metadata(metadata)
        .with_session_key_override(&item.session_key);
        dispatcher
            .lease_work(item, now_ms())
            .map_err(|error| error.to_string())?;
        open_dispatcher(data_dir)?
            .enqueue_inbound(
                format!("{}-follow-up", item.work_id),
                &message,
                Some(correlation.to_owned()),
                None,
            )
            .map_err(|error| error.to_string())?;
        dispatcher
            .record_terminal(item, WorkTerminalKind::Succeeded, "verification_enqueued")
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn required<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("automation verifier request lacks {key}"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}
