use super::task_method::{ClaimState, ValidationClaim};
use super::task_method_image::{
    CheckState, ImageAttempt, ImageEvidence, ImageState, Requirement, RequirementKind,
};

pub(crate) const UNSUPPORTED_EDIT: &str =
    "image edit, mask and variation are unsupported here, so an earlier image is never edited";

pub(crate) fn image_claims(evidence: &ImageEvidence) -> Vec<ValidationClaim> {
    let Some(verdict) = evidence.verdict.as_ref() else {
        return Vec::new();
    };
    let attempts: Vec<&ImageAttempt> = evidence
        .attempts
        .iter()
        .filter(|attempt| evidence.counts(attempt.selection_revision, verdict.selection_revision))
        .collect();
    let artifact_state = match verdict.state {
        ImageState::Generated => ClaimState::Verified,
        ImageState::Uncertain => ClaimState::NotVerified,
        ImageState::NotRun if attempts.is_empty() => ClaimState::NotVerified,
        ImageState::NotRun
        | ImageState::Unavailable
        | ImageState::Unsupported
        | ImageState::Failed => ClaimState::Failed,
    };
    let mut artifact_refs: Vec<String> = evidence
        .result_artifacts()
        .iter()
        .map(|artifact| {
            format!(
                "image {} sha256:{}",
                artifact.reference,
                short(&artifact.sha256)
            )
        })
        .collect();
    if artifact_refs.is_empty() {
        artifact_refs = attempts
            .iter()
            .map(|attempt| format!("attempt #{}: {}", attempt.seq, attempt.status.label()))
            .collect();
    }
    let requirement_state = if verdict
        .requirements
        .iter()
        .any(|requirement| requirement.state == CheckState::NotMet)
    {
        ClaimState::Failed
    } else {
        ClaimState::NotVerified
    };
    let mut requirement_refs: Vec<String> =
        verdict.requirements.iter().map(requirement_text).collect();
    if verdict.state == ImageState::Generated {
        requirement_refs.push("content and look: needs your review".to_owned());
    }
    let mut revision_refs = Vec::new();
    if let Some(previous) = &evidence.revision_of {
        revision_refs.push(format!("version {} revising {previous}", evidence.version));
        revision_refs.extend(
            evidence
                .previous
                .iter()
                .map(|artifact| format!("earlier image {}", artifact.reference)),
        );
        revision_refs.push(format!("new generation; {UNSUPPORTED_EDIT}"));
    }
    vec![
        claim("generated artifact", artifact_state, artifact_refs),
        claim("stated requirements", requirement_state, requirement_refs),
        claim("user revision", ClaimState::NotVerified, revision_refs),
    ]
}

fn claim(check: &str, state: ClaimState, mut evidence_refs: Vec<String>) -> ValidationClaim {
    evidence_refs.truncate(32);
    ValidationClaim {
        check: check.to_owned(),
        state,
        evidence_refs,
    }
}

pub(crate) fn requirement_text(requirement: &Requirement) -> String {
    let kind = match requirement.kind {
        RequirementKind::Size => "size",
        RequirementKind::Format => "format",
        RequirementKind::Count => "count",
    };
    let state = match requirement.state {
        CheckState::Met => "met",
        CheckState::NotMet => "not met",
        CheckState::Unverified => "not checked",
    };
    match &requirement.observed {
        Some(observed) => format!(
            "{kind} {} — {state} (measured {observed})",
            requirement.expected
        ),
        None => format!("{kind} {} — {state}", requirement.expected),
    }
}

pub(crate) fn short(sha256: &str) -> String {
    sha256.chars().take(12).collect()
}
