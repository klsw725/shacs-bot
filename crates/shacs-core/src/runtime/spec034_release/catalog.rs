use super::model::*;
use shacs_projection::{Spec034OwnerFactKind, Spec034ReviewKind, SPEC034_REQUIREMENTS};

pub const FIXTURES: [&str; 2] = [
    ".omo/evidence/spec034/task-12-integration.json",
    "docs/specs/034-generated-media-and-rich-file-context-expansion/documentation-policy.json",
];

pub const BLOCKERS: [&str; 8] = [
    "missing_owner_fact",
    "missing_requirement_evidence",
    "failed_review",
    "failed_cargo_command",
    "dirty_worktree",
    "source_mismatch",
    "artifact_mismatch",
    "cleanup_incomplete",
];

pub fn non_guarantees() -> Vec<String> {
    [
        "bounded_evidence_is_not_complete_video_understanding",
        "media_root_is_not_universal_filesystem_or_process_containment",
        "projection_omission_is_not_complete_redaction",
        "remote_reference_is_not_permanent_or_re_downloadable_artifact",
        "success_fixture_is_runner_only_not_spec034_closure",
        "dirty_current_worktree_records_provenance_not_final_closure",
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn requirements(evidence: &DigestRow) -> Vec<RequirementRow> {
    SPEC034_REQUIREMENTS
        .iter()
        .map(|spec| RequirementRow {
            requirement_id: spec.id.to_owned(),
            primary_prd: spec.primary_prd,
            command_kind: "sequential-integration".to_owned(),
            evidence: evidence.clone(),
        })
        .collect()
}

pub fn blockers(evidence: &DigestRow) -> Vec<BlockerRow> {
    BLOCKERS
        .iter()
        .map(|blocker| BlockerRow {
            blocker: (*blocker).to_owned(),
            disposition: "tested".to_owned(),
            evidence: evidence.clone(),
        })
        .collect()
}

pub fn reviews(evidence: &DigestRow, fixture_only: bool) -> Vec<ReviewRecord> {
    Spec034ReviewKind::required()
        .into_iter()
        .map(|kind| ReviewRecord {
            record_id: format!("runner-mechanics-{kind:?}").to_ascii_lowercase(),
            kind,
            final_review: false,
            fixture_only,
            evidence: evidence.clone(),
        })
        .collect()
}

pub fn owner_audits(evidence: &DigestRow) -> Vec<OwnerAudit> {
    Spec034OwnerFactKind::required()
        .into_iter()
        .map(|kind| OwnerAudit {
            kind,
            status: "command_observed".to_owned(),
            evidence: evidence.clone(),
        })
        .collect()
}
