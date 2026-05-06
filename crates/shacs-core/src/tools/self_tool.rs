use crate::tools::SchemaFragment;
use crate::tools::{JsonMap, StringSchema, Tool, ToolParameters, ToolResult, ValidationError};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex, MutexGuard};

const BLOCKED: &[&str] = &[
    "bus",
    "provider",
    "_running",
    "tools",
    "_runtime_vars",
    "runner",
    "sessions",
    "consolidator",
    "dream",
    "auto_compact",
    "context",
    "commands",
    "_mcp_servers",
    "_mcp_stacks",
    "_pending_queues",
    "_session_locks",
    "_active_tasks",
    "_background_tasks",
    "restrict_to_workspace",
    "channels_config",
    "_concurrency_gate",
    "_unified_session",
    "_extra_hooks",
];

const READ_ONLY: &[&str] = &[
    "subagents",
    "_current_iteration",
    "exec_config",
    "web_config",
];

const DENIED_ATTRS: &[&str] = &[
    "__class__",
    "__dict__",
    "__bases__",
    "__subclasses__",
    "__mro__",
    "__init__",
    "__new__",
    "__reduce__",
    "__getstate__",
    "__setstate__",
    "__del__",
    "__call__",
    "__getattr__",
    "__setattr__",
    "__delattr__",
    "__code__",
    "__globals__",
    "func_globals",
    "func_code",
    "__wrapped__",
    "__closure__",
];

const SENSITIVE_NAMES: &[&str] = &[
    "api_key",
    "secret",
    "password",
    "token",
    "credential",
    "private_key",
    "access_token",
    "refresh_token",
    "auth",
];

const SUMMARY_KEYS: &[&str] = &[
    "max_iterations",
    "context_window_tokens",
    "model",
    "workspace",
    "provider_retry_mode",
    "max_tool_result_chars",
    "_current_iteration",
    "web_config",
    "exec_config",
    "subagents",
    "_last_usage",
];

const MAX_RUNTIME_KEYS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct SelfRuntimeState {
    values: Map<String, Value>,
    scratchpad: Map<String, Value>,
    max_iterations_syncs: usize,
}

impl Default for SelfRuntimeState {
    fn default() -> Self {
        let mut values = Map::new();
        values.insert("max_iterations".to_owned(), json!(40));
        values.insert("context_window_tokens".to_owned(), json!(65_536));
        values.insert(
            "model".to_owned(),
            json!("anthropic/claude-sonnet-4-20250514"),
        );
        values.insert("provider_retry_mode".to_owned(), json!("standard"));
        values.insert("max_tool_result_chars".to_owned(), json!(16_000));
        values.insert("_current_iteration".to_owned(), json!(0));
        values.insert(
            "_last_usage".to_owned(),
            json!({
                "prompt_tokens": 100,
                "completion_tokens": 50,
            }),
        );
        values.insert("workspace".to_owned(), json!("/tmp/workspace"));
        values.insert(
            "web_config".to_owned(),
            json!({ "enable": true, "search": { "provider": "tavily" } }),
        );
        values.insert("exec_config".to_owned(), json!({ "sandbox": false }));
        Self {
            values,
            scratchpad: Map::new(),
            max_iterations_syncs: 0,
        }
    }
}

impl SelfRuntimeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_value(&mut self, key: impl Into<String>, value: Value) {
        self.values.insert(key.into(), value);
    }

    pub fn get_value(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn get_scratchpad(&self, key: &str) -> Option<&Value> {
        self.scratchpad.get(key)
    }

    pub fn set_scratchpad(&mut self, key: impl Into<String>, value: Value) {
        self.scratchpad.insert(key.into(), value);
    }

    pub fn scratchpad_len(&self) -> usize {
        self.scratchpad.len()
    }

    pub fn max_iterations_syncs(&self) -> usize {
        self.max_iterations_syncs
    }
}

