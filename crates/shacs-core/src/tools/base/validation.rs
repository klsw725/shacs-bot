use super::{JsonMap, ValidationError};
use serde_json::Value;

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

pub(super) fn resolve_schema_type(value: Option<&Value>) -> Option<String> {
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
