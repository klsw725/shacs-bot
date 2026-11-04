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
