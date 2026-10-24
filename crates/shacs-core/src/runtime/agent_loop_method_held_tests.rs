use super::*;

fn message(role: &str, content: &str, at: &str) -> Value {
    json!({"role": role, "content": content, "timestamp": at})
}

fn tool(call: &str, output: &str) -> Value {
    json!({"role": "tool", "tool_call_id": call, "content": output})
}

#[test]
fn a_request_starts_at_its_goal_message_or_else_where_its_time_began() {
    // Given
    let goal = "Write a report and an image";
    let digest = format!("sha256:{:x}", Sha256::digest(goal));
    let earlier = message("assistant", "an earlier reply", "2026-10-23T09:00:00+00:00");
    let asked = message("user", goal, "2026-10-23T08:00:00+00:00");
    let later = message("assistant", "a reply", "2026-10-23T08:00:01+00:00");
    let with_goal = [earlier.clone(), asked, later.clone()];
    let without_goal = [earlier.clone(), later.clone()];
    let began = Some("2026-10-23T08:00:00+00:00");
    // When / Then
    assert_eq!(request_start(&with_goal, &digest, None), Some(1));
    assert_eq!(
        request_start(&with_goal, &digest, began),
        Some(1),
        "an earlier message stamped later does not move the start"
    );
    assert_eq!(request_start(&without_goal, &digest, began), Some(0));
    assert_eq!(request_start(&without_goal, &digest, None), None);
    assert_eq!(
        request_start(&without_goal, &digest, Some("not a time")),
        None
    );
    let unstamped = [json!({"role": "assistant", "content": "a reply"})];
    assert_eq!(request_start(&unstamped, &digest, began), None);
}

#[test]
fn outputs_sharing_a_call_id_are_each_held_and_given_back() {
    // Given
    let mut session = Session::new("k");
    session.messages = vec![tool("c1", "first output"), tool("c1", "second output")];
    // When
    hold_from(&mut session, "method-request-1", 0);
    hold_from(&mut session, "method-request-1", 0);
    let mut context = session.messages.clone();
    restore_from(
        &held(&session, "method-request-1", "results").unwrap_or_default(),
        &mut context,
    );
    // Then
    assert_eq!(session.messages[0]["content"], quiet_text("first output"));
    assert_eq!(session.messages[1]["content"], quiet_text("second output"));
    assert_eq!(
        context,
        [tool("c1", "first output"), tool("c1", "second output")]
    );
}
