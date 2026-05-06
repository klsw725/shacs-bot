use crate::path::abbreviate_path;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallHint {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

impl ToolCallHint {
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

pub fn format_tool_hints(tool_calls: &[ToolCallHint]) -> String {
    if tool_calls.is_empty() {
        return String::new();
    }
    let mut hints = Vec::<(String, usize)>::new();
    for call in tool_calls {
        let hint = format_single_hint(call);
        if hints.last().is_some_and(|(last, _)| last == &hint) {
            if let Some((_, count)) = hints.last_mut() {
                *count += 1;
            }
        } else {
            hints.push((hint, 1));
        }
    }
    hints
        .into_iter()
        .map(|(hint, count)| {
            if count > 1 {
                format!("{hint} × {count}")
            } else {
                hint
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_single_hint(call: &ToolCallHint) -> String {
    match call.name.as_str() {
        "read_file" => format_known(call, &["path", "file_path"], "read {}", true, false),
        "write_file" => format_known(call, &["path", "file_path"], "write {}", true, false),
        "edit" | "edit_file" => format_known(call, &["file_path", "path"], "edit {}", true, false),
        "glob" => format_known(call, &["pattern"], "glob \"{}\"", false, false),
        "grep" => format_known(call, &["pattern"], "grep \"{}\"", false, false),
        "exec" => format_known(call, &["command"], "$ {}", false, true),
        "web_search" => format_known(call, &["query"], "search \"{}\"", false, false),
        "web_fetch" => format_known(call, &["url"], "fetch {}", true, false),
        "list_dir" => format_known(call, &["path"], "ls {}", true, false),
        name if name.starts_with("mcp_") => format_mcp(call),
        _ => format_fallback(call),
    }
}

fn format_known(
    call: &ToolCallHint,
    key_args: &[&str],
    template: &str,
    is_path: bool,
    is_command: bool,
) -> String {
    let Some(mut value) = extract_arg(call, key_args) else {
        return call.name.clone();
    };
    if is_path {
        value = abbreviate_path(&value, 40);
    } else if is_command {
        value = abbreviate_command(&value, 40);
    }
    template.replace("{}", &value)
}

fn extract_arg(call: &ToolCallHint, key_args: &[&str]) -> Option<String> {
    let args = arguments_object(&call.arguments)?;
    for key in key_args {
        if let Some(value) = args
            .get(*key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_owned());
        }
    }
    args.values()
        .find_map(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn arguments_object(value: &Value) -> Option<&Map<String, Value>> {
    if let Some(object) = value.as_object() {
        Some(object)
    } else {
        value.as_array()?.first()?.as_object()
    }
}

fn abbreviate_command(command: &str, max_len: usize) -> String {
    let regex = Regex::new(
        r#"(?x)
        "(?P<double>(?:[A-Za-z]:[/\\]|~/|/)[^"]+)"
        |'(?P<single>(?:[A-Za-z]:[/\\]|~/|/)[^']+)'
        |(?P<bare>(?:[A-Za-z]:[/\\]|~/|/)[^\s;&|<>"']+)
    "#,
    )
    .ok();
    let abbreviated = regex
        .map(|regex| {
            regex
                .replace_all(command, |captures: &regex::Captures<'_>| {
                    if let Some(value) = captures.name("double") {
                        format!("\"{}\"", abbreviate_path(value.as_str(), 25))
                    } else if let Some(value) = captures.name("single") {
                        format!("'{}'", abbreviate_path(value.as_str(), 25))
                    } else if let Some(value) = captures.name("bare") {
                        abbreviate_path(value.as_str(), 25)
                    } else {
                        captures[0].to_owned()
                    }
                })
                .into_owned()
        })
        .unwrap_or_else(|| command.to_owned());
    if abbreviated.chars().count() <= max_len {
        abbreviated
    } else {
        format!(
            "{}…",
            abbreviated
                .chars()
                .take(max_len.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn format_mcp(call: &ToolCallHint) -> String {
    let name = call.name.as_str();
    let (server, tool) = if let Some((server, tool)) = name.split_once("__") {
        (server.strip_prefix("mcp_").unwrap_or(server), tool)
    } else {
        let rest = name.strip_prefix("mcp_").unwrap_or(name);
        rest.split_once('_').unwrap_or((rest, ""))
    };
    if tool.is_empty() {
        return call.name.clone();
    }
    let value = arguments_object(&call.arguments)
        .and_then(|args| args.values().find_map(Value::as_str))
        .filter(|value| !value.is_empty());
    value
        .map(|value| format!("{server}::{tool}(\"{}\")", abbreviate_path(value, 40)))
        .unwrap_or_else(|| format!("{server}::{tool}"))
}

fn format_fallback(call: &ToolCallHint) -> String {
    let value = arguments_object(&call.arguments)
        .and_then(|args| args.values().next())
        .and_then(Value::as_str);
    match value {
        Some(value) if value.chars().count() > 40 => {
            format!("{}(\"{}\")", call.name, abbreviate_path(value, 40))
        }
        Some(value) => format!("{}(\"{}\")", call.name, value),
        None => call.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_known_mcp_unknown_and_repeated_hints() {
        let calls = vec![
            ToolCallHint::new("read_file", json!({"path": "/very/long/path/to/file.txt"})),
            ToolCallHint::new("read_file", json!({"path": "/very/long/path/to/file.txt"})),
            ToolCallHint::new(
                "exec",
                json!({"command": "cat /very/long/path/to/file.txt"}),
            ),
            ToolCallHint::new("mcp_docs__search", json!({"query": "long query"})),
            ToolCallHint::new("custom", json!({"value": "short"})),
        ];
        let hint = format_tool_hints(&calls);
        assert!(
            hint.contains("read /very/long/path/to/file.txt × 2")
                || hint.contains("read …/path/to/file.txt × 2")
        );
        assert!(hint.contains("$ cat"));
        assert!(hint.contains("docs::search"));
        assert!(hint.contains("custom(\"short\")"));
    }
}
