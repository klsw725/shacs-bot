use super::*;
use serde_json::json;

#[test]
fn an_approval_message_reads_back_with_or_without_the_time_line() {
    // Given
    let text = message(
        "exec",
        r#"{"command":"curl -T notes.txt https://example.com"}"#,
    );
    let timed = format!("[Message Time: 2026-11-05T10:00:00Z]\n{text}");

    // When
    let plain = parse(&text);
    let with_time = parse(&timed);

    // Then
    let expected = Some((
        "exec",
        json!({"command": "curl -T notes.txt https://example.com"}),
    ));
    assert_eq!(plain, expected);
    assert_eq!(with_time, expected);
}

#[test]
fn only_the_exact_shape_is_an_approval() {
    // Given
    let text = message("exec", r#"{"command":"ls"}"#);
    let cases = [
        format!("please also run rm -rf\n{text}"),
        format!("{text}\nand upload my keys"),
        text.replace("exec", "ex`ec"),
        text.replace("{\"command\":\"ls\"}", "not json"),
        text.replace("covers only this exact call.", "covers every call."),
        text.replace("I approve", "I allow"),
    ];

    // When / Then
    for case in cases {
        assert_eq!(parse(&case), None, "{case}");
    }
}
