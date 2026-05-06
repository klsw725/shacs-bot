use shacs_command::{
    build_help_text, is_builtin_command, normalize_channel_command, parse_loop_command, CommandId,
    CommandKind, CommandRouter, HistoryCommandArgs, LoopCommand,
};
use std::error::Error;

#[test]
fn builtin_router_distinguishes_priority_exact_and_prefix_commands() -> Result<(), Box<dyn Error>> {
    let router = CommandRouter::builtin();

    let priority = router
        .dispatch_priority("  /STATUS  ")
        .ok_or("missing status priority")?;
    assert_eq!(priority.id, CommandId::Status);
    assert_eq!(priority.kind, CommandKind::Priority);

    let exact = router.dispatch("/new").ok_or("missing new exact")?;
    assert_eq!(exact.id, CommandId::New);
    assert_eq!(exact.kind, CommandKind::Exact);

    let prefix = router
        .dispatch("/dream-log abc123")
        .ok_or("missing dream log prefix")?;
    assert_eq!(prefix.id, CommandId::DreamLog);
    assert_eq!(prefix.kind, CommandKind::Prefix);
    assert_eq!(prefix.args, "abc123");

    assert!(!router.is_dispatchable_command("/stop"));
    assert!(router.is_priority("/stop"));
    assert!(router.is_dispatchable_command("/history 5"));
    Ok(())
}

#[test]
fn prefix_dispatch_preserves_original_raw_and_args_case() -> Result<(), Box<dyn Error>> {
    let router = CommandRouter::builtin();
    let parsed = router
        .dispatch("  /dream-log AbC123  ")
        .ok_or("missing mixed-case dream log prefix")?;

    assert_eq!(parsed.id, CommandId::DreamLog);
    assert_eq!(parsed.kind, CommandKind::Prefix);
    assert_eq!(parsed.raw, "/dream-log AbC123");
    assert_eq!(parsed.args, "AbC123");
    Ok(())
}

#[test]
fn loop_command_parser_matches_builtin_router_semantics() {
    assert_eq!(parse_loop_command("/status now"), None);
    assert_eq!(parse_loop_command(" /new "), Some(LoopCommand::New));
    assert_eq!(parse_loop_command("/stop please"), None);
    assert_eq!(parse_loop_command("/restart"), Some(LoopCommand::Restart));
    assert_eq!(parse_loop_command("/dream"), Some(LoopCommand::Dream));
    assert_eq!(parse_loop_command("/help"), Some(LoopCommand::Help));
    assert_eq!(
        parse_loop_command("/history 25"),
        Some(LoopCommand::History(HistoryCommandArgs::Count(25)))
    );
    assert_eq!(
        parse_loop_command("/history 999"),
        Some(LoopCommand::History(HistoryCommandArgs::Count(50)))
    );
    assert_eq!(
        parse_loop_command("/history abc"),
        Some(LoopCommand::History(HistoryCommandArgs::Invalid))
    );
    assert_eq!(
        parse_loop_command("/dream-log abc"),
        Some(LoopCommand::DreamLog {
            sha: Some("abc".to_owned())
        })
    );
    assert_eq!(
        parse_loop_command("/dream-restore abc"),
        Some(LoopCommand::DreamRestore {
            sha: Some("abc".to_owned())
        })
    );
}

#[test]
fn channel_normalization_strips_bot_suffix_and_aliases_dream_commands() {
    assert_eq!(
        normalize_channel_command("/status@MyBot now", Some("mybot")),
        "/status now"
    );
    assert_eq!(
        normalize_channel_command("/dream_log abc", None),
        "/dream-log abc"
    );
    assert_eq!(
        normalize_channel_command("/dream_restore abc", Some("bot")),
        "/dream-restore abc"
    );
}

#[test]
fn builtin_command_detection_and_help_cover_registered_commands() {
    assert!(is_builtin_command("/dream-restore abc"));
    assert!(is_builtin_command("/status"));
    assert!(!is_builtin_command("/status now"));
    assert!(!is_builtin_command("hello"));

    let help = build_help_text();
    for command in [
        "/new",
        "/stop",
        "/restart",
        "/status",
        "/history",
        "/dream",
        "/dream-log",
        "/dream-restore",
        "/help",
    ] {
        assert!(help.contains(command), "help text missing {command}");
    }
}
