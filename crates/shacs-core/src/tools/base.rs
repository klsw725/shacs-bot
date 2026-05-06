use serde_json::{json, Map, Number, Value};

pub type JsonMap = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn render(&self) -> String {
        if self.path.is_empty() {
            self.message.clone()
        } else {
            format!("{} {}", self.path, self.message)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolResult {
    Text(String),
    Json(Value),
    AskUserInterrupt {
        question: String,
        options: Vec<String>,
    },
}

impl ToolResult {
    pub fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Json(value) => value.to_string(),
            Self::AskUserInterrupt { question, options } => {
                if options.is_empty() {
                    question
                } else {
                    format!("{question}\n{}", options.join("\n"))
                }
            }
        }
    }
}

impl From<String> for ToolResult {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ToolResult {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDefinition {
    pub fn to_openai_schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn parameters(&self) -> Value;

    fn read_only(&self) -> bool {
        false
    }

    fn exclusive(&self) -> bool {
        false
    }

    fn concurrency_safe(&self) -> bool {
        self.read_only() && !self.exclusive()
    }

    fn execute(&self, params: JsonMap) -> ToolResult;

    fn cast_params(&self, params: JsonMap) -> JsonMap {
        let schema = self.parameters();
        if schema.get("type") != Some(&Value::String("object".to_owned())) {
            return params;
        }

        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        params
            .into_iter()
            .map(|(key, value)| {
                let cast = properties
                    .get(&key)
                    .map_or(value.clone(), |fragment| cast_value(value, fragment));
                (key, cast)
            })
            .collect()
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        let schema = self.parameters();
        validate_json_schema_value(&Value::Object(params.clone()), &schema, "")
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_owned(),
            description: self.description().to_owned(),
            parameters: self.parameters(),
        }
    }

    fn to_schema(&self) -> Value {
        self.definition().to_openai_schema()
    }
}

pub fn validate_json_schema_value(
    value: &Value,
    schema: &Value,
    path: &str,
) -> Vec<ValidationError> {
    let Some(schema_object) = schema.as_object() else {
        return Vec::new();
    };
    let nullable = schema_object
        .get("nullable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || schema_object
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|types| types.iter().any(|value| value == "null"));

    if nullable && value.is_null() {
        return Vec::new();
    }

    let schema_type = resolve_schema_type(schema_object.get("type"));
    let label = if path.is_empty() { "parameter" } else { path };
    let mut errors = validate_type(value, schema_type.as_deref(), label);
    if !errors.is_empty() {
        return errors;
    }

    if let Some(enumerants) = schema_object.get("enum").and_then(Value::as_array) {
        if !enumerants.iter().any(|candidate| candidate == value) {
            errors.push(ValidationError::new(
                label,
                format!("must be one of {}", Value::Array(enumerants.clone())),
            ));
        }
    }

    match schema_type.as_deref() {
        Some("integer") | Some("number") => {
            validate_number_bounds(value, schema_object, label, &mut errors)
        }
        Some("string") => validate_string_bounds(value, schema_object, label, &mut errors),
        Some("object") => validate_object(value, schema_object, path, &mut errors),
        Some("array") => validate_array(value, schema_object, path, label, &mut errors),
        _ => {}
    }

    errors
}

fn resolve_schema_type(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .find(|value| *value != "null")
            .map(str::to_owned),
        _ => None,
    }
}

