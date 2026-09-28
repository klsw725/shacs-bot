use super::revised_delivery::delivery_projection;
use super::revised_runtime::runtime_controls;
use super::*;
use crate::{
    HookDenialProjection, HookDenialReason, Spec030Availability, Spec030RuntimeProjection,
    Spec031ActionRef, Spec031Capability, Spec031ConstructionError, Spec031Envelope,
    Spec031Freshness, Spec031ProgressDelivery, Spec031SafeSummary, Spec031SubjectRef,
};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Spec035RevisedOwnerFacts<'a> {
    surface: Spec035OwnerSurface,
    trusted_runtime: &'a Spec030RuntimeProjection,
    durable_approval: Option<Spec035DurableApprovalProjection>,
    ephemeral_confirmation: Option<Spec035EphemeralConfirmationProjection>,
    progress_delivery: Option<&'a Spec031Envelope>,
    final_delivery: Option<&'a Spec031Envelope>,
}

impl<'a> Spec035RevisedOwnerFacts<'a> {
    pub const fn new(
        surface: Spec035OwnerSurface,
        trusted_runtime: &'a Spec030RuntimeProjection,
    ) -> Self {
        Self {
            surface,
            trusted_runtime,
            durable_approval: None,
            ephemeral_confirmation: None,
            progress_delivery: None,
            final_delivery: None,
        }
    }

    pub fn with_durable_approval(mut self, approval: Spec035DurableApprovalProjection) -> Self {
        self.durable_approval = Some(approval);
        self
    }

    pub fn with_ephemeral_confirmation(
        mut self,
        confirmation: Spec035EphemeralConfirmationProjection,
    ) -> Self {
        self.ephemeral_confirmation = Some(confirmation);
        self
    }

    pub const fn with_delivery(mut self, delivery: &'a Spec031Envelope) -> Self {
        match delivery.capability() {
            Spec031Capability::Progress(progress) => match progress.delivery {
                Spec031ProgressDelivery::Live
                | Spec031ProgressDelivery::Coalesced
                | Spec031ProgressDelivery::Dropped
                | Spec031ProgressDelivery::Reconnected => self.progress_delivery = Some(delivery),
                Spec031ProgressDelivery::FinalDelivered
                | Spec031ProgressDelivery::FinalPending
                | Spec031ProgressDelivery::FinalFailed
                | Spec031ProgressDelivery::FinalUnknown => self.final_delivery = Some(delivery),
            },
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
            | Spec031Capability::Readiness(_) => {}
        }
        self
    }

    pub const fn with_progress_delivery(mut self, delivery: &'a Spec031Envelope) -> Self {
        self.progress_delivery = Some(delivery);
        self
    }

    pub const fn with_final_delivery(mut self, delivery: &'a Spec031Envelope) -> Self {
        self.final_delivery = Some(delivery);
        self
    }
}

