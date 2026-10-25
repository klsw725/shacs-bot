use super::coverage::Spec031ExternalOwnerId;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) struct ExternalOwnerFactDescriptor {
    pub(super) owner: Spec031ExternalOwnerId,
    pub(super) slug: &'static str,
    pub(super) source_locator: &'static str,
    pub(super) source_status_locator: &'static str,
    pub(super) fact_artifacts: &'static [&'static str],
    pub(super) command_result_ids: &'static [&'static str],
    pub(super) blocked_reason: &'static str,
}

pub(super) fn external_owner_facts() -> &'static [ExternalOwnerFactDescriptor] {
    &[
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec029,
            slug: "spec029",
            source_locator: "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md",
            source_status_locator: "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md:3",
            fact_artifacts: &[
                "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md#Status: Complete (Scoped)",
                "crates/shacs-core/tests/durable_dispatch.rs#durable_dispatch_restores_due_inbound_and_requeues_stale_lease",
                "crates/shacs-core/tests/durable_child.rs#subagent_runtime_restart_discards_late_success_after_durable_cancellation",
            ],
            command_result_ids: &[
                "spec031-owner-spec029-dispatch",
                "spec031-owner-spec029-child",
            ],
            blocked_reason: "Spec029 durable runtime fact artifacts are missing or failed",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec030,
            slug: "spec030",
            source_locator: "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md",
            source_status_locator: "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md:3",
            fact_artifacts: &["crates/shacs-core/tests/spec030_local_provider.rs#local_spec030_provider_discovers_live_resources_diagnostics_and_trace"],
            command_result_ids: &["spec031-owner-spec030"],
            blocked_reason: "Spec030 trusted-runtime fact adapter test is absent or failed",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec033,
            slug: "spec033",
            source_locator: "docs/specs/033-evaluation-automation-live-integration/SPEC.md",
            source_status_locator: "docs/specs/033-evaluation-automation-live-integration/SPEC.md:3",
            fact_artifacts: &["crates/shacs-core/tests/runtime_loop.rs#automation_channel_event_projects_delivery_only_when_user_visible"],
            command_result_ids: &["spec031-owner-spec033"],
            blocked_reason: "Spec033 automation event adapter fact is absent or failed",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec034,
            slug: "spec034",
            source_locator: "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md",
            source_status_locator: "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md:3",
            fact_artifacts: &["crates/shacs-core/tests/spec034_persistence_operations.rs#final_edit_and_variation_candidates_publish_one_complete_record_each"],
            command_result_ids: &["spec031-owner-spec034"],
            blocked_reason: "Spec034 generated media persistence fact is absent or failed",
        },
    ]
}

const RETIRED_SPEC034_FACT_ARTIFACT: &str =
    "crates/shacs-core/tests/runtime_agent.rs#runtime_context_routes_stored_video_with_injected_analyzer";
pub(super) const RETIRED_SPEC034_ARCHIVE: &str =
    "docs/archive/spec034-video-analyzer/runtime_agent_video_injection_test.rs";
const RETIRED_SPEC034_ARCHIVE_SHA256: &str =
    "df818f687d26dbc22881e136b6b0ea87dedcea3143943ab2f54f97d3fdcae512";

fn retired_spec034_owner_fact() -> &'static ExternalOwnerFactDescriptor {
    static FACT: ExternalOwnerFactDescriptor = ExternalOwnerFactDescriptor {
        owner: Spec031ExternalOwnerId::Spec034,
        slug: "spec034",
        source_locator: "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md",
        source_status_locator:
            "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md:3",
        fact_artifacts: &[RETIRED_SPEC034_FACT_ARTIFACT],
        command_result_ids: &["spec031-owner-spec034"],
        blocked_reason: "Spec034 media analyzer adapter fact is absent or failed",
    };
    &FACT
}

const RETIRED_SPEC032_FACT_ARTIFACT: &str =
    "crates/shacs-app/tests/app_environment.rs#enable_disable_lifecycle_projection_does_not_create_process_truth";
