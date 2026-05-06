use serde_json::{json, Value};

pub const EVALUATE_NOTIFICATION_TOOL: &str = "evaluate_notification";

pub trait NotificationEvaluator {
    fn evaluate_response(&self, prompt: &[Value]) -> Result<Option<bool>, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NotifyOnEvaluatorFailure;

impl NotificationEvaluator for NotifyOnEvaluatorFailure {
    fn evaluate_response(&self, _prompt: &[Value]) -> Result<Option<bool>, String> {
        Ok(Some(true))
    }
}

pub fn build_evaluator_messages(task: &str, response: &str) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": "Decide whether this background result should notify the user."}),
        json!({"role": "user", "content": format!("Task:\n{task}\n\nResponse:\n{response}")}),
    ]
}

pub fn evaluate_notification_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": EVALUATE_NOTIFICATION_TOOL,
            "description": "Decide whether the user should be notified about this background task result.",
            "parameters": {
                "type": "object",
                "properties": {
                    "should_notify": {
                        "type": "boolean",
                        "description": "true = result contains actionable/important info the user should see; false = routine or empty, safe to suppress"
                    },
                    "reason": {
                        "type": "string",
                        "description": "One-sentence reason for the decision"
                    }
                },
                "required": ["should_notify"]
            }
        }
    })
}

pub fn parse_notification_decision(response: &Value) -> bool {
    if !should_execute_tools(response) {
        return true;
    }
    response
        .get("tool_calls")
        .and_then(Value::as_array)
        .and_then(|calls| calls.first())
        .and_then(|call| {
            tool_call_arguments(call)
                .and_then(|arguments| arguments.get("should_notify").and_then(Value::as_bool))
        })
        .unwrap_or(true)
}

fn should_execute_tools(response: &Value) -> bool {
    response
        .get("finish_reason")
        .and_then(Value::as_str)
        .is_some_and(|reason| matches!(reason, "tool_calls" | "stop"))
        && response
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty())
}

fn tool_call_arguments(call: &Value) -> Option<Value> {
    if let Some(arguments) = call.get("arguments").and_then(Value::as_object) {
        return Some(Value::Object(arguments.clone()));
    }
    let arguments = call
        .get("function")
        .and_then(|function| function.get("arguments"))
        .or_else(|| call.get("arguments"))?;
    match arguments {
        Value::String(text) => serde_json::from_str(text).ok(),
        Value::Object(map) => Some(Value::Object(map.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_evaluator_is_safe_notify_true_boundary() {
        let evaluator = NotifyOnEvaluatorFailure;
        assert_eq!(evaluator.evaluate_response(&[]), Ok(Some(true)));
        let messages = build_evaluator_messages("cron", "done");
        assert!(messages[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("cron"));
        assert_eq!(
            evaluate_notification_tool_schema()["function"]["name"],
            EVALUATE_NOTIFICATION_TOOL
        );
    }

    #[test]
    fn parser_defaults_to_notify_unless_valid_tool_decision_suppresses() {
        assert!(parse_notification_decision(
            &json!({"finish_reason": "stop"})
        ));
        assert!(!parse_notification_decision(&json!({
            "finish_reason": "tool_calls",
            "tool_calls": [{"function": {"arguments": "{\"should_notify\": false}"}}]
        })));
        assert!(parse_notification_decision(&json!({
            "finish_reason": "length",
            "tool_calls": [{"arguments": {"should_notify": false}}]
        })));
    }
}
