use crate::RuntimeInspectReport;
use shacs_projection::{
    Spec031Availability, Spec031Count, Spec031ReadinessComponentKind, Spec031ReadinessObservation,
    Spec031ReasonCode,
};

pub(super) fn queue(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessObservation, String> {
    let depth = inspect.lifecycle.durable_work.pending_count
        + inspect.lifecycle.durable_work.leased_count
        + inspect.lifecycle.durable_work.waiting_retry_count;
    let blocked = !inspect.lifecycle.durable_work.writable
        || !inspect.lifecycle.durable_work.issues.is_empty();
    let mut observation = if blocked {
        super::readiness_observation::observation(
            Spec031ReadinessComponentKind::Queue,
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "durable work queue evidence blocks runtime admission",
        )?
    } else {
        super::readiness_observation::observation(
            Spec031ReadinessComponentKind::Queue,
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "durable work queue evidence allows runtime admission",
        )?
    };
    observation.queue_depth = Some(Spec031Count::new(u64::try_from(depth).unwrap_or(u64::MAX)));
    Ok(observation)
}