pub(super) const RETIRED_SPEC032_ARCHIVE: &str =
    "docs/archive/spec043-app-environment/app_environment_lifecycle_projection_test.rs";
const RETIRED_SPEC032_ARCHIVE_SHA256: &str =
    "dca1ec9b873d55db2d02aacac1cb750af75aa8eb746d688e05c174fca06530a4";

fn retired_spec032_owner_fact() -> &'static ExternalOwnerFactDescriptor {
    static FACT: ExternalOwnerFactDescriptor = ExternalOwnerFactDescriptor {
        owner: Spec031ExternalOwnerId::Spec032,
        slug: "spec032",
        source_locator: "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md",
        source_status_locator: "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md:3",
        fact_artifacts: &[RETIRED_SPEC032_FACT_ARTIFACT],
        command_result_ids: &["spec031-owner-spec032"],
        blocked_reason: "Spec032 app lifecycle adapter fact is absent or failed",
    };
    &FACT
}

pub(super) fn external_owner_facts_for(schema: &str) -> Vec<&'static ExternalOwnerFactDescriptor> {
    let mut facts: Vec<_> = external_owner_facts()
        .iter()
        .map(|fact| {
            if fact.owner == Spec031ExternalOwnerId::Spec034
                && super::model::uses_retired_spec034_fact(schema)
            {
                retired_spec034_owner_fact()
            } else {
                fact
            }
        })
        .collect();
    if super::model::includes_retired_spec032_owner(schema) {
        facts.insert(2, retired_spec032_owner_fact());
    }
    facts
}

pub(super) fn owner_fact_exists(
    repo_root: &Path,
    evidence_root: &Path,
    schema: &str,
    artifact: &str,
) -> bool {
    if artifact == RETIRED_SPEC034_FACT_ARTIFACT && super::model::uses_retired_spec034_fact(schema)
    {
        return archive_has_test(
            repo_root,
            RETIRED_SPEC034_ARCHIVE,
            RETIRED_SPEC034_ARCHIVE_SHA256,
            RETIRED_SPEC034_FACT_ARTIFACT,
        );
    }
    if artifact == RETIRED_SPEC032_FACT_ARTIFACT
        && super::model::includes_retired_spec032_owner(schema)
    {
        return archive_has_test(
            repo_root,
            RETIRED_SPEC032_ARCHIVE,
            RETIRED_SPEC032_ARCHIVE_SHA256,
            RETIRED_SPEC032_FACT_ARTIFACT,
        );
    }
    let (path, token) = artifact.split_once('#').unwrap_or((artifact, ""));
    let full_path = if path.starts_with("fixtures/") {
        evidence_root.join(path)
    } else {
        repo_root.join(path)
    };
    std::fs::read_to_string(full_path).is_ok_and(|text| token.is_empty() || text.contains(token))
}

fn archive_has_test(repo_root: &Path, archive: &str, sha256: &str, fact: &str) -> bool {
    let Ok(bytes) = std::fs::read(repo_root.join(archive)) else {
        return false;
    };
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let test_name = fact.split_once('#').map_or("", |(_, name)| name);
    digest == sha256 && String::from_utf8(bytes).is_ok_and(|text| text.contains(test_name))
}

pub(super) fn legacy_spec035_owner_fact() -> &'static ExternalOwnerFactDescriptor {
    static FACT: ExternalOwnerFactDescriptor = ExternalOwnerFactDescriptor {
        owner: Spec031ExternalOwnerId::Spec035,
        slug: "spec035",
        source_locator:
            "docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md",
        source_status_locator:
            "docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:3",
        fact_artifacts: &[".omo/evidence/spec035/prd000-009/manifest.json"],
        command_result_ids: &["spec031-owner-spec035"],
        blocked_reason: "retained Spec035 execution evidence is missing, stale, or blocked",
    };
    &FACT
}
