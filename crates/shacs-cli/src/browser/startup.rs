use crate::auto_mode_cli::{self, EnvironmentSources};
use shacs_config::{BrowserConfig, ConfigBundle, UserLocalBrowserEnable};
use shacs_core::runtime::{
    self, BrowserError, BrowserStartEvidence, ContainmentSnapshotRef, NetworkGuard,
    PermissionMaterials, ProcessAdapterKind, ProcessContainmentProofCandidate, ProcessGateInput,
    RawRuleLists, UserPermissionRules,
};

pub trait BrowserStartup: Send + Sync {
    fn config(&self) -> BrowserConfig;
    fn gate(&self, generation: u64) -> Result<ProcessGateInput, BrowserError>;
    fn network_guard(&self) -> NetworkGuard;
}

pub struct ConfigBrowserStartup {
    bundle: ConfigBundle,
}

impl ConfigBrowserStartup {
    pub fn new(bundle: ConfigBundle) -> Self {
        Self { bundle }
    }
}

impl BrowserStartup for ConfigBrowserStartup {
    fn config(&self) -> BrowserConfig {
        self.bundle.config.tools.browser.clone()
    }

    fn gate(&self, generation: u64) -> Result<ProcessGateInput, BrowserError> {
        if !self.bundle.config.tools.browser.enable {
            return Err(BrowserError::Disabled);
        }
        if !crate::official_runtime_package_marker_observed() {
            return Err(BrowserError::Unavailable);
        }
        let containment = crate::runtime_containment_inspect(&self.bundle);
        if containment.contained != Some(true) {
            return Err(BrowserError::Unavailable);
        }
        browser_start_input(
            &self.bundle,
            generation,
            Some(crate::runtime_containment_snapshot_ref(&containment)),
        )
    }

    fn network_guard(&self) -> NetworkGuard {
        NetworkGuard::with_ssrf_whitelist(&self.bundle.config.tools.ssrf_whitelist)
    }
}

/// Runtime entry point: the opt-in is reread from the selected file, never
/// taken from the call context or a tool registry's merged configuration.
pub fn browser_start_input(
    bundle: &ConfigBundle,
    generation: u64,
    containment: Option<ContainmentSnapshotRef>,
) -> Result<ProcessGateInput, BrowserError> {
    let enable =
        UserLocalBrowserEnable::read(&bundle.context.config_path, &bundle.context.workspace)
            .map_err(|_| BrowserError::StartDenied)?
            .ok_or(BrowserError::StartDenied)?;
    let mut input = crate::mcp_startup_gate(
        bundle,
        &format!("browser-generation-{generation}"),
        "chromium",
        containment,
    )
    .map_err(|_| BrowserError::StartDenied)?
    .input;
    input.envelope.adapter = ProcessAdapterKind::Browser;
    input.envelope.action.tool_name = "browser_startup".to_owned();
    input.envelope.redacted_command.redacted_summary = "browser generation startup".to_owned();
    let mut environment = auto_mode_cli::permission_environment(
        Some(bundle),
        EnvironmentSources {
            config_path: &bundle.context.config_path,
            workspace: &bundle.context.workspace,
            extra_skill_roots: &[],
            extra_context_files: &[],
            protected_targets: &[],
        },
    );
    // These rules and enable come from the same captured user-local bytes,
    // even if the supplied bundle is older or contains other config layers.
    let rules = enable.permission_rules();
    environment.user_rules = UserPermissionRules::parse(
        RawRuleLists {
            allow: &rules.allow,
            ask: &rules.ask,
            deny: &rules.deny,
        },
        &[],
    );
    input.policy_materials.materials = Some(PermissionMaterials::for_startup(
        &input.envelope.action,
        &environment,
    ));
    input.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(
        runtime::containment_permission_proof_for_process_gate(
            &input.envelope,
            &input.permission_rules,
            input.inherited_context.as_ref(),
            input.now_unix_ms,
        )
        .map_err(|_| BrowserError::StartDenied)?,
    ));
    input.policy_materials.browser_start =
        Some(BrowserStartEvidence::bind(enable, &input.envelope));
    Ok(input)
}
