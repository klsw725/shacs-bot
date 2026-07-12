use shacs_command::{
    build_help_text, is_builtin_command, is_builtin_command_name, normalize_channel_command,
    parse_loop_command, parse_loop_command_route, CommandId, CommandKind, CommandRouter,
    GoalCommandArgs, HistoryCommandArgs, LoopCommand, PermissionCommandArgs, PluginCommandRouter,
    PluginCommandSpec,
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
    assert!(router.is_dispatchable_command("/goal ship PRD 001"));
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
    assert_eq!(
        parse_loop_command(" /permission "),
        Some(LoopCommand::Permission(PermissionCommandArgs::ModeWizard))
    );
    assert_eq!(
        parse_loop_command("/permission recent"),
        Some(LoopCommand::Permission(PermissionCommandArgs::Recent))
    );
    assert_eq!(
        parse_loop_command("/permission recent retry auto_denial_abc123"),
        Some(LoopCommand::Permission(PermissionCommandArgs::RecentRetry(
            "auto_denial_abc123".to_owned()
        )))
    );
    assert_eq!(
        parse_loop_command("/permission auto"),
        Some(LoopCommand::Permission(PermissionCommandArgs::Invalid))
    );
    assert_eq!(parse_loop_command("/stop please"), None);
    assert_eq!(parse_loop_command("/restart"), Some(LoopCommand::Restart));
    assert_eq!(parse_loop_command("/dream"), Some(LoopCommand::Dream));
    assert_eq!(parse_loop_command("/help"), Some(LoopCommand::Help));
    assert_eq!(
        parse_loop_command("/goal"),
        Some(LoopCommand::Goal(GoalCommandArgs::Status))
    );
    assert_eq!(
        parse_loop_command("/goal status"),
        Some(LoopCommand::Goal(GoalCommandArgs::Status))
    );
    assert_eq!(
        parse_loop_command("/goal pause"),
        Some(LoopCommand::Goal(GoalCommandArgs::Pause))
    );
    assert_eq!(
        parse_loop_command("/goal resume"),
        Some(LoopCommand::Goal(GoalCommandArgs::Resume))
    );
    assert_eq!(
        parse_loop_command("/goal clear"),
        Some(LoopCommand::Goal(GoalCommandArgs::Clear))
    );
    assert_eq!(
        parse_loop_command("/goal done"),
        Some(LoopCommand::Goal(GoalCommandArgs::Done))
    );
    assert_eq!(
        parse_loop_command("/goal blocked waiting for token"),
        Some(LoopCommand::Goal(GoalCommandArgs::Blocked(
            "waiting for token".to_owned()
        )))
    );
    assert_eq!(
        parse_loop_command("/goal blocked"),
        Some(LoopCommand::Goal(GoalCommandArgs::Invalid))
    );
    assert_eq!(
        parse_loop_command("/goal ship PRD 001"),
        Some(LoopCommand::Goal(GoalCommandArgs::Set(
            "ship PRD 001".to_owned()
        )))
    );
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
fn permission_recent_retry_command_parses_denial_id() {
    assert_eq!(
        parse_loop_command("/permission recent retry auto_denial_deadbeef"),
        Some(LoopCommand::Permission(PermissionCommandArgs::RecentRetry(
            "auto_denial_deadbeef".to_owned()
        )))
    );
}

#[test]
fn permission_recent_retry_rejects_missing_denial_id() {
    assert_eq!(
        parse_loop_command("/permission recent retry"),
        Some(LoopCommand::Permission(PermissionCommandArgs::Invalid))
    );
}

#[test]
fn loop_command_route_preserves_priority_exact_and_prefix_boundary() -> Result<(), Box<dyn Error>> {
    let priority = parse_loop_command_route(" /status ").ok_or("missing status route")?;
    assert_eq!(priority.command, LoopCommand::Status);
    assert_eq!(priority.parsed.kind, CommandKind::Priority);

    let exact = parse_loop_command_route("/new").ok_or("missing new route")?;
    assert_eq!(exact.command, LoopCommand::New);
    assert_eq!(exact.parsed.kind, CommandKind::Exact);

    let prefix = parse_loop_command_route("/history 25").ok_or("missing history route")?;
    assert_eq!(
        prefix.command,
        LoopCommand::History(HistoryCommandArgs::Count(25))
    );
    assert_eq!(prefix.parsed.kind, CommandKind::Prefix);
    assert_eq!(prefix.parsed.args, "25");

    let goal = parse_loop_command_route("/goal Ship It").ok_or("missing goal route")?;
    assert_eq!(
        goal.command,
        LoopCommand::Goal(GoalCommandArgs::Set("Ship It".to_owned()))
    );
    assert_eq!(goal.parsed.kind, CommandKind::Prefix);
    assert_eq!(goal.parsed.args, "Ship It");
    Ok(())
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
    assert!(is_builtin_command("/goal ship PRD 001"));
    assert!(is_builtin_command("/status"));
    assert!(is_builtin_command("/permission"));
    assert!(is_builtin_command("/permission recent"));
    assert!(is_builtin_command("/permission auto"));
    assert!(!is_builtin_command("/status now"));
    assert!(!is_builtin_command("hello"));

    let help = build_help_text();
    for command in [
        "/new",
        "/stop",
        "/restart",
        "/status",
        "/permission",
        "/goal",
        "/history",
        "/dream",
        "/dream-log",
        "/dream-restore",
        "/help",
    ] {
        assert!(help.contains(command), "help text missing {command}");
    }
    assert!(help.contains("subsequent turns"));
    assert!(!help.contains("requires restart"));
}

#[test]
fn plugin_command_router_routes_without_extending_builtin_command_ids() -> Result<(), Box<dyn Error>>
{
    let router = PluginCommandRouter::new([
        PluginCommandSpec::new("review-plugin", "review"),
        PluginCommandSpec::new("daily-plugin", "/daily"),
    ]);

    let review = router
        .dispatch("  /Review today  ")
        .ok_or("missing plugin review route")?;
    assert_eq!(review.plugin_id, "review-plugin");
    assert_eq!(review.name, "review");
    assert_eq!(review.matched, "/review");
    assert_eq!(review.raw, "/Review today");
    assert_eq!(review.args, "today");
    assert_eq!(parse_loop_command("/review today"), None);
    assert!(router.dispatch("hello").is_none());
    Ok(())
}

#[test]
fn plugin_command_router_excludes_builtin_conflicts() {
    let router = PluginCommandRouter::new([
        PluginCommandSpec::new("bad-plugin", "status"),
        PluginCommandSpec::new("ok-plugin", "triage"),
    ]);

    assert!(is_builtin_command_name("status"));
    assert!(is_builtin_command_name("/help"));
    assert!(router.dispatch("/status").is_none());
    assert_eq!(
        router.dispatch("/triage bug").map(|route| route.plugin_id),
        Some("ok-plugin".to_owned())
    );
}