pub fn project_spec035_revised_owner_facts(
    facts: Spec035RevisedOwnerFacts<'_>,
) -> Result<Spec035RevisedProjection, Spec031ConstructionError> {
    let delivery = facts.final_delivery.or(facts.progress_delivery);
    let freshness = delivery.map_or_else(
        || match facts.trusted_runtime.availability() {
            Spec030Availability::Available | Spec030Availability::Degraded => {
                Spec031Freshness::Current
            }
            Spec030Availability::Unavailable => Spec031Freshness::Unavailable,
            Spec030Availability::Unknown => Spec031Freshness::Unknown,
        },
        |delivery| delivery.source().freshness,
    );
    let observed_at_unix_ms = facts
        .final_delivery
        .or(facts.progress_delivery)
        .and_then(|delivery| delivery.source().observed_at_unix_ms);
    let mut decisions = Vec::new();
    if let Some(approval) = facts.durable_approval {
        decisions.push(Spec035DecisionProjection::DurableApproval(approval));
    }
    if let Some(confirmation) = facts.ephemeral_confirmation {
        decisions.push(Spec035DecisionProjection::EphemeralConfirmation(
            confirmation,
        ));
    }
    decisions.extend(
        facts
            .trusted_runtime
            .hooks()
            .recent_denials
            .iter()
            .map(decision_projection)
            .collect::<Result<Vec<_>, Spec031ConstructionError>>()?,
    );
    let runtime_controls = runtime_controls(facts.trusted_runtime);
    let resources = facts
        .trusted_runtime
        .resources()
        .iter()
        .map(|resource| {
            Ok(Spec035ResourceDisclosureProjection {
                resource_ref: subject_ref("resource", &resource.resource_ref)?,
                trusted_code_disclosure: resource.trusted_code_disclosure,
                disclosure: if facts.trusted_runtime.disclosure().raw_content_possible {
                    Spec035DisclosureState::RawContentPossibleElsewhere
                } else {
                    Spec035DisclosureState::SafeSummary
                },
                boundary: Spec035DisclosureBoundary::ProjectionSerializationOnly,
            })
        })
        .collect::<Result<Vec<_>, Spec031ConstructionError>>()?;
    Ok(Spec035RevisedProjection::new(
        Spec035RevisedProjectionInput {
            surface_summary: Spec031SafeSummary::try_new(surface_summary(facts.surface))?,
            observed_at_unix_ms,
            freshness,
            decisions,
            runtime_controls,
            resources,
            delivery: delivery_projection(
                facts.surface,
                facts.progress_delivery,
                facts.final_delivery,
            ),
        },
    ))
}

fn subject_ref(kind: &str, value: &str) -> Result<Spec031SubjectRef, Spec031ConstructionError> {
    Spec031SubjectRef::try_new(value)
        .or_else(|_| Spec031SubjectRef::try_new(&opaque_owner_ref(kind, value)))
}

fn action_ref(kind: &str, value: &str) -> Result<Spec031ActionRef, Spec031ConstructionError> {
    Spec031ActionRef::try_new(value)
        .or_else(|_| Spec031ActionRef::try_new(&opaque_owner_ref(kind, value)))
}

fn opaque_owner_ref(kind: &str, value: &str) -> String {
    format!("{kind}:sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn decision_projection(
    denial: &HookDenialProjection,
) -> Result<Spec035DecisionProjection, Spec031ConstructionError> {
    let call_ref = action_ref("call", &denial.call_ref)?;
    match denial.reason {
        HookDenialReason::UserDenied => Ok(Spec035DecisionProjection::EphemeralConfirmation(
            Spec035EphemeralConfirmationProjection {
                state: Spec035EphemeralConfirmationState::ConfirmationDenied,
                call_ref,
            },
        )),
        HookDenialReason::HeadlessConfirmationDenied => {
            Ok(Spec035DecisionProjection::EphemeralConfirmation(
                Spec035EphemeralConfirmationProjection {
                    state: Spec035EphemeralConfirmationState::HeadlessConfirmationDenied,
                    call_ref,
                },
            ))
        }
        HookDenialReason::ExtensionBlocked | HookDenialReason::HookFailed => Ok(
            Spec035DecisionProjection::HookDenial(Spec035HookDenialProjection {
                state: Spec035HookDenialState::HookDenied,
                hook_ref: subject_ref("hook", &denial.hook_ref)?,
                call_ref,
            }),
        ),
    }
}

const fn surface_summary(surface: Spec035OwnerSurface) -> &'static str {
    match surface {
        Spec035OwnerSurface::Cli => "CLI owner facts projection",
        Spec035OwnerSurface::Api => "API owner facts projection",
        Spec035OwnerSurface::Websocket => "WebSocket owner facts projection",
        Spec035OwnerSurface::Channel => "channel owner facts projection",
        Spec035OwnerSurface::Tui => "TUI owner facts projection",
    }
}
