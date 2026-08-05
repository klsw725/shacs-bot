use std::path::Path;

use serde_json::Value;
use shacs_projection::{
    spec031_project_owner_record, Spec031ActionRef, Spec031ApprovalCapability,
    Spec031ApprovalState, Spec031Availability, Spec031Capability, Spec031FixtureFamily,
    Spec031Freshness, Spec031OwnerRecordProjectionInput, Spec031ReadinessCapability,
    Spec031ReasonCode, Spec031SafeSummary, Spec031Severity, Spec031SourceOwner, Spec031SubjectRef,
};

use super::OnboardWizardExternalOwnerFact;
use crate::{spec031_cli, CliError, RuntimeInspectOptions};

pub(crate) fn lines(config_path: &Path, workspace: &Path) -> Vec<String> {
    crate::runtime_inspect_inner(
        RuntimeInspectOptions {
            config_path: Some(config_path.to_path_buf()),
            workspace_override: Some(workspace.to_path_buf()),
        },
        false,
    )
    .and_then(|inspect| {
        spec031_cli::readiness::lines(&inspect).map_err(|error| {
            CliError::InvalidArguments(format!("readiness projection failed: {error}"))
        })
    })
    .unwrap_or_else(|error| {
        vec![format!(
            "Spec031 readiness: state=unavailable reason=missing detail={}",
            shacs_redaction::redact_string(&error.to_string())
        )]
    })
}

pub(crate) fn external_owner_facts() -> Vec<OnboardWizardExternalOwnerFact> {
    [spec030_absence(), spec035_absence()]
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|envelope| serde_json::to_value(envelope).ok())
        .filter_map(fact_from_value)
        .collect()
}

fn spec030_absence(
) -> Result<shacs_projection::Spec031Envelope, shacs_projection::Spec031ConstructionError> {
    spec031_project_owner_record(Spec031OwnerRecordProjectionInput {
        family: Spec031FixtureFamily::Approval,
        subject_ref: Spec031SubjectRef::try_new("subject:approval:external-owner")?,
        parent_ref: None,
        action_ref: Some(Spec031ActionRef::try_new("action:approval:external-owner")?),
        digest: None,
        owner: Spec031SourceOwner::Spec030,
        observed_at_unix_ms: None,
        freshness: Spec031Freshness::Unavailable,
        state: Spec031Availability::Unavailable,
        severity: Spec031Severity::Error,
        reason_code: Spec031ReasonCode::MissingExternalOwnerEvidence,
        safe_summary: Spec031SafeSummary::try_new("external owner evidence is missing")?,
        capability: Spec031Capability::Approval(Spec031ApprovalCapability {
            state: Spec031ApprovalState::Pending,
        }),
    })
}

fn spec035_absence(
) -> Result<shacs_projection::Spec031Envelope, shacs_projection::Spec031ConstructionError> {
    spec031_project_owner_record(Spec031OwnerRecordProjectionInput {
        family: Spec031FixtureFamily::Readiness,
        subject_ref: Spec031SubjectRef::try_new("subject:readiness:external-owner")?,
        parent_ref: None,
        action_ref: Some(Spec031ActionRef::try_new(
            "action:readiness:external-owner",
        )?),
        digest: None,
        owner: Spec031SourceOwner::Spec035,
        observed_at_unix_ms: None,
        freshness: Spec031Freshness::Unavailable,
        state: Spec031Availability::Unavailable,
        severity: Spec031Severity::Error,
        reason_code: Spec031ReasonCode::MissingExternalOwnerEvidence,
        safe_summary: Spec031SafeSummary::try_new("external owner evidence is missing")?,
        capability: Spec031Capability::Readiness(Spec031ReadinessCapability {
            availability: Spec031Availability::Unavailable,
            component_count: None,
            queue_depth: None,
            queue_capacity: None,
            remediation: None,
        }),
    })
}

fn fact_from_value(value: Value) -> Option<OnboardWizardExternalOwnerFact> {
    Some(OnboardWizardExternalOwnerFact {
        owner: value.pointer("/source/owner")?.as_str()?.to_owned(),
        capability: value.pointer("/capability/kind")?.as_str()?.to_owned(),
        state: value.get("state")?.as_str()?.to_owned(),
        reason_code: value.pointer("/reason/code")?.as_str()?.to_owned(),
    })
}
