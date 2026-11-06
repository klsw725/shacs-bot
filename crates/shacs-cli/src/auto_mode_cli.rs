//! Spec 048 PRD 000: the permission environment that the agent loop decides
//! with, and `permissions auto-mode defaults|config`. PRD 020: the notice of
//! a call auto mode did not run.

use crate::{CliError, PermissionsListOptions};
use serde_json::{json, Map, Value};
use shacs_channels::OutboundMessage;
use shacs_config::{
    load_config, AutoModeConfig, ConfigBundle, EnvFile, LoadOptions, PermissionsConfig,
};
use shacs_core::runtime::{
    is_auto_mode_block_notice, AutoModeSlots, PermissionEnvironment, PermissionEnvironmentInput,
    RawRuleLists, RuleEffect, SlotOverrides, ToolEvent, UserPermissionRules,
};
use shacs_redaction::redact_string;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The `.env.json` values each config path last read in this process. A turn
/// that cannot read the file again keeps masking with them (Spec 048 PRD 010
/// scope 10).
static LAST_ENV_FILES: Mutex<BTreeMap<PathBuf, EnvFile>> = Mutex::new(BTreeMap::new());

/// Remembers the values read for `config_path`, such as runtime start's
/// `${NAME}` read.
pub(crate) fn remember_env_file(config_path: &Path, env_file: &EnvFile) {
    LAST_ENV_FILES
        .lock()
        .expect("last .env.json values lock poisoned")
        .insert(config_path.to_path_buf(), env_file.clone());
}

/// This turn's `.env.json` and whether it was read now rather than kept
/// from an earlier read.
fn current_env_file(config_path: &Path) -> (EnvFile, bool) {
    match EnvFile::load(config_path) {
        Ok(env_file) => {
            remember_env_file(config_path, &env_file);
            (env_file, true)
        }
        Err(_) => {
            let kept = LAST_ENV_FILES
                .lock()
                .expect("last .env.json values lock poisoned")
                .get(config_path)
                .cloned();
            (kept.unwrap_or_default(), false)
        }
    }
}

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
    let (env_file, env_file_current) = current_env_file(sources.config_path);
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
        env_file: &env_file,
        env_file_current,
    })
}

/// A startup line on standard error when rules or `autoMode` were left out;
/// `permissions auto-mode config` shows which and why.
/// The notice for a call auto mode did not run, sent to the conversation
/// like the skill notice (Spec 048 PRD 020 scope 8). It holds the tool and
/// the rule only.
pub(crate) fn block_notification(
    event: &ToolEvent,
    channel: &str,
    chat_id: &str,
    routing_metadata: &Map<String, Value>,
    reply_to: Option<&str>,
) -> Option<OutboundMessage> {
    if !is_auto_mode_block_notice(event) {
        return None;
    }
    let result = event.result.as_ref();
    let field = |name: &str| result.and_then(|result| result.get(name)).cloned();
    let mut metadata = routing_metadata.clone();
    metadata.insert(
        "runtime_notification".to_owned(),
        json!({
            "kind": "auto_mode",
            "phase": "block",
            "tool": field("tool"),
            "outcome": field("outcome"),
            "rule": field("rule"),
        }),
    );
    let mut message =
        OutboundMessage::new(channel, chat_id, event.detail.clone()).with_metadata(metadata);
    message.reply_to = reply_to.map(str::to_owned);
    Some(message)
}

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
