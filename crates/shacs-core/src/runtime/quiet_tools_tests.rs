use super::*;
use serde_json::json;

#[test]
fn diagnostics_see_only_a_digest_of_a_calls_arguments() {
    // Given
    let image = json!({"prompt": "a blue chart of the room", "size": "64x48"});
    let write = json!({"path": "art.md", "content": "The artwork is blue."});
    let invented = json!({"The artwork is blue": true});
    // When
    let shown = [&image, &write, &invented].map(|arguments| quiet_arguments(arguments).to_string());
    // Then
    for shown in &shown {
        assert!(!shown.contains("blue"), "{shown}");
        assert!(shown.contains("\"arguments\":\"<withheld: "), "{shown}");
    }
    assert!(!shown[1].contains("art.md"), "{}", shown[1]);
    assert_ne!(
        shown[0],
        quiet_arguments(&json!({"prompt": "a red chart"})).to_string()
    );
}

#[test]
fn a_tool_output_keeps_only_its_length_digest_and_error_mark() {
    // Given
    let mut saved =
        json!({"role": "tool", "tool_call_id": "c1", "content": "The artwork is blue\n"});
    let mut answer = json!({"role": "assistant", "content": "kept"});
    let event = ToolEvent {
        name: "exec".to_owned(),
        status: ToolStatus::Error,
        detail: "Error: The artwork is blue".to_owned(),
        call_id: Some("c1".to_owned()),
        arguments: Some(json!({"command": "printf blue"})),
        result: Some(json!({"stdout": "The artwork is blue"})),
    };
    // When
    quiet_tool_message(&mut saved);
    quiet_tool_message(&mut answer);
    let quiet = quiet_event(&event);
    // Then
    assert_eq!(saved["content"], quiet_text("The artwork is blue\n"));
    assert_eq!(answer["content"], "kept");
    let shown = format!("{saved} {quiet:?}");
    assert!(!shown.contains("blue"), "{shown}");
    assert!(
        quiet.detail.starts_with("Error: <withheld: "),
        "{}",
        quiet.detail
    );
    assert_eq!(quiet.call_id, Some(quiet_id("c1")));
    assert_ne!(quiet_id("c1"), "c1");
    assert_eq!(
        saved["tool_call_id"], "c1",
        "the saved history keeps call ids"
    );
}

#[test]
fn calls_to_tools_not_offered_lose_only_their_name() {
    // Given
    let offered = |name: &str| name == "read_file";
    let mut assistant = json!({"role": "assistant", "content": "", "tool_calls": [
        {"id": "c1", "type": "function", "function": {"name": "read_file", "arguments": "{}"}},
        {"id": "c2", "type": "function", "function": {"name": "The artwork is blue", "arguments": "{}"}},
    ]});
    let mut output = json!({"role": "tool", "tool_call_id": "c2", "name": "The artwork is blue", "content": "x"});
    let mut pending = json!({"id": "c3", "type": "function", "function": {"name": "ask_user"}});
    let mut user = json!({"role": "user", "content": "The artwork is blue"});
    // When
    for value in [&mut assistant, &mut output, &mut pending, &mut user] {
        rename_unoffered(value, &offered);
    }
    // Then
    assert_eq!(assistant["tool_calls"][0]["function"]["name"], "read_file");
    assert_eq!(
        assistant["tool_calls"][1]["function"]["name"],
        UNOFFERED_TOOL
    );
    assert_eq!(assistant["tool_calls"][1]["id"], "c2");
    assert_eq!(output["name"], UNOFFERED_TOOL);
    assert_eq!(output["tool_call_id"], "c2");
    assert_eq!(pending["function"]["name"], UNOFFERED_TOOL);
    assert_eq!(user["content"], "The artwork is blue", "not a call");
}

#[test]
fn an_execution_record_names_earlier_calls_by_digests() {
    // Given
    let turn = "turn:chat:room:3";
    let scope = json!({"session_id": "chat:room", "turn_id": turn});
    let identity = |effect: &str, correlation: &str, causation: &str| {
        json!({"scope": scope, "effect_id": effect, "idempotency_key": effect,
               "correlation_id": correlation, "causation_id": causation})
    };
    let raw = format!("tool:{turn}:The-artwork-is-red");
    let opaque = format!("tool:{turn}:{}", opaque_call_id("c1"));
    let spoofed = format!("tool:{turn}:sha256:The-artwork-is-green");
    let tool = json!({"domain": "tool", "outcome": {"kind": "interrupted"}});
    let mut record = json!({
        "outcomes": [
            {"fact": {"identity": identity(&raw, "The-artwork-is-red", "provider:x"),
                      "outcome": tool, "detail": "Is the artwork blue?"}},
            {"fact": {"identity": identity(&opaque, &opaque_call_id("c1"), "provider:x"),
                      "outcome": tool, "detail": "Permission approval required."}},
            {"fact": {"identity": identity("provider:x:1:0", "provider:x:1", "")}},
            {"fact": {"identity": identity(&spoofed, "sha256:The-artwork-is-green", "provider:x"),
                      "outcome": tool, "detail": "<withheld: Is the artwork blue?>"}},
        ],
        "pending": [
            {"identity": identity("spawn:child-00000042", "child-00000042", &raw)},
            {"identity": identity("spawn:child-00000043", "child-00000043",
                                  &format!("tool:{turn}:sha256:The-artwork-is-blue"))},
        ],
    });
    // When
    quiet_execution_record(&mut record);
    let once = record.clone();
    quiet_execution_record(&mut record);
    // Then
    assert!(!record.to_string().contains("artwork"), "{record}");
    assert_eq!(record, once, "what is already quiet stays as it is");
    let digest = opaque_call_id("The-artwork-is-red");
    let first = &record["outcomes"][0]["fact"]["identity"];
    assert_eq!(first["effect_id"], format!("tool:{turn}:{digest}"));
    assert_eq!(first["idempotency_key"], first["effect_id"]);
    assert_eq!(first["correlation_id"], digest);
    assert_eq!(
        record["outcomes"][0]["fact"]["detail"],
        quiet_text("Is the artwork blue?")
    );
    assert_eq!(
        record["outcomes"][1]["fact"]["identity"]["effect_id"],
        opaque
    );
    assert_eq!(
        record["outcomes"][1]["fact"]["detail"],
        "Permission approval required."
    );
    assert_eq!(
        record["outcomes"][2]["fact"]["identity"]["effect_id"],
        "provider:x:1:0"
    );
    let spawned = &record["pending"][0]["identity"];
    assert_eq!(spawned["effect_id"], "spawn:child-00000042");
    assert_eq!(spawned["causation_id"], format!("tool:{turn}:{digest}"));
}
