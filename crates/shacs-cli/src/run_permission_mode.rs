//! The permission inputs of one run, all made from one read of the config
//! (Spec 048 PRD 050 scope 2), so the mode and the rules never come from
//! different versions of the file.

use crate::{auto_mode_cli, auto_mode_markers, AgentLoopChatCompletionAdapter};
use shacs_config::{
    ConfigBundle, PermissionActivationContext, PermissionMode, PermissionModeSource,
};
use shacs_core::runtime::{ceiling_for_origin, AgentLoopConfig, RuntimeBoundaryOrigin};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Whether the run can ask the user. A config without a mode starts an
/// interactive run in auto and any other run in `default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunKind {
    Interactive,
    NonInteractive,
}

impl AgentLoopChatCompletionAdapter {
    /// `bundle` is `None` when the config could not be read: the mode is then
    /// `default` and no user rule applies, while the secret file and
    /// critical path rules still hold.
    pub(crate) fn apply_permission_inputs(
        &self,
        config: &mut AgentLoopConfig,
        bundle: Option<&ConfigBundle>,
        kind: RunKind,
    ) {
        let snapshot = bundle.map_or_else(
            || crate::reload_failure_permission_config_snapshot(&self.permission_mode_snapshot),
            |bundle| {
                bundle.config.permissions.normalized_snapshot(
                    PermissionModeSource::UserLocalConfig,
                    PermissionActivationContext {
                        user_local_auto_opt_in: false,
                        containment_precondition_met:
                            crate::permission_containment_precondition_met(
                                self.containment_snapshot.as_ref(),
                            ),
                        interactive_run: kind == RunKind::Interactive,
                    },
                )
            },
        );
        config.extra_context_files = bundle
            .and_then(|bundle| bundle.config.resolve_profiles().ok())
            .and_then(|profiles| profiles.context)
            .map(|profile| profile.value.files.iter().map(PathBuf::from).collect())
            .unwrap_or_default();
        config.permission_mode_snapshot =
            crate::runtime_permission_mode_snapshot_for_workspace(&snapshot, &self.workspace);
        config.permission_auto_approval = snapshot.auto_approval;
        let workspace_scope_ref = config
            .permission_mode_snapshot
            .scope_ref
            .clone()
            .unwrap_or_else(|| crate::opaque_workspace_scope_ref(&self.workspace));
        config.permission_ceiling_snapshot = Some(ceiling_for_origin(
            config.permission_mode_snapshot.mode,
            crate::runtime_capability_ceiling_from_registry(&self.tools),
            vec![workspace_scope_ref],
            RuntimeBoundaryOrigin::UserTurn,
        ));
        let mut protected_targets = config.permission_auto_approval.protected_targets.clone();
        let permissions_target = shacs_config::config_context(
            Some(self.config_path.clone()),
            Some(self.workspace.clone()),
        )
        .remembered_permissions_path()
        .to_string_lossy()
        .to_string();
        if !protected_targets.contains(&permissions_target) {
            protected_targets.push(permissions_target);
        }
        config.permission_rule_input.protected_targets = protected_targets;
        let mut skill_roots = vec![crate::adapter_user_skills_dir(&self.config_path)];
        skill_roots.extend(self.plugin_skill_roots.iter().cloned());
        config.permission_environment = Some(Arc::new(auto_mode_cli::permission_environment(
            bundle,
            auto_mode_cli::EnvironmentSources {
                config_path: &self.config_path,
                workspace: &self.workspace,
                extra_skill_roots: &skill_roots,
                extra_context_files: &config.extra_context_files,
                protected_targets: &config.permission_rule_input.protected_targets,
            },
        )));
        config.auto_mode_setup_hint = config.permission_mode_snapshot.mode == PermissionMode::Auto
            && bundle
                .is_some_and(|bundle| bundle.config.permissions.auto_mode.environment.is_none())
            && !auto_mode_markers::setup_hint_off(self.data_folder());
    }

    pub(crate) fn data_folder(&self) -> &Path {
        self.config_path.parent().unwrap_or(&self.workspace)
    }
}

#[cfg(test)]
#[path = "run_permission_mode_tests.rs"]
mod tests;
