use super::envelope::ChannelProjectionParts;
use super::{channel_delivery_observation_from_metadata, channel_delivery_status_from_metadata};
use crate::OutboundMessage;
use shacs_projection::spec031::{
    Spec031Availability, Spec031Freshness, Spec031ProgressDelivery, Spec031ReasonCode,
    Spec031Severity,
};

pub(super) fn external_final_projection(message: OutboundMessage) -> ChannelProjectionParts {
    if let Some(status) =
        channel_delivery_status_from_metadata(&serde_json::Value::Object(message.metadata.clone()))
    {
        let mut parts = external_delivery_status_projection(&message, status);
        parts.delivery_observation = channel_delivery_observation_from_metadata(
            &serde_json::Value::Object(message.metadata.clone()),
        );
        return parts;
    }
    let parent_ref = external_parent_ref(&message);
    let action_ref = reply_ref(&message, "action");
    final_delivered(&message.channel, "external_final", parent_ref, action_ref)
}

fn external_delivery_status_projection(
    message: &OutboundMessage,
    status: &str,
) -> ChannelProjectionParts {
    let parent_ref = external_parent_ref(message);
    let action_ref = reply_ref(message, "action");
    match status {
        "pending" => pending(&message.channel, "external_final", parent_ref, action_ref),
        "sent" | "sent_hint" | "processed" => {
            final_delivered(&message.channel, "external_final", parent_ref, action_ref)
        }
        "failed" | "failed_hint" => final_failed(
            &message.channel,
            "external_final",
            "external channel send failed".to_owned(),
            parent_ref,
        ),
        "unknown" | "dedupe_candidate" => parts(
            message.channel.clone(),
            "external_final",
            Spec031ProgressDelivery::FinalUnknown,
            Spec031Availability::Unknown,
            Spec031ReasonCode::Degraded,
            "external channel delivery status is unknown".to_owned(),
        )
        .with_lineage(parent_ref, action_ref),
        _ => parts(
            message.channel.clone(),
            "external_final",
            Spec031ProgressDelivery::FinalUnknown,
            Spec031Availability::Unavailable,
            Spec031ReasonCode::Unsupported,
            "external channel delivery metadata is unavailable".to_owned(),
        )
        .with_freshness(Spec031Freshness::Unavailable)
        .with_lineage(parent_ref, action_ref),
    }
}

fn external_parent_ref(message: &OutboundMessage) -> Option<String> {
    message
        .metadata
        .get("thread_id")
        .or_else(|| message.metadata.get("thread_ts"))
        .map(|_| format!("parent:channel:{}:thread", message.channel))
        .or_else(|| reply_ref(message, "parent"))
}

pub(super) fn unsupported_projection(
    channel: String,
    capability: String,
) -> ChannelProjectionParts {
    parts(
        channel,
        "unsupported",
        Spec031ProgressDelivery::FinalFailed,
        Spec031Availability::Unavailable,
        Spec031ReasonCode::Unsupported,
        format!("unsupported channel capability: {capability}"),
    )
    .with_freshness(Spec031Freshness::Unavailable)
}

pub(super) fn malformed_projection(channel: String, detail: String) -> ChannelProjectionParts {
    final_failure(
        channel,
        "malformed",
        Spec031ReasonCode::ExtractionFailed,
        detail,
    )
}

pub(super) fn progress_projection(
    channel: String,
    delivery: Spec031ProgressDelivery,
) -> ChannelProjectionParts {
    parts(
        channel,
        "progress",
        delivery,
        Spec031Availability::Degraded,
        Spec031ReasonCode::Degraded,
        format!("channel progress is {delivery:?}").to_lowercase(),
    )
}

pub(super) fn progress_live(
    channel: &str,
    event_kind: &'static str,
    parent_ref: Option<String>,
    action_ref: Option<String>,
) -> ChannelProjectionParts {
    parts(
        channel.to_owned(),
        event_kind,
        Spec031ProgressDelivery::Live,
        Spec031Availability::Degraded,
        Spec031ReasonCode::Degraded,
        "degraded live progress update is non terminal".to_owned(),
    )
    .with_lineage(parent_ref, action_ref)
}

pub(super) fn pending(
    channel: &str,
    event_kind: &'static str,
    parent_ref: Option<String>,
    action_ref: Option<String>,
) -> ChannelProjectionParts {
    parts(
        channel.to_owned(),
        event_kind,
        Spec031ProgressDelivery::FinalPending,
        Spec031Availability::Degraded,
        Spec031ReasonCode::Degraded,
        "degraded stream_end is a presentation marker; final outcome pending".to_owned(),
    )
    .with_lineage(parent_ref, action_ref)
}

pub(super) fn final_delivered(
    channel: &str,
    event_kind: &'static str,
    parent_ref: Option<String>,
    action_ref: Option<String>,
) -> ChannelProjectionParts {
    parts(
        channel.to_owned(),
        event_kind,
        Spec031ProgressDelivery::FinalDelivered,
        Spec031Availability::Ready,
        Spec031ReasonCode::Included,
        "included final channel message accepted by local transport".to_owned(),
    )
    .with_lineage(parent_ref, action_ref)
}

pub(super) fn final_failed(
    channel: &str,
    event_kind: &'static str,
    detail: String,
    parent_ref: Option<String>,
) -> ChannelProjectionParts {
    final_failure(
        channel.to_owned(),
        event_kind,
        Spec031ReasonCode::Blocked,
        detail,
    )
    .with_lineage(parent_ref, None)
}

pub(super) fn error_projection(
    channel: &str,
    detail: String,
    parent_ref: Option<String>,
) -> ChannelProjectionParts {
    let reason_code = if detail.contains("valid JSON") {
        Spec031ReasonCode::ExtractionFailed
    } else if detail.contains("not configured") || detail.contains("not implemented") {
        Spec031ReasonCode::Unsupported
    } else {
        Spec031ReasonCode::Blocked
    };
    final_failure(channel.to_owned(), "error", reason_code, detail).with_lineage(parent_ref, None)
}

pub(super) fn chat_ref(channel: &str, chat_id: &str) -> Option<String> {
    Some(format!("parent:channel:{channel}:chat:{chat_id}"))
}

fn final_failure(
    channel: String,
    event_kind: &'static str,
    reason_code: Spec031ReasonCode,
    safe_summary: String,
) -> ChannelProjectionParts {
    parts(
        channel,
        event_kind,
        Spec031ProgressDelivery::FinalFailed,
        Spec031Availability::Blocked,
        reason_code,
        safe_summary,
    )
}

fn parts(
    channel: String,
    event_kind: &'static str,
    delivery: Spec031ProgressDelivery,
    state: Spec031Availability,
    reason_code: Spec031ReasonCode,
    safe_summary: String,
) -> ChannelProjectionParts {
    let severity = match state {
        Spec031Availability::Ready => Spec031Severity::Info,
        Spec031Availability::Degraded | Spec031Availability::Unknown => Spec031Severity::Warning,
        Spec031Availability::Blocked | Spec031Availability::Unavailable => Spec031Severity::Error,
    };
    ChannelProjectionParts {
        channel,
        event_kind,
        delivery,
        state,
        severity,
        reason_code,
        safe_summary,
        parent_ref: None,
        action_ref: None,
        freshness: Spec031Freshness::Current,
        delivery_observation: super::ChannelDeliveryObservation::unavailable(),
    }
}

fn reply_ref(message: &OutboundMessage, kind: &str) -> Option<String> {
    message
        .reply_to
        .as_ref()
        .map(|_| format!("{kind}:channel:{}:reply", message.channel))
}
