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
            owner: Spec031ExternalOwnerId::Spec032,
            slug: "spec032",
            source_locator: "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md",
            source_status_locator: "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md:3",
            fact_artifacts: &["crates/shacs-app/tests/app_environment.rs#enable_disable_lifecycle_projection_does_not_create_process_truth"],
            command_result_ids: &["spec031-owner-spec032"],
            blocked_reason: "Spec032 app lifecycle adapter fact is absent or failed",
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
            fact_artifacts: &["crates/shacs-core/tests/runtime_agent.rs#runtime_context_routes_stored_video_with_injected_analyzer"],
            command_result_ids: &["spec031-owner-spec034"],
            blocked_reason: "Spec034 media analyzer adapter fact is absent or failed",
        },
        ExternalOwnerFactDescriptor {
            owner: Spec031ExternalOwnerId::Spec035,
            slug: "spec035",
            source_locator: "docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md",
            source_status_locator: "docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:3",
            fact_artifacts: &["crates/shacs-cli/tests/spec031_cli_projection.rs#spec031_readiness_parity_uses_runtime_inspect_owner_source_for_api_cli_and_bundle"],
            command_result_ids: &["spec031-owner-spec035"],
            blocked_reason: "Spec035 projection adapter fact is absent or failed",
        },
    ]
}
