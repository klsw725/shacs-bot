use super::*;
use crate::review_provider::ScriptedClient;
use std::error::Error;
use std::fs;

fn permissions(value: Value) -> PermissionsConfig {
    serde_json::from_value(value).expect("permissions config")
}

fn secrets(value: &str) -> SecretValues {
    SecretValues::new(
        &EnvFile::parse(format!("{{\"SHACS_TEST_TOKEN\": \"{value}\"}}").as_bytes())
            .expect("valid .env.json"),
    )
}

#[test]
fn no_written_rule_sends_nothing() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(&config_path, r#"{"permissions": {"mode": "auto"}}"#)?;

    // When
    let answer = critique(PermissionsListOptions {
        config_path: Some(config_path),
        workspace_override: None,
    })?;

    // Then
    assert_eq!(answer, NOTHING_WRITTEN);
    assert!(request(
        &permissions(json!({"mode": "auto"})),
        &SecretValues::default()
    )
    .is_none());
    Ok(())
}

#[test]
fn the_request_carries_the_written_rules_masked_and_the_answer_comes_back_whole(
) -> Result<(), Box<dyn Error>> {
    // Given
    let written = permissions(json!({
        "deny": ["exec(curl * spec048-mask-value-77)"],
        "autoMode": {
            "environment": ["$defaults", "[Mine] my server with key sk-abcdefghijklmnopqrstuvwx"],
            "classifyAllShell": true,
        },
    }));
    let client = ScriptedClient::new(Ok("[Mine] is too broad.\nNarrow it to one host."));

    // When
    let user = request(&written, &secrets("spec048-mask-value-77")).ok_or("no request")?;
    let answer = critique_with(&client.provider(), user)?;

    // Then
    assert_eq!(answer, "[Mine] is too broad.\nNarrow it to one host.");
    let sent = client.user_texts().join("\n");
    assert!(sent.contains("[secret SHACS_TEST_TOKEN]"), "{sent}");
    assert!(!sent.contains("spec048-mask-value-77"), "{sent}");
    assert!(!sent.contains("sk-abcdefghijklmnopqrstuvwx"), "{sent}");
    let payload: Value = serde_json::from_str(&sent)?;
    assert_eq!(
        payload["written"]["autoMode"]["environment"][0],
        "$defaults"
    );
    assert_eq!(payload["written"]["autoMode"]["classifyAllShell"], true);
    assert_eq!(
        payload["defaults"]["hard_deny"],
        json!(AutoModeSlots::default().hard_deny)
    );
    Ok(())
}

#[test]
fn overfilled_lists_stay_within_the_request_limit() {
    // Given: every list at its limits, each entry far over 1,000 characters
    // as JSON text.
    let long = |label: &str| format!("[{label}] {}", "\"".repeat(990));
    let slot = |label: &str| (0..64).map(|_| long(label)).collect::<Vec<_>>();
    let rules = (0..200)
        .map(|_| format!("exec({})", "y".repeat(2_000)))
        .collect::<Vec<_>>();
    let written = permissions(json!({
        "allow": rules, "ask": rules, "deny": rules,
        "autoMode": {
            "environment": slot("Env"), "hard_deny": slot("Hard"),
            "soft_deny": slot("Soft"), "allow": slot("Allow"),
        },
    }));

    // When
    let user = request(&written, &SecretValues::default()).expect("a request");

    // Then
    assert!(user.chars().count() <= 48_000, "{}", user.chars().count());
    let payload: Value = serde_json::from_str(&user).expect("request JSON");
    let environment = payload["written"]["autoMode"]["environment"]
        .as_array()
        .expect("environment list");
    assert!(environment[0]
        .as_str()
        .is_some_and(|entry| entry.ends_with(" [cut]")));
    assert!(environment
        .last()
        .and_then(Value::as_str)
        .is_some_and(|entry| entry.ends_with("more entries not shown]")));
    for list in [
        &payload["written"]["autoMode"]["environment"],
        &payload["written"]["permissions"]["deny"],
    ] {
        let text = serde_json::to_string(list).expect("list JSON");
        assert!(text.chars().count() <= 6_000, "{}", text.chars().count());
    }
}

#[test]
fn provider_errors_and_timeouts_fail() {
    for failure in ["upstream unavailable", "request timed out"] {
        // Given
        let client = ScriptedClient::new(Err(failure));

        // When
        let answered = critique_with(&client.provider(), "{}".to_owned());

        // Then
        assert!(answered.is_err(), "{failure}");
    }
}
