mod envelope;
mod parts;

use crate::{OutboundMessage, WebSocketServerEvent, WEBSOCKET_CHANNEL};
use envelope::ChannelProjectionParts;
use parts::{
    external_final_projection, final_failed, malformed_projection, pending, progress_live,
    progress_projection, unsupported_projection,
};
use serde_json::Value;
use shacs_projection::spec031::{
    Spec031ConstructionError, Spec031Count, Spec031Envelope, Spec031ObservedAtUnixMs,
    Spec031ProgressCapability, Spec031ProgressDelivery,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelDeliveryObservation {
    pub queue_depth: Option<u64>,
    pub queue_capacity: Option<u64>,
    pub accepted: Option<u64>,
    pub emitted: Option<u64>,
    pub coalesced: Option<u64>,
    pub dropped: Option<u64>,
    pub reconnect_generation: Option<u64>,
    pub reconnect_gap: Option<bool>,
    pub slow_consumer: Option<u64>,
}

impl ChannelDeliveryObservation {
    pub const fn unavailable() -> Self {
        Self {
            queue_depth: None,
            queue_capacity: None,
            accepted: None,
            emitted: None,
            coalesced: None,
            dropped: None,
            reconnect_generation: None,
            reconnect_gap: None,
            slow_consumer: None,
        }
    }

    pub(crate) fn apply_to(
        self,
        mut capability: Spec031ProgressCapability,
    ) -> Spec031ProgressCapability {
        capability.queue_depth = self.queue_depth.map(Spec031Count::new);
        capability.queue_capacity = self.queue_capacity.map(Spec031Count::new);
        capability.accepted = self.accepted.map(Spec031Count::new);
        capability.emitted = self.emitted.map(Spec031Count::new);
        capability.coalesced = self.coalesced.map(Spec031Count::new);
        capability.dropped = self.dropped.map(Spec031Count::new);
        capability.reconnect_generation = self.reconnect_generation.map(Spec031Count::new);
        capability.reconnect_gap = self.reconnect_gap;
        capability.slow_consumer = self.slow_consumer.map(Spec031Count::new);
        capability
    }
}

pub fn channel_delivery_observation_from_metadata(value: &Value) -> ChannelDeliveryObservation {
    match channel_delivery_status_from_metadata(value) {
        Some("pending") => ChannelDeliveryObservation {
            accepted: Some(1),
            ..ChannelDeliveryObservation::unavailable()
        },
        Some("sent") | Some("sent_hint") | Some("processed") => ChannelDeliveryObservation {
            emitted: Some(1),
            ..ChannelDeliveryObservation::unavailable()
        },
        Some("failed") | Some("failed_hint") => ChannelDeliveryObservation {
            dropped: Some(1),
            slow_consumer: Some(1),
            ..ChannelDeliveryObservation::unavailable()
        },
        Some("dedupe_candidate") => ChannelDeliveryObservation {
            coalesced: Some(1),
            ..ChannelDeliveryObservation::unavailable()
        },
        Some("unknown") | None | Some(_) => ChannelDeliveryObservation::unavailable(),
    }
}

pub(crate) fn channel_delivery_status_from_metadata(value: &Value) -> Option<&str> {
    value
        .get("delivery_projection_status")
        .or_else(|| value.get("status"))
        .or_else(|| value.get("delivery_status"))
        .and_then(Value::as_str)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelSpec031ProjectionInput {
    pub kind: ChannelSpec031ProjectionKind,
    pub observed_at_unix_ms: Option<u64>,
    pub delivery_observation: ChannelDeliveryObservation,
}

impl ChannelSpec031ProjectionInput {
    pub fn websocket_event(event: WebSocketServerEvent) -> Self {
        Self::new(ChannelSpec031ProjectionKind::WebSocketEvent(event))
    }

    pub fn external_final(message: OutboundMessage) -> Self {
        Self::new(ChannelSpec031ProjectionKind::ExternalFinal(message))
    }

    pub fn unsupported(channel: &str, capability: &str) -> Self {
        Self::new(ChannelSpec031ProjectionKind::Unsupported {
            channel: channel.to_owned(),
            capability: capability.to_owned(),
        })
    }

    pub fn malformed_frame(channel: &str, detail: &str) -> Self {
        Self::new(ChannelSpec031ProjectionKind::MalformedFrame {
            channel: channel.to_owned(),
            detail: detail.to_owned(),
        })
    }

    pub fn disconnected(channel: &str, chat_id: Option<&str>) -> Self {
        Self::new(ChannelSpec031ProjectionKind::Disconnected {
            channel: channel.to_owned(),
            chat_id: chat_id.map(str::to_owned),
        })
    }

    pub fn progress_delivery(
        channel: &str,
        delivery: Spec031ProgressDelivery,
        stream_id: Option<&str>,
    ) -> Self {
        Self::new(ChannelSpec031ProjectionKind::ProgressDelivery {
            channel: channel.to_owned(),
            delivery,
            stream_id: stream_id.map(str::to_owned),
        })
    }

    pub const fn with_delivery_observation(
        mut self,
        observation: ChannelDeliveryObservation,
    ) -> Self {
        self.delivery_observation = observation;
        self
    }

    const fn new(kind: ChannelSpec031ProjectionKind) -> Self {
        Self {
            kind,
            observed_at_unix_ms: None,
            delivery_observation: ChannelDeliveryObservation::unavailable(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelSpec031ProjectionKind {
    WebSocketEvent(WebSocketServerEvent),
    ExternalFinal(OutboundMessage),
    Unsupported {
        channel: String,
        capability: String,
    },
    MalformedFrame {
        channel: String,
        detail: String,
    },
    Disconnected {
        channel: String,
        chat_id: Option<String>,
    },
    ProgressDelivery {
        channel: String,
        delivery: Spec031ProgressDelivery,
        stream_id: Option<String>,
    },
}

pub fn project_spec031_channel_event(
    input: ChannelSpec031ProjectionInput,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    let observed_at_unix_ms = input.observed_at_unix_ms.map(Spec031ObservedAtUnixMs::new);
    let mut event = match input.kind {
        ChannelSpec031ProjectionKind::WebSocketEvent(event) => websocket_projection(event),
        ChannelSpec031ProjectionKind::ExternalFinal(message) => external_final_projection(message),
        ChannelSpec031ProjectionKind::Unsupported {
            channel,
            capability,
        } => unsupported_projection(channel, capability),
        ChannelSpec031ProjectionKind::MalformedFrame { channel, detail } => {
            malformed_projection(channel, detail)
        }
        ChannelSpec031ProjectionKind::Disconnected { channel, chat_id } => final_failed(
            &channel,
            "disconnect",
            "client disconnected".to_owned(),
            chat_id
                .as_deref()
                .and_then(|id| parts::chat_ref(&channel, id)),
        ),
        ChannelSpec031ProjectionKind::ProgressDelivery {
            channel, delivery, ..
        } => progress_projection(channel, delivery),
    };
    if input.delivery_observation != ChannelDeliveryObservation::unavailable() {
        event.delivery_observation = input.delivery_observation;
    }
    event.into_envelope(observed_at_unix_ms)
}

fn websocket_projection(event: WebSocketServerEvent) -> ChannelProjectionParts {
    match event {
        WebSocketServerEvent::Ready { chat_id, .. } => pending(
            WEBSOCKET_CHANNEL,
            "ready",
            parts::chat_ref(WEBSOCKET_CHANNEL, &chat_id),
            None,
        ),
        WebSocketServerEvent::Attached { chat_id } => pending(
            WEBSOCKET_CHANNEL,
            "attached",
            parts::chat_ref(WEBSOCKET_CHANNEL, &chat_id),
            None,
        ),
        WebSocketServerEvent::Message {
            chat_id, reply_to, ..
        } => parts::final_delivered(
            WEBSOCKET_CHANNEL,
            "message",
            parts::chat_ref(WEBSOCKET_CHANNEL, &chat_id),
            reply_to.map(|value| format!("action:channel:websocket:reply:{value}")),
        ),
        WebSocketServerEvent::Delta {
            chat_id, stream_id, ..
        } => progress_live(
            WEBSOCKET_CHANNEL,
            "delta",
            parts::chat_ref(WEBSOCKET_CHANNEL, &chat_id),
            stream_id.map(|value| format!("action:channel:websocket:stream:{value}")),
        ),
        WebSocketServerEvent::StreamEnd { chat_id, stream_id } => pending(
            WEBSOCKET_CHANNEL,
            "stream_end",
            parts::chat_ref(WEBSOCKET_CHANNEL, &chat_id),
            stream_id.map(|value| format!("action:channel:websocket:stream:{value}")),
        ),
        WebSocketServerEvent::Error {
            chat_id, detail, ..
        } => parts::error_projection(
            WEBSOCKET_CHANNEL,
            detail.unwrap_or_else(|| "websocket error".to_owned()),
            chat_id
                .as_deref()
                .and_then(|id| parts::chat_ref(WEBSOCKET_CHANNEL, id)),
        ),
    }
}
