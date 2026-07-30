use super::{RememberedPermissionEffect, RememberedPermissionMatcher, RememberedPermissionRuleId};
use sha2::{Digest, Sha256};

const STRING_FIELD_TAG: u8 = 1;
const NUMBER_FIELD_TAG: u8 = 2;

pub(super) fn rule_id(
    effect: RememberedPermissionEffect,
    matcher: &RememberedPermissionMatcher,
) -> RememberedPermissionRuleId {
    let mut canonical = Vec::new();
    push_string(&mut canonical, "effect", effect.as_str());
    push_matcher(&mut canonical, matcher);
    RememberedPermissionRuleId(sha256_hex(&canonical))
}

fn push_matcher(output: &mut Vec<u8>, matcher: &RememberedPermissionMatcher) {
    match matcher {
        RememberedPermissionMatcher::ExactAction { action_digest } => {
            push_string(output, "kind", "exact_action");
            push_string(output, "action_digest", action_digest);
        }
        RememberedPermissionMatcher::ExecPrefix { tokens } => {
            push_string(output, "kind", "exec_prefix");
            push_number(output, "tokens", tokens.len());
            for token in tokens {
                push_string(output, "token", token);
            }
        }
        RememberedPermissionMatcher::WorkspacePath {
            tool_name,
            path,
            scope,
        } => {
            push_string(output, "kind", "workspace_path");
            push_string(output, "tool_name", tool_name);
            push_string(output, "path", path);
            push_string(output, "scope", scope.as_str());
        }
        RememberedPermissionMatcher::WebOrigin { origin } => {
            push_string(output, "kind", "web_origin");
            push_string(output, "origin", origin);
        }
        RememberedPermissionMatcher::McpTool { tool_name } => {
            push_string(output, "kind", "mcp_tool");
            push_string(output, "tool_name", tool_name);
        }
    }
}

fn push_string(output: &mut Vec<u8>, field: &str, value: &str) {
    push_field(output, STRING_FIELD_TAG, field, value.as_bytes());
}

fn push_number(output: &mut Vec<u8>, field: &str, value: usize) {
    push_field(
        output,
        NUMBER_FIELD_TAG,
        field,
        value.to_string().as_bytes(),
    );
}

fn push_field(output: &mut Vec<u8>, value_tag: u8, field: &str, value: &[u8]) {
    output.push(value_tag);
    push_len(output, field.len());
    output.extend_from_slice(field.as_bytes());
    push_len(output, value.len());
    output.extend_from_slice(value);
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn contains_forbidden_raw_field(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "rawWorkspacePath" | "rawArguments" | "rawSecret" | "secret"
            ) || contains_forbidden_raw_field(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_forbidden_raw_field),
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => false,
    }
}

fn push_len(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&value.to_be_bytes());
}
