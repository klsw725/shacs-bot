use super::*;
use serde_json::json;

fn values(text: &str) -> SecretValues {
    SecretValues::new(&EnvFile::parse(text.as_bytes()).expect("valid .env.json"))
}

#[test]
fn values_of_eight_characters_or_more_are_masked_by_name() {
    // Given
    let secrets = values(r#"{"SHACS_EIGHT": "12345678", "SHACS_SEVEN": "1234567"}"#);

    // When
    let masked = secrets.mask("a=12345678 b=1234567");

    // Then
    assert_eq!(masked, "a=[secret SHACS_EIGHT] b=1234567");
}

#[test]
fn the_longest_value_wins_and_replaced_text_is_not_looked_at_again() {
    // Given
    let secrets = values(
        r#"{"SHACS_LONG": "token-abcdefgh-rest", "SHACS_SHORT": "token-abcdefgh", "SHACS_NAME": "SHACS_LONG]"}"#,
    );

    // When
    let masked = secrets.mask("x token-abcdefgh-rest y token-abcdefgh z");

    // Then
    assert_eq!(masked, "x [secret SHACS_LONG] y [secret SHACS_SHORT] z");
}

#[test]
fn a_value_under_two_names_keeps_the_first_name() {
    // Given
    let secrets = values(r#"{"SHACS_B": "same-value-1", "SHACS_A": "same-value-1"}"#);

    // When / Then
    assert_eq!(secrets.mask("same-value-1"), "[secret SHACS_A]");
}

#[test]
fn json_values_are_masked_inside_texts_and_keys() {
    // Given
    let secrets = values(r#"{"SHACS_QUOTED": "pa\"ss\\word-9"}"#);
    let value = json!({
        "command": "echo 'pa\"ss\\word-9'",
        "list": ["pa\"ss\\word-9", 3],
        "pa\"ss\\word-9": true
    });

    // When
    let masked = secrets.mask_value(&value);

    // Then
    assert_eq!(
        masked,
        json!({
            "command": "echo '[secret SHACS_QUOTED]'",
            "list": ["[secret SHACS_QUOTED]", 3],
            "[secret SHACS_QUOTED]": true
        })
    );
}

#[test]
fn debug_shows_names_only() {
    // Given
    let secrets = values(r#"{"SHACS_TOKEN": "canary-debug-value"}"#);

    // When
    let shown = format!("{secrets:?}");

    // Then
    assert!(shown.contains("SHACS_TOKEN"), "{shown}");
    assert!(!shown.contains("canary-debug-value"), "{shown}");
}
