use super::*;

#[test]
fn user_lists_replace_defaults_unless_they_splice_them_in() {
    // Given
    let environment = vec!["[Mine] my server".to_owned(), DEFAULTS_MARKER.to_owned()];
    let soft_deny = vec!["[Only] nothing else".to_owned()];

    // When
    let slots = AutoModeSlots::resolve(SlotOverrides {
        environment: Some(&environment),
        soft_deny: Some(&soft_deny),
        ..SlotOverrides::default()
    });

    // Then
    assert_eq!(slots.environment.len(), 1 + DEFAULT_ENVIRONMENT.len());
    assert_eq!(slots.environment[0], "[Mine] my server");
    assert_eq!(slots.soft_deny, soft_deny);
    assert_eq!(slots.hard_deny.len(), DEFAULT_HARD_DENY.len());
    let filled = slots.filled("/ws", "discord:1");
    assert!(filled.environment[1].starts_with("[Workspace] /ws "));
    let labelled = AutoModeSlots::default().with_label_prefix("data");
    assert_eq!(labelled.soft_deny.len(), 1);
    assert!(labelled.allow.is_empty());
}

#[test]
fn rule_labels_come_from_the_rule_lists_only() {
    // Given
    let soft_deny = vec![
        "[Mine] my rule".to_owned(),
        "no label here".to_owned(),
        "[ ] empty label".to_owned(),
    ];
    let slots = AutoModeSlots::resolve(SlotOverrides {
        soft_deny: Some(&soft_deny),
        ..SlotOverrides::default()
    });

    // When
    let labels = slots.rule_labels();

    // Then
    assert!(labels.contains(&"Credential Exfiltration".to_owned()));
    assert!(labels.contains(&"Mine".to_owned()));
    assert!(labels.contains(&"Public Reading".to_owned()));
    assert!(!labels.contains(&"Workspace".to_owned()));
    assert!(!labels.iter().any(String::is_empty));
}
