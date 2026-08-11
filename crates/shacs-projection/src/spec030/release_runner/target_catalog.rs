#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec030IntegrationTarget {
    pub command_id: &'static str,
    pub package: &'static str,
    pub target: &'static str,
    pub prds: &'static [&'static str],
}

const fn target(
    command_id: &'static str,
    package: &'static str,
    target: &'static str,
    prds: &'static [&'static str],
) -> Spec030IntegrationTarget {
    Spec030IntegrationTarget {
        command_id,
        package,
        target,
        prds,
    }
}

#[rustfmt::skip]
const TARGETS: &[Spec030IntegrationTarget] = &[
    target("target-projection-main", "shacs-projection", "spec030_projection", &["000"]),
    target("target-projection-auth", "shacs-projection", "spec030_auth_status_conversion", &["003"]),
    target("target-projection-integrity", "shacs-projection", "spec030_integrity", &["000", "005", "006"]),
    target("target-projection-semantic", "shacs-projection", "spec030_semantic_evidence", &["006"]),
    target("target-projection-runner-integrity", "shacs-projection", "spec030_runner_integrity", &["006"]),
    target("target-config-policy", "shacs-config", "spec030_runtime_policy", &["000"]),
    target("target-config-auth-resolution", "shacs-config", "spec030_auth_resolution", &["003"]),
    target("target-config-auth-lifecycle", "shacs-config", "spec030_auth_lifecycle", &["003"]),
    target("target-core-tool-before", "shacs-core", "spec030_tool_before", &["001"]),
    target("target-core-tool-confirmation", "shacs-core", "spec030_tool_before_confirmation", &["001"]),
    target("target-core-tool-preparation", "shacs-core", "spec030_tool_before_preparation", &["001"]),
    target("target-core-tool-timeout", "shacs-core", "spec030_tool_before_timeout", &["001"]),
    target("target-core-tool-interaction", "shacs-core", "spec030_tool_before_interaction", &["001"]),
    target("target-core-js-hook-exec", "shacs-core", "spec030_javascript_tool_before_exec", &["001", "005"]),
    target("target-core-process", "shacs-core", "spec030_process_controlled_child", &["002"]),
    target("target-core-process-scope", "shacs-core", "spec030_process_fact_scope", &["002"]),
    target("target-core-startup-facts", "shacs-core", "spec030_startup_facts", &["002"]),
    target("target-core-exec-config", "shacs-core", "spec030_exec_config", &["002", "004"]),
    target("target-core-credential-command", "shacs-core", "spec030_provider_credentials_command", &["003"]),
    target("target-core-credential-oauth", "shacs-core", "spec030_provider_credentials_oauth", &["003"]),
    target("target-core-credential-transport", "shacs-core", "spec030_provider_credentials_transport", &["003"]),
    target("target-core-sandbox", "shacs-core", "spec030_sandbox_adapter", &["004"]),
    target("target-core-sandbox-fallback", "shacs-core", "spec030_sandbox_invalid_plan_fallback", &["004"]),
    target("target-core-resources", "shacs-core", "spec030_resource_selection", &["005"]),
    target("target-core-resource-load", "shacs-core", "spec030_resource_load_checks", &["005"]),
    target("target-core-trace", "shacs-core", "spec030_trace_disclosure", &["005"]),
    target("target-core-runtime", "shacs-core", "spec030_trusted_runtime", &["000", "005"]),
    target("target-core-facts", "shacs-core", "spec030_fact_store", &["000", "005"]),
    target("target-core-local-provider", "shacs-core", "spec030_local_provider", &["000", "005"]),
    target("target-core-classifier", "shacs-core", "spec030_classifier_baseline", &["001"]),
    target("target-core-diagnostics", "shacs-core", "spec030_diagnostics_aggregate", &["005"]),
    target("target-cli-surface", "shacs-cli", "spec030_cli", &["000", "005"]),
    target("target-cli-active", "shacs-cli", "spec030_active_runtime_surfaces", &["000", "003", "005"]),
    target("target-cli-credentials", "shacs-cli", "spec030_provider_credential_construction", &["003"]),
    target("target-cli-credential-invocation", "shacs-cli", "spec030_provider_credential_invocation", &["002", "003"]),
    target("target-cli-handlers", "shacs-cli", "spec030_trusted_handler_registry", &["001"]),
    target("target-cli-js-hook-host", "shacs-cli", "spec030_javascript_tool_before_host", &["001", "005"]),
    target("target-cli-mcp-startup", "shacs-cli", "spec030_mcp_startup_facts", &["002"]),
    target("target-cli-user-plugin", "shacs-cli", "spec030_user_data_plugin", &["005"]),
    target("target-api-surface", "shacs-api", "spec030_api", &["000", "005"]),
    target("target-tui-shared", "shacs-tui", "spec030_shared_surfaces", &["000", "003", "005"]),
    target("target-tui-credentials", "shacs-tui", "spec030_credential_resolution_surfaces", &["003"]),
    target("target-session-diagnostics", "shacs-session", "spec030_session_diagnostics", &["005"]),
    target("target-skills-registry", "shacs-skills", "spec030_skill_registry_baseline", &["005"]),
    target("target-app-registry", "shacs-app", "spec030_app_registry_baseline", &["005"]),
];

pub const fn spec030_integration_targets() -> &'static [Spec030IntegrationTarget] {
    TARGETS
}