#[derive(Clone)]
pub struct SelfTool {
    state: Arc<Mutex<SelfRuntimeState>>,
    modify_allowed: bool,
    channel: Arc<Mutex<String>>,
    chat_id: Arc<Mutex<String>>,
}

impl SelfTool {
    pub fn new(state: Arc<Mutex<SelfRuntimeState>>) -> Self {
        Self::with_modify_allowed(state, true)
    }

    pub fn with_modify_allowed(state: Arc<Mutex<SelfRuntimeState>>, modify_allowed: bool) -> Self {
        Self {
            state,
            modify_allowed,
            channel: Arc::new(Mutex::new(String::new())),
            chat_id: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn set_context(&self, channel: impl Into<String>, chat_id: impl Into<String>) {
        *recover_lock(&self.channel) = channel.into();
        *recover_lock(&self.chat_id) = chat_id.into();
    }
}

impl Tool for SelfTool {
    fn name(&self) -> &str {
        "my"
    }

    fn description(&self) -> &str {
        if self.modify_allowed {
            "Check and set your own runtime state. Actions: check, set. check without key shows config overview; check with key drills into a value using dot-paths; set changes config or stores notes in scratchpad."
        } else {
            "Check and set your own runtime state. Actions: check, set. READ-ONLY MODE: set is disabled."
        }
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .raw_property(
                "action",
                json!({
                    "type": "string",
                    "enum": ["check", "set"],
                    "description": "Action to perform",
                }),
            )
            .property(
                "key",
                StringSchema::new(
                    "Dot-path for check/set. For check without key, shows all config values.",
                ),
            )
            .raw_property(
                "value",
                json!({ "description": "New value (for set). Type must match target." }),
            )
            .required(["action"])
            .to_json_schema()
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        crate::tools::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        )
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key = params.get("key").and_then(Value::as_str);
        match action {
            "inspect" | "check" => self.inspect(key).into(),
            "modify" | "set" => {
                if !self.modify_allowed {
                    "Error: set is disabled (tools.my.allow_set is false)".into()
                } else {
                    self.modify(key, params.get("value").cloned().unwrap_or(Value::Null))
                        .into()
                }
            }
            other => format!("Unknown action: {other}").into(),
        }
    }
}

impl SelfTool {
    fn inspect(&self, key: Option<&str>) -> String {
        let state = recover_lock(&self.state);
        let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
            return inspect_all(&state);
        };
        let top = key.split('.').next().unwrap_or_default();
        if is_denied(top) || is_blocked(top) || is_sensitive(top) {
            return format!("Error: '{top}' is not accessible");
        }
        if key == "scratchpad" {
            return if state.scratchpad.is_empty() {
                "scratchpad is empty".to_owned()
            } else {
                format_value(&Value::Object(state.scratchpad.clone()), "scratchpad")
            };
        }
        match resolve_path(&state.values, key) {
            Ok(value) => format_value(value, key),
            Err(error) => {
                if !key.contains('.') {
                    if let Some(value) = state.scratchpad.get(key) {
                        return format_value(value, key);
                    }
                }
                format!("Error: {error}")
            }
        }
    }

