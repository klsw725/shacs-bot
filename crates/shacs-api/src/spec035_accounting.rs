use crate::{ApiError, ReconnectObservation};
use serde_json::Value;
use shacs_channels::{
    project_spec031_channel_event, ChannelDeliveryObservation, ChannelSpec031ProjectionInput,
    WebSocketServerEvent,
};
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec031Capability, Spec031Envelope, Spec031ProgressDelivery, Spec035OwnerSurface,
    Spec035RevisedOwnerFacts,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct ReconnectDeliveryAccounting {
    progress: Option<ProgressAccounting>,
    final_delivery: Option<Spec031Envelope>,
}

#[derive(Debug, Clone, Copy)]
struct ProgressAccounting {
    delivery: Spec031ProgressDelivery,
    observation: ChannelDeliveryObservation,
}

impl ReconnectDeliveryAccounting {
    pub(crate) fn observe(
        &mut self,
        channel: &str,
        delivery: Spec031ProgressDelivery,
        observation: ChannelDeliveryObservation,
    ) -> Result<Spec031Envelope, ApiError> {
        let envelope = project_spec031_channel_event(
            ChannelSpec031ProjectionInput::progress_delivery(channel, delivery, None)
                .with_delivery_observation(observation),
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
        self.observe_envelope(envelope.clone());
        Ok(envelope)
    }

    pub(crate) fn observe_envelope(&mut self, envelope: Spec031Envelope) {
        let Some((delivery, observation)) = delivery_fact(&envelope) else {
            return;
        };
        match delivery {
            Spec031ProgressDelivery::Live
            | Spec031ProgressDelivery::Coalesced
            | Spec031ProgressDelivery::Dropped
            | Spec031ProgressDelivery::Reconnected => {
                self.progress = Some(ProgressAccounting {
                    delivery,
                    observation,
                });
            }
            Spec031ProgressDelivery::FinalDelivered
            | Spec031ProgressDelivery::FinalPending
            | Spec031ProgressDelivery::FinalFailed
            | Spec031ProgressDelivery::FinalUnknown => {
                self.final_delivery = Some(envelope);
            }
        }
    }

    pub(crate) fn projection(&self) -> Result<Option<Value>, ApiError> {
        if self.progress.is_none() && self.final_delivery.is_none() {
            return Ok(None);
        }
        let runtime =
            Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
        let mut facts = Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Websocket, &runtime);
        let progress = self
            .progress
            .map(|progress| {
                project_spec031_channel_event(
                    ChannelSpec031ProjectionInput::progress_delivery(
                        shacs_channels::WEBSOCKET_CHANNEL,
                        progress.delivery,
                        None,
                    )
                    .with_delivery_observation(progress.observation),
                )
                .map_err(|error| ApiError::internal(error.to_string()))
            })
            .transpose()?;
        if let Some(progress) = progress.as_ref() {
            facts = facts.with_progress_delivery(progress);
        }
        if let Some(final_delivery) = self.final_delivery.as_ref() {
            facts = facts.with_final_delivery(final_delivery);
        }
        project_spec035_revised_owner_facts(facts)
            .map_err(|error| ApiError::internal(error.to_string()))
            .and_then(|projection| {
                serde_json::to_value(projection)
                    .map(Some)
                    .map_err(|error| ApiError::internal(error.to_string()))
            })
    }

    pub(crate) fn observation(&self) -> ChannelDeliveryObservation {
        self.progress
            .map(|progress| progress.observation)
            .or_else(|| {
                self.final_delivery
                    .as_ref()
                    .and_then(delivery_fact)
                    .map(|(_, observation)| observation)
            })
            .unwrap_or_else(ChannelDeliveryObservation::unavailable)
    }

    pub(crate) fn sent_observation(
        &self,
        update: ChannelDeliveryObservation,
        reconnect: ReconnectObservation,
    ) -> ChannelDeliveryObservation {
        let mut observation = merge_observations(self.observation(), update);
        observation.emitted = increment(observation.emitted, 1);
        observation.reconnect_generation = Some(reconnect.generation);
        observation.reconnect_gap = Some(reconnect.gap);
        observation
    }

    pub(crate) fn failed_observation(
        &self,
        reconnect: ReconnectObservation,
    ) -> ChannelDeliveryObservation {
        let mut observation = self.observation();
        observation.dropped = increment(observation.dropped, 1);
        observation.slow_consumer = increment(observation.slow_consumer, 1);
        observation.reconnect_generation = Some(reconnect.generation);
        observation.reconnect_gap = Some(reconnect.gap);
        observation
    }

    pub(crate) fn accumulated_observation(
        &self,
        update: ChannelDeliveryObservation,
    ) -> ChannelDeliveryObservation {
        merge_observations(self.observation(), update)
    }

