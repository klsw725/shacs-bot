use super::receipt_model::{all_observed, sub_observations, SubObservation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DocumentationPolicyReport {
    pub sub_observations: Vec<SubObservation>,
    pub checked_files: [&'static str; 2],
}

impl DocumentationPolicyReport {
    pub fn is_complete(&self) -> bool {
        all_observed(&self.sub_observations)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentationPolicy {
    schema_version: u32,
    acceptance_criterion: String,
    spec_status: String,
    unsupported_claims: BTreeMap<String, bool>,
    scoped_non_guarantees: Vec<String>,
}

pub fn run(repo: &Path) -> Result<DocumentationPolicyReport, Box<dyn Error>> {
    let policy_path = repo.join(
        "docs/specs/034-generated-media-and-rich-file-context-expansion/documentation-policy.json",
    );
    let index_path = repo.join("docs/specs/README.md");
    let policy: DocumentationPolicy = serde_json::from_slice(&std::fs::read(policy_path)?)?;
    let index = std::fs::read_to_string(index_path)?;
    let expected_claims = [
        "cdn",
        "gallery",
        "ui_editor",
        "all_provider_parity",
        "built_in_ffmpeg",
        "full_codec_understanding",
        "arbitrary_url_intake",
    ];
    let expected_non_guarantees = [
        "bounded_evidence_is_not_complete_video_understanding",
        "media_root_is_not_universal_filesystem_or_process_containment",
        "projection_omission_is_not_complete_redaction",
        "remote_reference_is_not_permanent_or_re_downloadable_artifact",
        "success_fixture_is_runner_only_not_spec034_closure",
        "dirty_current_worktree_records_provenance_not_final_closure",
        "portable_command_booleans_and_digests_are_structural_audit_not_external_execution_attestation",
        "detected_execution_closure_tamper_fails_attestation_not_zero_instruction_prevention",
        "darwin_same_uid_double_fork_reparent_escape_is_not_atomically_tracked",
        "same_uid_cleanup_path_replacement_between_identity_check_and_unlink_is_not_atomically_prevented",
        "release_runner_is_not_universal_sandbox_or_process_containment",
    ];
    let policy_identity = policy.schema_version == 1
        && policy.acceptance_criterion == "034-AC010"
        && policy.spec_status == "complete_scoped";
    let unsupported_claims_false = policy.unsupported_claims.len() == expected_claims.len()
        && expected_claims.iter().all(|claim| {
            policy
                .unsupported_claims
                .get(*claim)
                .is_some_and(|supported| !supported)
        });
    let scoped_non_guarantees = policy.scoped_non_guarantees.len() == expected_non_guarantees.len()
        && expected_non_guarantees.iter().all(|value| {
            policy
                .scoped_non_guarantees
                .iter()
                .any(|actual| actual == value)
        });
    let spec_complete_scoped = index.lines().any(|line| {
        let mut cells = line.split('|').map(str::trim);
        cells.next() == Some("")
            && cells.next() == Some("`034-generated-media-and-rich-file-context-expansion`")
            && cells
                .next()
                .is_some_and(|scope| scope.starts_with("`Complete (Scoped)`"))
    });
    Ok(DocumentationPolicyReport {
        sub_observations: sub_observations([
            ("policy_identity", policy_identity),
            ("unsupported_claims_false", unsupported_claims_false),
            ("scoped_non_guarantees", scoped_non_guarantees),
            ("spec_complete_scoped", spec_complete_scoped),
        ]),
        checked_files: [
            "docs/specs/034-generated-media-and-rich-file-context-expansion/documentation-policy.json",
            "docs/specs/README.md",
        ],
    })
}
