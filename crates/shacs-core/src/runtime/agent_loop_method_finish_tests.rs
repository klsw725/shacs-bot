use super::report_answer;
use crate::runtime::AgentRunResult;

fn result(stop_reason: &str, error: Option<&str>) -> AgentRunResult {
    serde_json::from_value(serde_json::json!({
        "final_content": "Revenue grew [source: notes.md]",
        "messages": [], "tools_used": [], "usage": {}, "stop_reason": stop_reason,
        "error": error, "error_message": error, "interrupt": null,
        "tool_events": [], "had_injections": false
    }))
    .expect("run result")
}

#[test]
fn only_a_completed_provider_answer_is_report_text() {
    // Given
    let completed = result("completed", None);
    let runtime_texts = [
        (
            "tool_error",
            Some("Error: path is outside the configured workspace"),
        ),
        ("cancelled", None),
        ("max_iterations", None),
        ("empty_final_response", Some("empty")),
        ("error", Some("provider failed")),
        ("completed", Some("late error")),
    ];
    // When / Then
    assert_eq!(
        report_answer(&completed),
        Some("Revenue grew [source: notes.md]")
    );
    for (stop_reason, error) in runtime_texts {
        assert!(
            report_answer(&result(stop_reason, error)).is_none(),
            "{stop_reason}"
        );
    }
}
