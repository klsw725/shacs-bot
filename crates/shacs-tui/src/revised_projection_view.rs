use crate::state::{ApprovalStatus, PendingApproval};
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec031ActionRef,
    Spec031ApprovalState, Spec031ConstructionError, Spec031ObservedAtUnixMs,
    Spec035DurableApprovalProjection, Spec035OwnerSurface, Spec035RevisedOwnerFacts,
    Spec035RevisedParseError, Spec035RevisedProjection, Spec035TransportMutationRejection,
};

pub fn spec035_revised_projection_json(
    projection: &Spec035RevisedProjection,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(projection)
}

pub fn spec035_transport_rejection_view(rejection: &Spec035TransportMutationRejection) -> String {
    rejection.to_string()
}

pub fn spec035_revised_json_view(input: &str) -> Result<String, Spec035RevisedParseError> {
    let projection = Spec035RevisedProjection::parse_json(input)?;
    serde_json::to_string(&projection).map_err(|_| Spec035RevisedParseError::InvalidSchema)
}

pub fn spec035_revised_tui_view(
    trusted_runtime: &Spec030RuntimeProjection,
    pending_approval: Option<&PendingApproval>,
) -> Result<String, Spec031ConstructionError> {
    let facts = Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Tui, trusted_runtime);
    let facts = match pending_approval.and_then(durable_approval) {
        Some(approval) => facts.with_durable_approval(approval?),
        None => facts,
    };
    Ok(serde_json::json!(project_spec035_revised_owner_facts(facts)?).to_string())
}

fn durable_approval(
    approval: &PendingApproval,
) -> Option<Result<Spec035DurableApprovalProjection, Spec031ConstructionError>> {
    let state = match approval.status {
        ApprovalStatus::Pending => Spec031ApprovalState::Pending,
        ApprovalStatus::Executing => Spec031ApprovalState::Allowed,
        ApprovalStatus::Unknown => return None,
    };
    Some(
        Spec031ActionRef::try_new(approval.lineage.as_str()).map(|approval_ref| {
            Spec035DurableApprovalProjection {
                state,
                approval_ref,
                expires_at_unix_ms: approval
                    .expires_at_unix_ms
                    .map(Spec031ObservedAtUnixMs::new),
                retry_count: None,
                remembered_allow: None,
            }
        }),
    )
}
