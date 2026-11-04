//! Spec 048 PRD 000: the permission environment that the agent loop decides
//! with, and `permissions auto-mode defaults|config`.

use crate::{CliError, PermissionsListOptions};
use serde_json::{json, Value};
use shacs_config::{load_config, AutoModeConfig, ConfigBundle, LoadOptions, PermissionsConfig};
use shacs_core::runtime::{
    AutoModeSlots, PermissionEnvironment, PermissionEnvironmentInput, RawRuleLists, RuleEffect,
    SlotOverrides, UserPermissionRules,
};
use shacs_redaction::redact_string;
use std::path::{Path, PathBuf};

pub(crate) struct EnvironmentSources<'a> {
    pub(crate) config_path: &'a Path,
    pub(crate) workspace: &'a Path,
    pub(crate) extra_skill_roots: &'a [PathBuf],
    pub(crate) extra_context_files: &'a [PathBuf],
    pub(crate) protected_targets: &'a [String],
}

/// Built even when the config cannot be read: the secret file and critical
/// path rules hold without user rules.
pub(crate) fn permission_environment(
    bundle: Option<&ConfigBundle>,
    sources: EnvironmentSources<'_>,
) -> PermissionEnvironment {
    let fallback = PermissionsConfig::default();
    let permissions = bundle.map_or(&fallback, |bundle| &bundle.config.permissions);
    let mcp_servers = mcp_servers(bundle);
    let data_folder = sources
        .config_path
        .parent()
        .map_or_else(|| sources.workspace.to_path_buf(), Path::to_path_buf);
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let auto_mode = &permissions.auto_mode;
    PermissionEnvironment::new(PermissionEnvironmentInput {
        workspace: sources.workspace,
        data_folder: &data_folder,
        config_file: sources.config_path,
        home: &home,
        rules: raw_rules(permissions),
        mcp_servers: &mcp_servers,
        extra_skill_roots: sources.extra_skill_roots,
        extra_context_files: sources.extra_context_files,
        configured_targets: sources.protected_targets,
        classify_all_shell: auto_mode.classify_all_shell,
        slots: slot_overrides(auto_mode),
        classifier_model: auto_mode.classifier_model.as_deref(),
    })
}

/// A startup line on standard error when rules or `autoMode` were left out;
/// `permissions auto-mode config` shows which and why.
pub(crate) fn report_rule_problems(bundle: &ConfigBundle) {
    let permissions = &bundle.config.permissions;
    let rules = UserPermissionRules::parse(raw_rules(permissions), &mcp_servers(Some(bundle)));
    let unused = rules
        .diagnostics
        .iter()
        .filter(|diagnostic| !diagnostic.problem.is_warning())
        .count();
    let warnings = rules.diagnostics.len() - unused;
    let malformed = &permissions.diagnostics().malformed_fields;
    if unused + warnings > 0 || !malformed.is_empty() {
        eprintln!(
            "permissions: {unused} rule(s) not used, {warnings} warning(s), ignored fields {malformed:?}; run `shacs-bot permissions auto-mode config` for details"
        );
    }
    if let Some(problem) = permissions.auto_mode_problem() {
        eprintln!("permissions.autoMode was ignored ({problem}); auto mode starts as default");
    }
}

pub(crate) fn auto_mode_defaults(label: Option<&str>) -> Result<String, CliError> {
    let slots = AutoModeSlots::default();
    let slots = match label {
        Some(label) => slots.with_label_prefix(label),
        None => slots,
    };
    pretty(&json!(slots))
}

pub(crate) fn auto_mode_config(options: PermissionsListOptions) -> Result<String, CliError> {
    let bundle = load_config(LoadOptions {
        config_path: options.config_path,
        workspace_override: options.workspace_override,
        resolve_env: false,
        write_back_migrations: false,
    })?;
    let permissions = &bundle.config.permissions;
    let auto_mode = &permissions.auto_mode;
    let slots = AutoModeSlots::resolve(slot_overrides(auto_mode));
    let rules = UserPermissionRules::parse(raw_rules(permissions), &mcp_servers(Some(&bundle)));
    let rule_texts = |effect| {
        rules
            .rule_texts(effect)
            .into_iter()
            .map(redact_string)
            .collect::<Vec<_>>()
    };
    pretty(&json!({
        "environment": slots.environment,
        "hard_deny": slots.hard_deny,
        "soft_deny": slots.soft_deny,
        "allow": slots.allow,
        "classifierModel": auto_mode.classifier_model,
        "classifyAllShell": auto_mode.classify_all_shell,
        "rules": {
            "allow": rule_texts(RuleEffect::Allow),
            "ask": rule_texts(RuleEffect::Ask),
            "deny": rule_texts(RuleEffect::Deny),
        },
        "diagnostics": {
            "rules": rules.diagnostics,
            "autoMode": permissions.auto_mode_problem(),
            "malformedFields": permissions.diagnostics().malformed_fields,
            "warnings": permissions.diagnostics().warnings,
        },
    }))
}

fn raw_rules(permissions: &PermissionsConfig) -> RawRuleLists<'_> {
    RawRuleLists {
        allow: &permissions.user_rules.allow,
        ask: &permissions.user_rules.ask,
        deny: &permissions.user_rules.deny,
    }
}

fn slot_overrides(auto_mode: &AutoModeConfig) -> SlotOverrides<'_> {
    SlotOverrides {
        environment: auto_mode.environment.as_deref(),
        hard_deny: auto_mode.hard_deny.as_deref(),
        soft_deny: auto_mode.soft_deny.as_deref(),
        allow: auto_mode.allow.as_deref(),
    }
}

fn mcp_servers(bundle: Option<&ConfigBundle>) -> Vec<String> {
    bundle
        .map(|bundle| bundle.config.tools.mcp_servers.keys().cloned().collect())
        .unwrap_or_default()
}

fn pretty(value: &Value) -> Result<String, CliError> {
    serde_json::to_string_pretty(value)
        .map_err(|error| CliError::Runtime(format!("could not render JSON: {error}")))
}

#[cfg(test)]
#[path = "auto_mode_cli_tests.rs"]
mod tests;
