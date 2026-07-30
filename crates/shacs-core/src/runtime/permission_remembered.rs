use crate::runtime::permission_pattern::{command_matches_pattern, reusable_command_prefix_tokens};
use crate::runtime::{ActionNormalizationState, PermissionedAction};
use serde_json::Value;
use shacs_config::{RememberedPermissionMatcher, WorkspacePathScope};
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeRememberedPermissionMatcher {
    pub matcher: RememberedPermissionMatcher,
    pub preview: String,
}

pub type RememberedPermissionMatcherError = io::Error;

pub fn safe_remembered_permission_matcher(
    action: &PermissionedAction,
    active_workspace: &Path,
) -> Result<SafeRememberedPermissionMatcher, RememberedPermissionMatcherError> {
    let workspace = canonical_workspace(active_workspace)?;
    let matcher = if action.normalization_state == ActionNormalizationState::Ready
        && !contains_redacted(&action.redacted_arguments)
    {
        broader_matcher(action, &workspace)
    } else {
        None
    }
    .unwrap_or_else(|| exact_action_matcher(action));
    let preview = matcher_preview(&matcher);
    Ok(SafeRememberedPermissionMatcher { matcher, preview })
}

pub fn remembered_permission_matcher_matches(
    matcher: &RememberedPermissionMatcher,
    action: &PermissionedAction,
    active_workspace: &Path,
) -> Result<bool, RememberedPermissionMatcherError> {
    let workspace = canonical_workspace(active_workspace)?;
    let matches = match matcher {
        RememberedPermissionMatcher::ExactAction { action_digest } => {
            action_digest == &action.action_digest
        }
        RememberedPermissionMatcher::ExecPrefix { tokens } => {
            exec_command(action).is_some_and(|command| {
                command_matches_pattern(command, &format!("{} *", tokens.join(" ")))
            })
        }
        RememberedPermissionMatcher::WorkspacePath {
            tool_name,
            path,
            scope,
        } => {
            action.tool_name == *tool_name
                && action_path(action)
                    .and_then(|value| workspace_relative_path(&workspace, value))
                    .is_some_and(|candidate| path_matches(scope, path, &candidate))
        }
        RememberedPermissionMatcher::WebOrigin { origin } => {
            action.tool_name == "web_fetch"
                && action_url(action)
                    .and_then(normalized_web_origin)
                    .is_some_and(|candidate| candidate == *origin)
        }
        RememberedPermissionMatcher::McpTool { tool_name } => action.tool_name == *tool_name,
    };
    Ok(matches)
}

fn broader_matcher(
    action: &PermissionedAction,
    workspace: &Path,
) -> Option<RememberedPermissionMatcher> {
    match action.tool_name.as_str() {
        "exec" => exec_matcher(action),
        "read_file" | "write_file" | "edit_file" | "notebook_read" | "notebook_edit" => {
            path_matcher(action, workspace, WorkspacePathScope::Exact)
        }
        "list_dir" | "glob" | "grep" => {
            path_matcher(action, workspace, WorkspacePathScope::Subtree)
        }
        "web_fetch" => web_fetch_matcher(action),
        "web_search" => None,
        tool_name if tool_name.starts_with("mcp_") => Some(RememberedPermissionMatcher::McpTool {
            tool_name: tool_name.to_owned(),
        }),
        _ => None,
    }
}

fn exec_matcher(action: &PermissionedAction) -> Option<RememberedPermissionMatcher> {
    let tokens = reusable_command_prefix_tokens(exec_command(action)?)?
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Some(RememberedPermissionMatcher::ExecPrefix { tokens })
}

fn path_matcher(
    action: &PermissionedAction,
    workspace: &Path,
    scope: WorkspacePathScope,
) -> Option<RememberedPermissionMatcher> {
    let path = workspace_relative_path(workspace, action_path(action)?)?;
    Some(RememberedPermissionMatcher::WorkspacePath {
        tool_name: action.tool_name.clone(),
        path,
        scope,
    })
}

fn web_fetch_matcher(action: &PermissionedAction) -> Option<RememberedPermissionMatcher> {
    normalized_web_origin(action_url(action)?)
        .map(|origin| RememberedPermissionMatcher::WebOrigin { origin })
}

