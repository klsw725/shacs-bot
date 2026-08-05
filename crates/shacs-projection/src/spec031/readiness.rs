use super::readiness_envelope::{aggregate_envelope, component_envelopes};
use super::readiness_input::{normalized_components, Spec031ReadinessAggregationError};
use super::{
    Spec031Availability, Spec031Count, Spec031Envelope, Spec031Freshness, Spec031ObservedAtUnixMs,
    Spec031ReasonCode, Spec031SafeSummary,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ReadinessComponentKind {
    ProviderAuth,
    Storage,
    Containment,
    ChannelWorker,
    PluginApp,
    Queue,
    ExternalIntegration,
}

impl Spec031ReadinessComponentKind {
    pub const REQUIRED: [Self; 6] = [
        Self::ProviderAuth,
        Self::Storage,
        Self::Containment,
        Self::ChannelWorker,
        Self::PluginApp,
        Self::Queue,
    ];

    pub(super) const fn subject_slug(self) -> &'static str {
        match self {
            Self::ProviderAuth => "provider-auth",
            Self::Storage => "storage",
            Self::Containment => "containment",
            Self::ChannelWorker => "channel-worker",
            Self::PluginApp => "plugin-app",
            Self::Queue => "queue",
            Self::ExternalIntegration => "external-integration",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ReadinessRequirement {
    Required,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ReadinessObservation {
    pub kind: Spec031ReadinessComponentKind,
    pub requirement: Spec031ReadinessRequirement,
    pub state: Spec031Availability,
    pub freshness: Spec031Freshness,
    pub reason_code: Spec031ReasonCode,
    pub safe_summary: Spec031SafeSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_capacity: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Spec031ReadinessReport {
    envelope: Spec031Envelope,
    components: Vec<Spec031ReadinessObservation>,
}

impl Spec031ReadinessReport {
    pub const fn envelope(&self) -> &Spec031Envelope {
        &self.envelope
    }

    pub fn components(&self) -> &[Spec031ReadinessObservation] {
        &self.components
    }
}

pub fn spec031_aggregate_readiness(
    observations: &[Spec031ReadinessObservation],
) -> Result<Spec031ReadinessReport, Spec031ReadinessAggregationError> {
    let components = normalized_components(observations)?;
    let state = aggregate_state(&components);
    let children = component_envelopes(&components)?;
    let envelope = aggregate_envelope(
        state,
        aggregate_freshness(&components),
        aggregate_observed_at(&components),
        Spec031Count::new(components.len() as u64),
        children,
    )?;
    Ok(Spec031ReadinessReport {
        envelope,
        components,
    })
}

fn aggregate_state(components: &[Spec031ReadinessObservation]) -> Spec031Availability {
    if required_has(components, Spec031Availability::Blocked) {
        return Spec031Availability::Blocked;
    }
    if required_has(components, Spec031Availability::Unavailable)
        || required_has(components, Spec031Availability::Unknown)
    {
        return Spec031Availability::Unknown;
    }
    if required_has(components, Spec031Availability::Degraded)
        || optional_has(components, Spec031Availability::Degraded)
        || optional_has(components, Spec031Availability::Blocked)
    {
        return Spec031Availability::Degraded;
    }
    Spec031Availability::Ready
}

fn required_has(components: &[Spec031ReadinessObservation], state: Spec031Availability) -> bool {
    components.iter().any(|component| {
        component.requirement == Spec031ReadinessRequirement::Required && component.state == state
    })
}

fn optional_has(components: &[Spec031ReadinessObservation], state: Spec031Availability) -> bool {
    components.iter().any(|component| {
        component.requirement == Spec031ReadinessRequirement::Optional && component.state == state
    })
}

fn aggregate_freshness(components: &[Spec031ReadinessObservation]) -> Spec031Freshness {
    if components
        .iter()
        .any(|component| component.freshness == Spec031Freshness::Stale)
    {
        Spec031Freshness::Stale
    } else if components
        .iter()
        .any(|component| component.freshness == Spec031Freshness::Unavailable)
    {
        Spec031Freshness::Unavailable
    } else if components
        .iter()
        .any(|component| component.freshness == Spec031Freshness::Unknown)
    {
        Spec031Freshness::Unknown
    } else {
        Spec031Freshness::Current
    }
}

fn aggregate_observed_at(
    components: &[Spec031ReadinessObservation],
) -> Option<Spec031ObservedAtUnixMs> {
    components
        .iter()
        .filter_map(|component| component.observed_at_unix_ms)
        .min()
}
