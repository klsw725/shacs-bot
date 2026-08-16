use super::validation::resolve_schema_type;
use serde_json::{Number, Value};

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
