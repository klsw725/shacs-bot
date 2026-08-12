use super::current_commands::focused_test;
use super::model::{Spec031ReleaseCommandSpec, Spec031ReleaseGateKind, Spec031ReleaseRunnerConfig};

pub(super) fn exact_owner_commands(
    config: &Spec031ReleaseRunnerConfig,
) -> [Spec031ReleaseCommandSpec; 7] {
    [
        exact_owner_test(
            config,
            "spec031-owner-spec029-dispatch",
            "shacs-core",
            "durable_dispatch",
            "durable_dispatch_restores_due_inbound_and_requeues_stale_lease",
        ),
        exact_owner_test(
            config,
            "spec031-owner-spec029-child",
            "shacs-core",
            "durable_child",
            "subagent_runtime_restart_discards_late_success_after_durable_cancellation",
        ),
        exact_owner_test(
            config,
            "spec031-owner-spec030",
            "shacs-core",
            "spec030_local_provider",
            "local_spec030_provider_discovers_live_resources_diagnostics_and_trace",
        ),
        exact_owner_test(
            config,
            "spec031-owner-spec032",
            "shacs-app",
            "app_environment",
            "enable_disable_lifecycle_projection_does_not_create_process_truth",
        ),
        exact_owner_test(
            config,
            "spec031-owner-spec033",
            "shacs-core",
            "runtime_loop",
            "automation_channel_event_projects_delivery_only_when_user_visible",
        ),
        exact_owner_test(
            config,
            "spec031-owner-spec034",
            "shacs-core",
            "runtime_agent",
            "runtime_context_routes_stored_video_with_injected_analyzer",
        ),
        exact_owner_test(
            config,
            "spec031-owner-spec035",
            "shacs-cli",
            "spec031_cli_projection",
            "spec031_readiness_parity_uses_runtime_inspect_owner_source_for_api_cli_and_bundle",
        ),
    ]
}

fn exact_owner_test(
    config: &Spec031ReleaseRunnerConfig,
    id: &str,
    package: &str,
    target: &str,
    test_name: &str,
) -> Spec031ReleaseCommandSpec {
    focused_test(
        config,
        id,
        package,
        test_name,
        &["--test", target, test_name, "--", "--exact"],
        Spec031ReleaseGateKind::FocusedCargoTest,
    )
}
