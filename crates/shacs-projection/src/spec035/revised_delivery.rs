use super::*;
use crate::{
    Spec031Capability, Spec031Count, Spec031Envelope, Spec031ProgressCapability,
    Spec031ProgressDelivery,
};

pub(super) fn delivery_projection(
    surface: Spec035OwnerSurface,
    progress_envelope: Option<&Spec031Envelope>,
    final_envelope: Option<&Spec031Envelope>,
) -> Spec035DeliveryProjection {
    let progress = progress_envelope.and_then(progress_capability);
    let final_progress = final_envelope.and_then(progress_capability);
    let Some(counter_source) = progress.or(final_progress) else {
        return unavailable_delivery(surface);
    };
    let mut states = Vec::new();
    if let Some(progress) = progress {
        match progress.delivery {
            Spec031ProgressDelivery::Live => states.push(Spec035ProgressState::Live),
            Spec031ProgressDelivery::Coalesced => states.push(Spec035ProgressState::Coalesced),
            Spec031ProgressDelivery::Dropped => states.push(Spec035ProgressState::Dropped),
            Spec031ProgressDelivery::Reconnected => states.push(Spec035ProgressState::Reconnected),
            Spec031ProgressDelivery::FinalDelivered
            | Spec031ProgressDelivery::FinalPending
            | Spec031ProgressDelivery::FinalFailed
            | Spec031ProgressDelivery::FinalUnknown => {}
        }
    }
    Spec035DeliveryProjection {
        progress: states,
        accepted_count: observed_count(counter_source.accepted),
        emitted_count: observed_count(counter_source.emitted),
        coalesced_count: observed_count(counter_source.coalesced),
        dropped_count: observed_count(counter_source.dropped),
        final_delivery: Spec035FinalDeliveryProjection {
            owner_surface: surface,
            state: final_progress.map_or(Spec035FinalDeliveryState::Unknown, |progress| {
                final_delivery_state(progress.delivery)
            }),
            guarantee: Spec035DeliveryGuarantee::OwnerSurfaceObservationOnly,
        },
    }
}

fn progress_capability(envelope: &Spec031Envelope) -> Option<&Spec031ProgressCapability> {
    match envelope.capability() {
        Spec031Capability::Progress(progress) => Some(progress),
        Spec031Capability::Session(_)
        | Spec031Capability::Turn(_)
        | Spec031Capability::Subagent(_)
        | Spec031Capability::Approval(_)
        | Spec031Capability::Tool(_)
        | Spec031Capability::Context(_)
        | Spec031Capability::Plugin(_)
        | Spec031Capability::App(_)
        | Spec031Capability::Media(_)
        | Spec031Capability::Diagnostics(_)
        | Spec031Capability::ReleaseEvidence(_)
        | Spec031Capability::Readiness(_) => None,
    }
}

const fn observed_count(value: Option<Spec031Count>) -> Spec035ObservedCount {
    match value {
        Some(value) => Spec035ObservedCount::Observed(value),
        None => Spec035ObservedCount::Unavailable,
    }
}

const fn final_delivery_state(delivery: Spec031ProgressDelivery) -> Spec035FinalDeliveryState {
    match delivery {
        Spec031ProgressDelivery::FinalDelivered => Spec035FinalDeliveryState::FinalDelivered,
        Spec031ProgressDelivery::FinalPending => Spec035FinalDeliveryState::FinalPending,
        Spec031ProgressDelivery::FinalFailed => Spec035FinalDeliveryState::FinalFailed,
        Spec031ProgressDelivery::Live
        | Spec031ProgressDelivery::Coalesced
        | Spec031ProgressDelivery::Dropped
        | Spec031ProgressDelivery::Reconnected
        | Spec031ProgressDelivery::FinalUnknown => Spec035FinalDeliveryState::Unknown,
    }
}

const fn unavailable_delivery(surface: Spec035OwnerSurface) -> Spec035DeliveryProjection {
    Spec035DeliveryProjection {
        progress: Vec::new(),
        accepted_count: Spec035ObservedCount::Unavailable,
        emitted_count: Spec035ObservedCount::Unavailable,
        coalesced_count: Spec035ObservedCount::Unavailable,
        dropped_count: Spec035ObservedCount::Unavailable,
        final_delivery: Spec035FinalDeliveryProjection {
            owner_surface: surface,
            state: Spec035FinalDeliveryState::Unknown,
            guarantee: Spec035DeliveryGuarantee::OwnerSurfaceObservationOnly,
        },
    }
}