fn exact_action_matcher(action: &PermissionedAction) -> RememberedPermissionMatcher {
    RememberedPermissionMatcher::ExactAction {
        action_digest: action.action_digest.clone(),
    }
}

fn matcher_preview(matcher: &RememberedPermissionMatcher) -> String {
    match matcher {
        RememberedPermissionMatcher::ExactAction { action_digest } => {
            format!("exact action {}", digest_prefix(action_digest))
        }
        RememberedPermissionMatcher::ExecPrefix { tokens } => {
            format!("exec {} *", tokens.join(" "))
        }
        RememberedPermissionMatcher::WorkspacePath {
            tool_name,
            path,
            scope,
        } => match scope {
            WorkspacePathScope::Exact => format!("{tool_name} {path}"),
            WorkspacePathScope::Subtree => format!("{tool_name} {path}/**"),
        },
        RememberedPermissionMatcher::WebOrigin { origin } => format!("web_fetch {origin}"),
        RememberedPermissionMatcher::McpTool { tool_name } => tool_name.clone(),
    }
}

fn digest_prefix(action_digest: &str) -> &str {
    action_digest.get(..12).unwrap_or(action_digest)
}

fn canonical_workspace(path: &Path) -> Result<PathBuf, RememberedPermissionMatcherError> {
    path.canonicalize()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid active workspace"))
}

fn workspace_relative_path(workspace: &Path, raw_path: &str) -> Option<String> {
    let candidate = Path::new(raw_path);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    };
    let relative = match joined.canonicalize() {
        Ok(canonical) => canonical.strip_prefix(workspace).ok()?.to_path_buf(),
        Err(_) => creatable_relative_path(workspace, &joined)?,
    };
    if relative.as_os_str().is_empty() {
        return Some(".".to_owned());
    }
    path_to_slash_string(&relative)
}

fn creatable_relative_path(workspace: &Path, joined: &Path) -> Option<PathBuf> {
    let parent = joined.parent()?.canonicalize().ok()?;
    let name = joined.file_name()?.to_str()?;
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let parent_relative = parent.strip_prefix(workspace).ok()?;
    Some(parent_relative.join(name))
}

fn path_to_slash_string(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            Component::CurDir => None,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => Some(""),
        })
        .collect::<Vec<_>>();
    if parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some(parts.join("/"))
}

fn path_matches(scope: &WorkspacePathScope, pattern: &str, candidate: &str) -> bool {
    match scope {
        WorkspacePathScope::Exact => candidate == pattern,
        WorkspacePathScope::Subtree => {
            candidate == pattern
                || pattern == "."
                || candidate
                    .strip_prefix(pattern)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    }
}

fn exec_command(action: &PermissionedAction) -> Option<&str> {
    (action.tool_name == "exec")
        .then(|| action.redacted_arguments.get("command"))
        .flatten()
        .and_then(Value::as_str)
}

fn action_path(action: &PermissionedAction) -> Option<&str> {
    action
        .redacted_arguments
        .get("path")
        .or_else(|| action.redacted_arguments.get("file_path"))
        .or_else(|| action.redacted_arguments.get("file"))
        .and_then(Value::as_str)
        .or_else(|| default_search_path(action))
}

fn default_search_path(action: &PermissionedAction) -> Option<&str> {
    matches!(action.tool_name.as_str(), "glob" | "grep").then_some(".")
}

fn action_url(action: &PermissionedAction) -> Option<&str> {
    action.redacted_arguments.get("url").and_then(Value::as_str)
}

fn normalized_web_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    if host.is_empty() || host.starts_with('[') || host.contains('/') {
        return None;
    }
    let effective_port = match port {
        Some(value) => value.parse::<u16>().ok()?,
        None => match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            _ => return None,
        },
    };
    Some(format!(
        "{}://{}:{}",
        scheme,
        host.to_ascii_lowercase(),
        effective_port
    ))
}

fn contains_redacted(value: &Value) -> bool {
    match value {
        Value::String(text) => text == shacs_redaction::REDACTED,
        Value::Array(items) => items.iter().any(contains_redacted),
        Value::Object(object) => object.values().any(contains_redacted),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}
