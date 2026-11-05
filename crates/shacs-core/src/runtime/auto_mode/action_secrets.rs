//! The two views of an action's redacted arguments (Spec 048 PRD 010 scopes 9
//! and 10): the exposed view masks `.env.json` values for everything people
//! and providers see; the decision view keeps them and restores secret-file
//! paths that 022 redaction erased, so decisions do not change.

use super::permission_environment::PermissionEnvironment;
use super::permission_materials::{READ_TOOLS, WRITE_TOOLS};
use super::shell_command;
use serde_json::Value;
use shacs_redaction::REDACTED;

pub(crate) struct ArgumentViews {
    pub(crate) exposed: Value,
    /// `None` when it equals the exposed view.
    pub(crate) decision: Option<Value>,
}

pub(crate) fn argument_views(
    tool_name: &str,
    raw: &Value,
    redacted: Value,
    environment: Option<&PermissionEnvironment>,
) -> ArgumentViews {
    let Some(environment) = environment else {
        return ArgumentViews {
            exposed: redacted,
            decision: None,
        };
    };
    let exposed = environment.secret_values.mask_value(&redacted);
    let decision = restored_secret_path(tool_name, raw, &redacted, environment).unwrap_or(redacted);
    ArgumentViews {
        decision: (decision != exposed).then_some(decision),
        exposed,
    }
}

/// The redacted arguments of a file tool whose `path` redaction erased,
/// with `path` set to the cleaned path when it names a secret file. The
/// raw text is never kept.
fn restored_secret_path(
    tool_name: &str,
    raw: &Value,
    redacted: &Value,
    environment: &PermissionEnvironment,
) -> Option<Value> {
    if !READ_TOOLS.contains(&tool_name) && !WRITE_TOOLS.contains(&tool_name) {
        return None;
    }
    if redacted.get("path").and_then(Value::as_str) != Some(REDACTED) {
        return None;
    }
    let text = raw.get("path").and_then(Value::as_str)?;
    let path = shell_command::join_word(&environment.workspace, text, &environment.home);
    if !environment.secret_files.contains(&path) {
        return None;
    }
    let shown = match path.strip_prefix(&environment.workspace) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    };
    let mut restored = redacted.clone();
    restored
        .as_object_mut()?
        .insert("path".to_owned(), Value::String(shown));
    Some(restored)
}

/// Masks `.env.json` values in text shown to people or providers.
pub(crate) fn mask_shown(environment: Option<&PermissionEnvironment>, text: &str) -> String {
    environment.map_or_else(
        || text.to_owned(),
        |environment| environment.secret_values.mask(text),
    )
}
