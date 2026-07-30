mod text_format;

pub use text_format::{format_remembered_permission_projection, format_remembered_permission_rule};

use serde::{Deserialize, Serialize};
use shacs_config::{
    RememberedPermissionEffect, RememberedPermissionMatcher,
    RememberedPermissionRemoveByPrefixOutcome, RememberedPermissionRule, RememberedPermissionStore,
    WorkspacePathScope, WorkspacePermissionId,
};
use shacs_redaction::redact_string;
use std::path::{Component, Path};

const DIGEST_PREFIX_LEN: usize = 12;

#[derive(Debug, Clone, Copy)]
pub struct RememberedPermissionStoreHealthInput<'a> {
    status: RememberedPermissionStoreStatus,
    reason: Option<&'a str>,
}

impl<'a> RememberedPermissionStoreHealthInput<'a> {
    pub const fn available() -> Self {
        Self {
            status: RememberedPermissionStoreStatus::Available,
            reason: None,
        }
    }

    pub const fn unavailable(reason: &'a str) -> Self {
        Self {
            status: RememberedPermissionStoreStatus::Unavailable,
            reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RememberedPermissionStoreStatus {
    Available,
    Unavailable,
}

impl RememberedPermissionStoreStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RememberedPermissionProjectionInput<'a> {
    pub store: Option<&'a RememberedPermissionStore>,
    pub workspace_id: &'a WorkspacePermissionId,
    pub health: RememberedPermissionStoreHealthInput<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedPermissionProjection {
    pub schema_version: u32,
    pub status: String,
    pub workspace_digest_prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_health_reason: Option<String>,
    pub rules: Vec<RememberedPermissionRuleProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedPermissionRuleProjection {
    pub rule_id_prefix: String,
    pub effect: RememberedPermissionEffect,
    pub matcher_kind: String,
    pub pattern_summary: String,
    pub created_unix_ms: u64,
    pub last_used_unix_ms: u64,
    pub use_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RememberedPermissionRulePrefixError {
    Empty,
    Missing,
    Ambiguous,
}

impl RememberedPermissionRulePrefixError {
    pub const fn cli_message(self) -> &'static str {
        match self {
            Self::Empty => "remembered permission rule id prefix is required",
            Self::Missing => "remembered permission rule id prefix did not match a rule",
            Self::Ambiguous => "remembered permission rule id prefix is ambiguous",
        }
    }

    pub const fn runtime_message(self) -> &'static str {
        match self {
            Self::Empty => "Remembered permission rule id prefix is required.",
            Self::Missing => "Remembered permission rule id prefix did not match a rule.",
            Self::Ambiguous => "Remembered permission rule id prefix is ambiguous.",
        }
    }
}

pub fn build_remembered_permission_projection(
    input: RememberedPermissionProjectionInput<'_>,
) -> RememberedPermissionProjection {
    let rules = input
        .store
        .and_then(|store| store.project(input.workspace_id))
        .unwrap_or_default()
        .iter()
        .map(project_remembered_permission_rule)
        .collect();

    RememberedPermissionProjection {
        schema_version: input
            .store
            .map_or(1, RememberedPermissionStore::schema_version),
        status: input.health.status.as_str().to_owned(),
        workspace_digest_prefix: workspace_digest_prefix(input.workspace_id),
        store_health_reason: input.health.reason.map(redact_health_reason),
        rules,
    }
}

pub fn project_remembered_permission_rule_by_prefix(
    store: &RememberedPermissionStore,
    workspace_id: &WorkspacePermissionId,
    rule_id_prefix: &str,
) -> Result<RememberedPermissionRuleProjection, RememberedPermissionRulePrefixError> {
    let prefix = normalize_remembered_permission_rule_prefix(rule_id_prefix)?;
    let rules = store.project(workspace_id).unwrap_or_default();
    let mut matches = rules
        .iter()
        .filter(|rule| rule.id().as_str().starts_with(prefix));
    let Some(first) = matches.next() else {
        return Err(RememberedPermissionRulePrefixError::Missing);
    };
    if matches.next().is_some() {
        return Err(RememberedPermissionRulePrefixError::Ambiguous);
    }
    Ok(project_remembered_permission_rule(first))
}

pub fn project_removed_remembered_permission_rule(
    outcome: RememberedPermissionRemoveByPrefixOutcome,
) -> Result<RememberedPermissionRuleProjection, RememberedPermissionRulePrefixError> {
    match outcome {
        RememberedPermissionRemoveByPrefixOutcome::Removed(rule) => {
            Ok(project_remembered_permission_rule(&rule))
        }
        RememberedPermissionRemoveByPrefixOutcome::Missing => {
            Err(RememberedPermissionRulePrefixError::Missing)
        }
        RememberedPermissionRemoveByPrefixOutcome::Ambiguous => {
            Err(RememberedPermissionRulePrefixError::Ambiguous)
        }
    }
}

pub fn project_remembered_permission_rule(
    rule: &RememberedPermissionRule,
) -> RememberedPermissionRuleProjection {
    let (matcher_kind, pattern_summary) = matcher_projection(rule.matcher());
    RememberedPermissionRuleProjection {
        rule_id_prefix: digest_prefix(rule.id().as_str()),
        effect: rule.effect(),
        matcher_kind,
        pattern_summary,
        created_unix_ms: rule.created_unix_ms(),
        last_used_unix_ms: rule.last_used_unix_ms(),
        use_count: rule.use_count(),
    }
}

pub fn normalize_remembered_permission_rule_prefix(
    rule_id_prefix: &str,
) -> Result<&str, RememberedPermissionRulePrefixError> {
    let prefix = rule_id_prefix.trim();
    if prefix.is_empty() {
        return Err(RememberedPermissionRulePrefixError::Empty);
    }
    Ok(prefix)
}

fn matcher_projection(matcher: &RememberedPermissionMatcher) -> (String, String) {
    match matcher {
        RememberedPermissionMatcher::ExactAction { action_digest } => (
            "exact_action".to_owned(),
            format!("exact action {}", digest_prefix(action_digest)),
        ),
        RememberedPermissionMatcher::ExecPrefix { tokens } => {
            let tokens = tokens
                .iter()
                .map(|token| redact_string(token))
                .collect::<Vec<_>>()
                .join(" ");
            ("exec_prefix".to_owned(), format!("exec {tokens} *"))
        }
        RememberedPermissionMatcher::WorkspacePath {
            tool_name,
            path,
            scope,
        } => (
            "workspace_path".to_owned(),
            workspace_path_summary(tool_name, path, scope),
        ),
        RememberedPermissionMatcher::WebOrigin { origin } => (
            "web_origin".to_owned(),
            format!("web_fetch {}", safe_origin(origin)),
        ),
        RememberedPermissionMatcher::McpTool { tool_name } => {
            ("mcp_tool".to_owned(), redact_string(tool_name))
        }
    }
}

fn workspace_path_summary(tool_name: &str, path: &str, scope: &WorkspacePathScope) -> String {
    let safe_path = safe_relative_path(path);
    let tool_name = redact_string(tool_name);
    match scope {
        WorkspacePathScope::Exact => format!("{tool_name} {safe_path}"),
        WorkspacePathScope::Subtree => format!("{tool_name} {safe_path}/**"),
    }
}

fn safe_relative_path(path: &str) -> String {
    let candidate = Path::new(path);
    let is_safe_relative = !candidate.is_absolute()
        && candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if is_safe_relative {
        redact_string(path)
    } else {
        shacs_redaction::REDACTED.to_owned()
    }
}

fn safe_origin(origin: &str) -> String {
    if origin.contains('@') || origin.contains('/') && !origin.contains("://") {
        return shacs_redaction::REDACTED.to_owned();
    }
    redact_string(origin)
}

fn redact_health_reason(_reason: &str) -> String {
    "remembered permission store is unavailable; inspect the local permission store".to_owned()
}

fn workspace_digest_prefix(workspace_id: &WorkspacePermissionId) -> String {
    workspace_id
        .as_str()
        .strip_prefix("workspace:sha256:")
        .map_or_else(|| digest_prefix(workspace_id.as_str()), digest_prefix)
}

fn digest_prefix(digest: &str) -> String {
    digest.chars().take(DIGEST_PREFIX_LEN).collect()
}