fn subpath(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

fn validate_type(value: &Value, schema_type: Option<&str>, label: &str) -> Vec<ValidationError> {
    let valid = match schema_type {
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        Some("array") => value.is_array(),
        Some("object") => value.is_object(),
        _ => true,
    };

    if valid {
        Vec::new()
    } else {
        vec![ValidationError::new(
            label,
            format!("should be {}", schema_type.unwrap_or("valid")),
        )]
    }
}

fn validate_number_bounds(
    value: &Value,
    schema: &JsonMap,
    label: &str,
    errors: &mut Vec<ValidationError>,
) {
    let Some(actual) = value.as_f64() else {
        return;
    };
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        if actual < minimum {
            errors.push(ValidationError::new(label, format!("must be >= {minimum}")));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
        if actual > maximum {
            errors.push(ValidationError::new(label, format!("must be <= {maximum}")));
        }
    }
}

fn validate_string_bounds(
    value: &Value,
    schema: &JsonMap,
    label: &str,
    errors: &mut Vec<ValidationError>,
) {
    let Some(actual) = value.as_str() else {
        return;
    };
    let length = actual.chars().count() as u64;
    if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
        if length < minimum {
            errors.push(ValidationError::new(
                label,
                format!("must be at least {minimum} chars"),
            ));
        }
    }
    if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64) {
        if length > maximum {
            errors.push(ValidationError::new(
                label,
                format!("must be at most {maximum} chars"),
            ));
        }
    }
}

fn validate_object(value: &Value, schema: &JsonMap, path: &str, errors: &mut Vec<ValidationError>) {
    let Some(actual) = value.as_object() else {
        return;
    };
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !actual.contains_key(key) {
                errors.push(ValidationError::new(
                    "",
                    format!("missing required {}", subpath(path, key)),
                ));
            }
        }
    }

    for (key, child_value) in actual {
        if let Some(child_schema) = properties.get(key) {
            errors.extend(validate_json_schema_value(
                child_value,
                child_schema,
                &subpath(path, key),
            ));
        }
    }
}

fn validate_array(
    value: &Value,
    schema: &JsonMap,
    path: &str,
    label: &str,
    errors: &mut Vec<ValidationError>,
) {
    let Some(actual) = value.as_array() else {
        return;
    };
    let length = actual.len() as u64;
    if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64) {
        if length < minimum {
            errors.push(ValidationError::new(
                label,
                format!("must have at least {minimum} items"),
            ));
        }
    }
    if let Some(maximum) = schema.get("maxItems").and_then(Value::as_u64) {
        if length > maximum {
            errors.push(ValidationError::new(
                label,
                format!("must be at most {maximum} items"),
            ));
        }
    }
    if let Some(item_schema) = schema.get("items") {
        for (index, item) in actual.iter().enumerate() {
            let item_path = if path.is_empty() {
                format!("[{index}]")
            } else {
                format!("{path}[{index}]")
            };
            errors.extend(validate_json_schema_value(item, item_schema, &item_path));
        }
    }
}

pub fn cast_value(value: Value, schema: &Value) -> Value {
    let schema_type = schema
        .as_object()
        .and_then(|object| resolve_schema_type(object.get("type")));
    match schema_type.as_deref() {
        Some("integer") => cast_integer(value),
        Some("number") => cast_number(value),
        Some("string") => cast_string(value),
        Some("boolean") => cast_boolean(value),
        Some("array") => cast_array(value, schema),
        Some("object") => cast_object(value, schema),
        _ => value,
    }
}

fn cast_integer(value: Value) -> Value {
    match value {
        Value::String(text) => text
            .parse::<i64>()
            .map(|number| Value::Number(number.into()))
            .unwrap_or(Value::String(text)),
        other => other,
    }
}

fn cast_number(value: Value) -> Value {
    match value {
        Value::String(text) => text
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::String(text)),
        other => other,
    }
}

fn cast_string(value: Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(text) => Value::String(text),
        other => Value::String(match other {
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            Value::Array(_) | Value::Object(_) => other.to_string(),
            Value::Null | Value::String(_) => String::new(),
        }),
    }
}

fn cast_boolean(value: Value) -> Value {
    match value {
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Value::Bool(true),
            "false" | "0" | "no" => Value::Bool(false),
            _ => Value::String(text),
        },
        other => other,
    }
}

fn cast_array(value: Value, schema: &Value) -> Value {
    let Some(items) = schema.get("items") else {
        return value;
    };
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| cast_value(value, items))
                .collect(),
        ),
        other => other,
    }
}

fn cast_object(value: Value, schema: &Value) -> Value {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return value;
    };
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let cast = properties
                        .get(&key)
                        .map_or(value.clone(), |schema| cast_value(value, schema));
                    (key, cast)
                })
                .collect(),
        ),
        other => other,
    }
}
