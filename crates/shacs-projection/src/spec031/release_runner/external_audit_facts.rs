use super::coverage::Spec031ExternalOwnerId;

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
            command_result_ids: &["spec031-test-workspace"],
            blocked_reason: "Spec029 durable runtime fact artifacts are missing or failed",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec030,
            slug: "spec030",
            source_locator: "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md",
            source_status_locator: "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md:3",
            fact_artifacts: &[".omo/evidence/spec030/closure-owner-facts.json#approval_policy_redaction_containment"],
            command_result_ids: &["spec031-test-projection-parity"],
            blocked_reason: "Spec030 exact approval/policy/redaction/containment facts are absent",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec032,
            slug: "spec032",
            source_locator: "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md",
            source_status_locator: "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md:3",
            fact_artifacts: &[".omo/evidence/spec032/closure-owner-facts.json#app_lifecycle_readiness_receipt"],
            command_result_ids: &["spec031-test-surface-smoke"],
            blocked_reason: "Spec032 exact app lifecycle owner facts are absent",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec033,
            slug: "spec033",
            source_locator: "docs/specs/033-evaluation-automation-live-integration/SPEC.md",
            source_status_locator: "docs/specs/033-evaluation-automation-live-integration/SPEC.md:3",
            fact_artifacts: &[".omo/evidence/spec033/closure-owner-facts.json#automation_event_coverage"],
            command_result_ids: &["spec031-test-failure-injection"],
            blocked_reason: "Spec033 exact automation owner facts are absent",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec034,
            slug: "spec034",
            source_locator: "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md",
            source_status_locator: "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md:3",
            fact_artifacts: &[".omo/evidence/spec034/closure-owner-facts.json#media_analyzer_projection"],
            command_result_ids: &["spec031-test-surface-smoke"],
            blocked_reason: "Spec034 exact media owner facts are absent",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec035,
            slug: "spec035",
            source_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md",
            source_status_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:3",
            fact_artifacts: &[".omo/evidence/spec035/closure-owner-facts.json#config_profile_secret_ref_snapshot"],
            command_result_ids: &["spec031-test-lifecycle"],
            blocked_reason: "Spec031 exact config/profile/secret-ref facts are absent",
        },
    ]
}
