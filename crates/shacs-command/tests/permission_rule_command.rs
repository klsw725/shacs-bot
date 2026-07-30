use shacs_command::{parse_loop_command, LoopCommand, PermissionCommandArgs};

#[test]
fn permission_rule_command_parses_runtime_rule_management() {
    assert_eq!(
        parse_loop_command("/permission rules"),
        Some(LoopCommand::Permission(PermissionCommandArgs::Rules))
    );
    assert_eq!(
        parse_loop_command("/permission inspect abc123"),
        Some(LoopCommand::Permission(PermissionCommandArgs::Inspect(
            "abc123".to_owned()
        )))
    );
    assert_eq!(
        parse_loop_command("/permission revoke abc123"),
        Some(LoopCommand::Permission(PermissionCommandArgs::Revoke(
            "abc123".to_owned()
        )))
    );
}

#[test]
fn permission_rule_command_rejects_missing_and_extra_rule_id_tokens() {
    for command in [
        "/permission inspect",
        "/permission revoke",
        "/permission inspect abc def",
        "/permission revoke abc def",
        "/permission rules abc",
    ] {
        assert_eq!(
            parse_loop_command(command),
            Some(LoopCommand::Permission(PermissionCommandArgs::Invalid))
        );
    }
}