    fn modify(&self, key: Option<&str>, value: Value) -> String {
        let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
            return "Error: 'key' cannot be empty or whitespace".to_owned();
        };
        let top = key.split('.').next().unwrap_or_default();
        if is_blocked(top) || is_denied(top) || is_sensitive(top) {
            return format!("Error: '{key}' is protected and cannot be modified");
        }
        if is_read_only(top) {
            return format!("Error: '{key}' is read-only and cannot be modified");
        }
        if key.contains('.') {
            return self.modify_nested(key, value);
        }
        match key {
            "max_iterations" => self.modify_restricted_int(key, value, 1, 100),
            "context_window_tokens" => self.modify_restricted_int(key, value, 4096, 1_000_000),
            "model" => self.modify_restricted_string(key, value, 1),
            _ => self.modify_free(key, value),
        }
    }

    fn modify_restricted_int(&self, key: &str, value: Value, min: i64, max: i64) -> String {
        if value.is_boolean() {
            return format!("Error: '{key}' must be int, got bool");
        }
        let Some(new_value) = coerce_i64(&value) else {
            return format!("Error: '{key}' must be int, got {}", value_type(&value));
        };
        if new_value < min {
            return format!("Error: '{key}' must be >= {min}");
        }
        if new_value > max {
            return format!("Error: '{key}' must be <= {max}");
        }
        let mut state = recover_lock(&self.state);
        let old = state.values.insert(key.to_owned(), json!(new_value));
        if key == "max_iterations" {
            state.max_iterations_syncs += 1;
        }
        format!("Set {key} = {new_value} (was {})", format_old(old.as_ref()))
    }

    fn modify_restricted_string(&self, key: &str, value: Value, min_len: usize) -> String {
        let Some(new_value) = value.as_str().map(str::to_owned) else {
            return format!("Error: '{key}' must be str, got {}", value_type(&value));
        };
        if new_value.len() < min_len {
            return format!("Error: '{key}' must be at least {min_len} characters");
        }
        let mut state = recover_lock(&self.state);
        let old = state.values.insert(key.to_owned(), json!(new_value));
        format!(
            "Set {key} = {:?} (was {})",
            value.as_str().unwrap_or_default(),
            format_old(old.as_ref())
        )
    }

    fn modify_free(&self, key: &str, value: Value) -> String {
        if let Err(error) = validate_json_safe(&value, 0) {
            return format!("Error: {error}");
        }
        let mut state = recover_lock(&self.state);
        if let Some(old) = state.values.get(key) {
            if scalar_type(old).is_some() && scalar_type(old) != scalar_type(&value) {
                return format!(
                    "Error: '{key}' expects {}, got {}",
                    value_type(old),
                    value_type(&value)
                );
            }
            let old = state.values.insert(key.to_owned(), value.clone());
            return format!(
                "Set {key} = {} (was {})",
                format_value_inline(&value),
                format_old(old.as_ref())
            );
        }
        if !state.scratchpad.contains_key(key) && state.scratchpad.len() >= MAX_RUNTIME_KEYS {
            return format!("Error: scratchpad is full (max {MAX_RUNTIME_KEYS} keys). Remove unused keys first.");
        }
        let old = state.scratchpad.insert(key.to_owned(), value.clone());
        if let Some(old) = old {
            format!(
                "Set scratchpad.{key} = {} (was {})",
                format_value_inline(&value),
                format_value_inline(&old)
            )
        } else {
            format!("Set scratchpad.{key} = {}", format_value_inline(&value))
        }
    }

    fn modify_nested(&self, key: &str, value: Value) -> String {
        let (parent_path, leaf) = key.rsplit_once('.').unwrap_or(("", key));
        if is_denied(leaf) || is_sensitive(leaf) {
            return format!("Error: '{leaf}' is not accessible");
        }
        if let Err(error) = validate_json_safe(&value, 0) {
            return format!("Error: {error}");
        }
        let mut state = recover_lock(&self.state);
        match resolve_path_mut(&mut state.values, parent_path) {
            Ok(Value::Object(parent)) => {
                parent.insert(leaf.to_owned(), value.clone());
                format!("Set {key} = {}", format_value_inline(&value))
            }
            Ok(_) => format!("Error: '{parent_path}' is not an object"),
            Err(error) => format!("Error: {error}"),
        }
    }
}

fn inspect_all(state: &SelfRuntimeState) -> String {
    let mut parts = Vec::new();
    for key in SUMMARY_KEYS {
        if let Some(value) = state.values.get(*key) {
            parts.push(format_value(value, key));
        }
    }
    if !state.scratchpad.is_empty() {
        parts.push(format_value(
            &Value::Object(state.scratchpad.clone()),
            "scratchpad",
        ));
    }
    parts.join("\n")
}