    pub(crate) fn merge_from(&mut self, incoming: &Self) {
        self.progress = match (self.progress, incoming.progress) {
            (Some(current), Some(incoming)) => Some(ProgressAccounting {
                delivery: dominant_progress(current.delivery, incoming.delivery),
                observation: max_observations(current.observation, incoming.observation),
            }),
            (None, Some(incoming)) => Some(incoming),
            (current, None) => current,
        };
        if let Some(final_delivery) = incoming.final_delivery.as_ref() {
            self.final_delivery = Some(final_delivery.clone());
        }
    }
}

pub(crate) const fn websocket_send_failure_delivery(
    event: &WebSocketServerEvent,
) -> Option<Spec031ProgressDelivery> {
    match event {
        WebSocketServerEvent::Message { .. } => Some(Spec031ProgressDelivery::FinalFailed),
        WebSocketServerEvent::Delta { .. } | WebSocketServerEvent::StreamEnd { .. } => {
            Some(Spec031ProgressDelivery::Dropped)
        }
        WebSocketServerEvent::Ready { .. }
        | WebSocketServerEvent::Attached { .. }
        | WebSocketServerEvent::Error { .. } => None,
    }
}

fn merge_observations(
    prior: ChannelDeliveryObservation,
    update: ChannelDeliveryObservation,
) -> ChannelDeliveryObservation {
    ChannelDeliveryObservation {
        queue_depth: update.queue_depth.or(prior.queue_depth),
        queue_capacity: update.queue_capacity.or(prior.queue_capacity),
        accepted: add(prior.accepted, update.accepted),
        emitted: add(prior.emitted, update.emitted),
        coalesced: add(prior.coalesced, update.coalesced),
        dropped: add(prior.dropped, update.dropped),
        reconnect_generation: update.reconnect_generation.or(prior.reconnect_generation),
        reconnect_gap: update.reconnect_gap.or(prior.reconnect_gap),
        slow_consumer: add(prior.slow_consumer, update.slow_consumer),
    }
}

fn max_observations(
    current: ChannelDeliveryObservation,
    incoming: ChannelDeliveryObservation,
) -> ChannelDeliveryObservation {
    ChannelDeliveryObservation {
        queue_depth: incoming.queue_depth.or(current.queue_depth),
        queue_capacity: incoming.queue_capacity.or(current.queue_capacity),
        accepted: maximum(current.accepted, incoming.accepted),
        emitted: maximum(current.emitted, incoming.emitted),
        coalesced: maximum(current.coalesced, incoming.coalesced),
        dropped: maximum(current.dropped, incoming.dropped),
        reconnect_generation: incoming
            .reconnect_generation
            .or(current.reconnect_generation),
        reconnect_gap: incoming.reconnect_gap.or(current.reconnect_gap),
        slow_consumer: maximum(current.slow_consumer, incoming.slow_consumer),
    }
}

const fn maximum(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left > right { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

const fn dominant_progress(
    current: Spec031ProgressDelivery,
    incoming: Spec031ProgressDelivery,
) -> Spec031ProgressDelivery {
    match (current, incoming) {
        (Spec031ProgressDelivery::Dropped, _) | (_, Spec031ProgressDelivery::Dropped) => {
            Spec031ProgressDelivery::Dropped
        }
        (Spec031ProgressDelivery::Coalesced, _) | (_, Spec031ProgressDelivery::Coalesced) => {
            Spec031ProgressDelivery::Coalesced
        }
        (_, incoming) => incoming,
    }
}

const fn add(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

const fn increment(value: Option<u64>, amount: u64) -> Option<u64> {
    match value {
        Some(value) => Some(value.saturating_add(amount)),
        None => Some(amount),
    }
}

fn delivery_fact(
    envelope: &Spec031Envelope,
) -> Option<(Spec031ProgressDelivery, ChannelDeliveryObservation)> {
    let Spec031Capability::Progress(progress) = envelope.capability() else {
        return None;
    };
    Some((
        progress.delivery,
        ChannelDeliveryObservation {
            queue_depth: progress.queue_depth.map(|value| value.as_u64()),
            queue_capacity: progress.queue_capacity.map(|value| value.as_u64()),
            accepted: progress.accepted.map(|value| value.as_u64()),
            emitted: progress.emitted.map(|value| value.as_u64()),
            coalesced: progress.coalesced.map(|value| value.as_u64()),
            dropped: progress.dropped.map(|value| value.as_u64()),
            reconnect_generation: progress.reconnect_generation.map(|value| value.as_u64()),
            reconnect_gap: progress.reconnect_gap,
            slow_consumer: progress.slow_consumer.map(|value| value.as_u64()),
        },
    ))
}

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod progress_failure_tests;
#[cfg(test)]
mod queue_live_tests;
#[cfg(test)]
mod tests;
