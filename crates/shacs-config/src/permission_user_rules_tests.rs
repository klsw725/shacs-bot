use super::*;
use crate::{PermissionActivationContext, PermissionMode, PermissionModeSource, PermissionsConfig};
use serde_json::json;

fn config(value: Value) -> PermissionsConfig {
    serde_json::from_value(value).expect("permissions config")
}

#[test]
fn rule_lists_are_read_and_a_list_that_is_not_strings_is_dropped_whole() {
    // Given
    let value = json!({
        "mode": "auto",
        "allow": ["exec(git status)", "read"],
        "ask": "exec(git push *)",
        "deny": ["edit(.env)", 3],
    });

    // When
    let config = config(value);

    // Then
    assert_eq!(config.user_rules.allow, vec!["exec(git status)", "read"]);
    assert!(config.user_rules.ask.is_empty());
    assert!(config.user_rules.deny.is_empty());
    let malformed = &config.diagnostics().malformed_fields;
    assert!(malformed.contains(&"permissions.ask".to_owned()));
    assert!(malformed.contains(&"permissions.deny".to_owned()));
    assert_eq!(config.mode(), PermissionMode::Auto);
}

#[test]
fn auto_mode_lists_accept_both_spellings_and_keep_unlisted_defaults() {
    // Given
    let value = json!({
        "mode": "auto",
        "autoMode": {
            "environment": ["[Mine] my server", "$defaults"],
            "hard_deny": ["[Never] wire money"],
            "classifierModel": "small-model",
            "classifyAllShell": true,
        },
    });

    // When
    let config = config(value);

    // Then
    assert_eq!(config.auto_mode_problem(), None);
    assert_eq!(
        config.auto_mode.environment.as_deref(),
        Some(&["[Mine] my server".to_owned(), "$defaults".to_owned()][..])
    );
    assert_eq!(
        config.auto_mode.hard_deny.as_deref(),
        Some(&["[Never] wire money".to_owned()][..])
    );
    assert_eq!(config.auto_mode.soft_deny, None);
    assert_eq!(
        config.auto_mode.classifier_model.as_deref(),
        Some("small-model")
    );
    assert!(config.auto_mode.classify_all_shell);
}

#[test]
fn an_unreadable_auto_mode_starts_in_default_mode() {
    let cases = [
        (json!("on"), AutoModeProblem::NotAnObject),
        (
            json!({"softDeny": ["a"], "soft_deny": ["b"]}),
            AutoModeProblem::WrittenTwice("softDeny".to_owned()),
        ),
        (
            json!({"allow": ["$defaults", "x", "$defaults"]}),
            AutoModeProblem::DefaultsRepeated("allow".to_owned()),
        ),
        (
            json!({"environment": vec!["x"; 65]}),
            AutoModeProblem::TooManyEntries("environment".to_owned()),
        ),
        (
            json!({"allow": ["y".repeat(1_001)]}),
            AutoModeProblem::EntryTooLong("allow".to_owned()),
        ),
        (
            json!({"hardDeny": [1]}),
            AutoModeProblem::NotAListOfStrings("hardDeny".to_owned()),
        ),
        (
            json!({"classifierModel": "m".repeat(201)}),
            AutoModeProblem::ClassifierModelTooLong,
        ),
        (
            json!({"classifyAllShell": "yes"}),
            AutoModeProblem::ClassifyAllShellNotBoolean,
        ),
    ];
    for (auto_mode, problem) in cases {
        // Given
        let value = json!({"mode": "auto", "autoMode": auto_mode});

        // When
        let config = config(value);
        let snapshot = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext::default(),
        );

        // Then
        assert_eq!(config.auto_mode_problem(), Some(&problem));
        assert_eq!(snapshot.mode, PermissionMode::Default);
        assert_eq!(
            snapshot.diagnostics.safe_fallback_reason.as_deref(),
            Some("malformed_auto_mode_config")
        );
    }
}

#[test]
fn saving_the_config_keeps_the_written_rules_and_auto_mode() {
    // Given
    let value = json!({
        "mode": "auto",
        "deny": ["exec(rm *)"],
        "autoMode": {"soft_deny": ["$defaults"]},
    });

    // When
    let saved = serde_json::to_value(config(value)).expect("serialize");

    // Then
    assert_eq!(saved["deny"], json!(["exec(rm *)"]));
    assert_eq!(saved["autoMode"], json!({"soft_deny": ["$defaults"]}));
    assert!(!config(json!({"ask": []})).is_default());
}