fn resolve_path<'a>(values: &'a Map<String, Value>, path: &str) -> Result<&'a Value, String> {
    let mut parts = path.split('.');
    let Some(first) = parts.next() else {
        return Err("empty path".to_owned());
    };
    let mut current = values
        .get(first)
        .ok_or_else(|| format!("'{first}' not found"))?;
    for part in parts {
        if is_denied(part) || is_blocked(part) || is_sensitive(part) {
            return Err(format!("'{part}' is not accessible"));
        }
        let object = current
            .as_object()
            .ok_or_else(|| format!("'{part}' not found"))?;
        current = object
            .get(part)
            .ok_or_else(|| format!("'{part}' not found in dict"))?;
    }
    Ok(current)
}

fn resolve_path_mut<'a>(
    values: &'a mut Map<String, Value>,
    path: &str,
) -> Result<&'a mut Value, String> {
    let mut parts = path.split('.');
    let Some(first) = parts.next() else {
        return Err("empty path".to_owned());
    };
    let mut current = values
        .get_mut(first)
        .ok_or_else(|| format!("'{first}' not found"))?;
    for part in parts {
        if is_denied(part) || is_blocked(part) || is_sensitive(part) {
            return Err(format!("'{part}' is not accessible"));
        }
        let object = current
            .as_object_mut()
            .ok_or_else(|| format!("'{part}' not found"))?;
        current = object
            .get_mut(part)
            .ok_or_else(|| format!("'{part}' not found in dict"))?;
    }
    Ok(current)
}

fn format_value(value: &Value, key: &str) -> String {
    match value {
        Value::Object(map) if map.is_empty() => format!("{key}: {{}}"),
        Value::Object(map) if map.len() <= 5 => format!("{key}: {}", redact_object(map)),
        Value::Object(map) => {
            let preview = map.keys().take(15).cloned().collect::<Vec<_>>().join(", ");
            let suffix = if map.len() > 15 { ", ..." } else { "" };
            format!("{key}: {{{preview}{suffix}}}")
        }
        Value::Array(values) if values.len() > 20 => format!("{key}: [{} items]", values.len()),
        _ => format!("{key}: {}", format_value_inline(value)),
    }
}

fn redact_object(map: &Map<String, Value>) -> String {
    redact_value(&Value::Object(map.clone())).to_string()
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut safe = Map::new();
            for (key, value) in map {
                if !is_sensitive(key) {
                    safe.insert(key.clone(), redact_value(value));
                }
            }
            Value::Object(safe)
        }
        Value::Array(values) => Value::Array(values.iter().map(redact_value).collect()),
        value => value.clone(),
    }
}

fn format_value_inline(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{value}'"),
        other => other.to_string(),
    }
}

fn format_old(value: Option<&Value>) -> String {
    value
        .map(format_value_inline)
        .unwrap_or_else(|| "None".to_owned())
}

fn validate_json_safe(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 10 {
        return Err("value nesting too deep (max 10 levels)".to_owned());
    }
    match value {
        Value::Array(values) => {
            for (index, item) in values.iter().enumerate() {
                validate_json_safe(item, depth + 1)
                    .map_err(|error| format!("list[{index}] contains {error}"))?;
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                validate_json_safe(item, depth + 1)
                    .map_err(|error| format!("dict key '{key}' contains {error}"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn coerce_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
}

fn scalar_type(value: &Value) -> Option<&'static str> {
    match value {
        Value::String(_) => Some("str"),
        Value::Number(_) => Some("int"),
        Value::Bool(_) => Some("bool"),
        Value::Null => Some("NoneType"),
        _ => None,
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn is_blocked(name: &str) -> bool {
    BLOCKED.contains(&name)
}

fn is_read_only(name: &str) -> bool {
    READ_ONLY.contains(&name)
}

fn is_denied(name: &str) -> bool {
    name.starts_with("__") || DENIED_ATTRS.contains(&name)
}

fn is_sensitive(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    SENSITIVE_NAMES.contains(&lowered.as_str())
        || lowered
            .split('_')
            .any(|part| SENSITIVE_NAMES.contains(&part))
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
