use super::{reason_summary, render::envelope_line, severity};
use crate::opaque_ref;
use shacs_projection::{
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031ConstructionError,
    Spec031Count, Spec031Envelope, Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage,
    Spec031ObservedAtUnixMs, Spec031ParentRef, Spec031ProjectionKind, Spec031Reason,
    Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Source, Spec031SourceOwner,
    Spec031SubagentCapability, Spec031SubjectRef,
};
use shacs_session::durable_child::{ReplayChildTask, ReplayChildTaskState};

pub(crate) fn line(child: &ReplayChildTask) -> String {
    match envelope(child) {
        Ok(envelope) => format!(
            "{} outcome={:?} cancellation_requested={} result={}",
            envelope_line("subagent", &envelope),
            child.state,
            child.cancellation_requested_at_ms.is_some(),
            child
                .result_ref
                .as_deref()
                .map(|reference| {
                    Spec031SubjectRef::try_new(reference)
                        .map(|reference| reference.as_str().to_owned())
                        .unwrap_or_else(|_| opaque_ref("result", reference))
                })
                .unwrap_or_else(|| "none".to_owned())
        ),
        Err(error) => {
            format!("Spec031 subagent: state=unavailable reason=unsupported detail={error}")
        }
    }
}

fn envelope(child: &ReplayChildTask) -> Result<Spec031Envelope, Spec031ConstructionError> {
    let (state, reason) = match child.state {
        ReplayChildTaskState::Completed => {
            (Spec031Availability::Ready, Spec031ReasonCode::Completed)
        }
        ReplayChildTaskState::Spawned => {
            (Spec031Availability::Unknown, Spec031ReasonCode::Requested)
        }
        ReplayChildTaskState::Running => {
            (Spec031Availability::Unknown, Spec031ReasonCode::Progress)
        }
        ReplayChildTaskState::Failed => (Spec031Availability::Blocked, Spec031ReasonCode::Blocked),
        ReplayChildTaskState::TimedOut | ReplayChildTaskState::Cancelled => {
            (Spec031Availability::Blocked, Spec031ReasonCode::Interrupted)
        }
    };
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Subagent,
        state,
        severity: severity(state),
        reason: Spec031Reason {
            code: reason,
            safe_summary: Spec031SafeSummary::try_new(reason_summary(reason))?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(&opaque_ref(
                "subject:child",
                &child.child_task_id,
            ))?,
            parent_ref: Some(Spec031ParentRef::try_new(&opaque_ref(
                "parent:turn",
                &child.parent_turn_id,
            ))?),
            action_ref: Some(Spec031ActionRef::try_new(&opaque_ref(
                "action:spawn",
                &child.spawn_effect_id,
            ))?),
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Spec030,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(
                child
                    .finished_at_ms
                    .or(child.started_at_ms)
                    .unwrap_or(child.spawned_at_ms),
            )),
            freshness: Spec031Freshness::Current,
        },
        capability: Spec031Capability::Subagent(Spec031SubagentCapability {
            child_count: Some(Spec031Count::new(1)),
        }),
        children: Vec::new(),
    })
}
