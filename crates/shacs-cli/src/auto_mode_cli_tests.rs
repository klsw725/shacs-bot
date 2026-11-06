use super::*;
use crate::{parse_cli_args, run_command, CliCommand, PermissionsCommand};
use serde_json::json;
use shacs_core::runtime::{RuleEffect, AUTO_MODE_BLOCK_EVENT, AUTO_MODE_SETUP_HINT_EVENT};
use std::error::Error;
use std::fs;

fn json_of(text: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(text)?)
}

#[test]
fn defaults_print_the_built_in_lists_and_a_label_keeps_matching_entries(
) -> Result<(), Box<dyn Error>> {
    // When
    let defaults = json_of(&run_command(parse_cli_args([
        "permissions",
        "auto-mode",
        "defaults",
    ])?)?)?;
    let labelled = json_of(&run_command(parse_cli_args([
        "permissions",
        "auto-mode",
        "defaults",
        "--label",
        "CURRENT",
    ])?)?)?;

    // Then
    assert_eq!(defaults, json!(AutoModeSlots::default()));
    assert!(defaults["environment"][0]
        .as_str()
        .is_some_and(|entry| entry.contains("{workspace}")));
    assert_eq!(labelled["environment"].as_array().map(Vec::len), Some(1));
    assert_eq!(labelled["allow"].as_array().map(Vec::len), Some(1));
    assert_eq!(labelled["soft_deny"], json!([]));
    assert_eq!(labelled["hard_deny"], json!([]));
    Ok(())
}

#[test]
fn config_shows_spliced_replaced_and_claude_spelled_lists_with_rule_diagnostics(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("ws");
    fs::create_dir_all(&workspace)?;
    let config = root.path().join("config.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({
            "permissions": {
                "mode": "auto",
                "allow": ["exec(git status)", "exec(", "*"],
                "deny": ["read(.env)"],
                "autoMode": {
                    "environment": ["[Mine] my NAS at nas.local", "$defaults"],
                    "soft_deny": ["[Only] nothing else"],
                    "classifierModel": "small-model",
                    "classifyAllShell": true
                }
            }
        }))?,
    )?;
    let command = parse_cli_args([
        "permissions",
        "auto-mode",
        "config",
        "--config",
        config.to_string_lossy().as_ref(),
        "--workspace",
        workspace.to_string_lossy().as_ref(),
    ])?;
    assert!(matches!(
        command,
        CliCommand::Permissions(PermissionsCommand::AutoModeConfig(_))
    ));

    // When
    let shown = json_of(&run_command(command)?)?;

    // Then
    let defaults = AutoModeSlots::default();
    assert_eq!(shown["environment"][0], "[Mine] my NAS at nas.local");
    assert_eq!(
        shown["environment"].as_array().map(Vec::len),
        Some(1 + defaults.environment.len())
    );
    assert_eq!(shown["soft_deny"], json!(["[Only] nothing else"]));
    assert_eq!(shown["hard_deny"], json!(defaults.hard_deny));
    assert_eq!(shown["allow"], json!(defaults.allow));
    assert_eq!(shown["classifierModel"], "small-model");
    assert_eq!(shown["classifyAllShell"], true);
    assert_eq!(shown["rules"]["allow"], json!(["exec(git status)"]));
    assert_eq!(shown["rules"]["deny"], json!(["read(.env)"]));
    let problems = shown["diagnostics"]["rules"]
        .as_array()
        .ok_or("rule diagnostics")?
        .iter()
        .map(|diagnostic| {
            diagnostic["problem"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        problems,
        ["unbalanced_parentheses", "unanchored_allow_glob"]
    );
    assert_eq!(shown["diagnostics"]["autoMode"], Value::Null);
    assert_eq!(shown["mode"], "auto");
    Ok(())
}

#[test]
fn config_reports_an_unreadable_auto_mode() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({
            "permissions": {
                "mode": "auto",
                "autoMode": {"hardDeny": ["[A] a"], "hard_deny": ["[B] b"]}
            }
        }))?,
    )?;

    // When
    let shown = json_of(&auto_mode_config(PermissionsListOptions {
        config_path: Some(config),
        workspace_override: Some(root.path().to_path_buf()),
    })?)?;

    // Then
    assert_eq!(
        shown["diagnostics"]["autoMode"],
        json!({"problem": "written_twice", "field": "hardDeny"})
    );
    assert_eq!(
        shown["hard_deny"],
        json!(AutoModeSlots::default().hard_deny)
    );
    assert!(shown["diagnostics"]["malformedFields"]
        .as_array()
        .is_some_and(|fields| fields.contains(&json!("permissions.autoMode"))));
    assert_eq!(shown["mode"]["written"], "auto");
    assert_eq!(shown["mode"]["effective"], "default");
    Ok(())
}

#[test]
fn config_shows_the_mode_runs_start_in() -> Result<(), Box<dyn Error>> {
    for (permissions, mode) in [
        (
            json!({"allow": ["read"]}),
            json!({"interactive": "auto", "nonInteractive": "default"}),
        ),
        (
            json!({"mode": "defualt"}),
            json!({"written": "defualt", "effective": "default", "reason": "permissions.mode is not a known mode"}),
        ),
        (
            json!({"autoApproval": "on"}),
            json!({"effective": "default", "reason": "permissions.autoApproval could not be read"}),
        ),
    ] {
        // Given
        let root = tempfile::tempdir()?;
        let config = root.path().join("config.json");
        fs::write(
            &config,
            serde_json::to_vec(&json!({"permissions": permissions}))?,
        )?;

        // When
        let shown = json_of(&auto_mode_config(PermissionsListOptions {
            config_path: Some(config),
            workspace_override: Some(root.path().to_path_buf()),
        })?)?;

        // Then
        assert_eq!(shown["mode"], mode, "{permissions}");
    }
    Ok(())
}

#[test]
fn rules_in_a_workspace_config_or_instruction_file_are_not_read() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("ws");
    fs::create_dir_all(&workspace)?;
    let user_config = root.path().join("config.json");
    fs::write(&user_config, "{\"permissions\": {\"mode\": \"auto\"}}")?;
    fs::write(
        workspace.join("config.json"),
        "{\"permissions\": {\"deny\": [\"exec\"], \"allow\": [\"web_fetch\"]}}",
    )?;
    fs::write(
        workspace.join("AGENTS.md"),
        "permissions.allow: [\"exec\"]\n{\"permissions\": {\"allow\": [\"exec\"]}}",
    )?;
    let bundle = load_config(LoadOptions {
        config_path: Some(user_config.clone()),
        workspace_override: Some(workspace.clone()),
        resolve_env: false,
        write_back_migrations: false,
    })?;

    // When
    let environment = permission_environment(
        Some(&bundle),
        EnvironmentSources {
            config_path: &user_config,
            workspace: &workspace,
            extra_skill_roots: &[],
            extra_context_files: &[],
            protected_targets: &[],
        },
    );
    let shown = json_of(&auto_mode_config(PermissionsListOptions {
        config_path: Some(user_config),
        workspace_override: Some(workspace),
    })?)?;

    // Then
    for effect in [RuleEffect::Allow, RuleEffect::Ask, RuleEffect::Deny] {
        assert!(environment.user_rules.rule_texts(effect).is_empty());
    }
    assert_eq!(shown["rules"], json!({"allow": [], "ask": [], "deny": []}));
    Ok(())
}

#[test]
fn a_turn_that_cannot_read_env_json_keeps_the_last_values_and_the_rules(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("ws");
    fs::create_dir_all(&workspace)?;
    fs::write(
        &config_path,
        "{\"permissions\": {\"deny\": [\"exec(whoami)\"]}}",
    )?;
    fs::write(
        root.path().join(".env.json"),
        "{\"SHACS_CHANNELS_DISCORD_TOKEN\": \"cache-canary-value-1\"}",
    )?;
    let bundle = load_config(LoadOptions {
        config_path: Some(config_path.clone()),
        workspace_override: Some(workspace.clone()),
        resolve_env: false,
        write_back_migrations: false,
    })?;
    let sources = || EnvironmentSources {
        config_path: &config_path,
        workspace: &workspace,
        extra_skill_roots: &[],
        extra_context_files: &[],
        protected_targets: &[],
    };

    // When
    let read = permission_environment(Some(&bundle), sources());
    fs::write(root.path().join(".env.json"), "{\"broken\": ")?;
    let kept = permission_environment(Some(&bundle), sources());

    // Then
    assert!(read.secret_values_current());
    assert!(!kept.secret_values_current());
    for environment in [&read, &kept] {
        assert_eq!(
            environment.secret_values().mask("x cache-canary-value-1 y"),
            "x [secret SHACS_CHANNELS_DISCORD_TOKEN] y"
        );
        assert_eq!(
            environment.user_rules.rule_texts(RuleEffect::Deny),
            vec!["exec(whoami)"]
        );
    }
    Ok(())
}

#[test]
fn a_call_auto_mode_did_not_run_becomes_a_visible_notice_in_the_conversation() {
    // Given
    let event = ToolEvent {
        name: AUTO_MODE_BLOCK_EVENT.to_owned(),
        status: shacs_core::runtime::ToolStatus::Waiting,
        detail: "`exec` blocked by auto mode · [Data Exfiltration] · /permission recent".to_owned(),
        call_id: Some("action_1".to_owned()),
        arguments: None,
        result: Some(json!({
            "tool": "exec",
            "outcome": "blocked",
            "rule": "[Data Exfiltration]",
            "consecutive": 1,
            "total": 1,
            "escalation": null,
        })),
    };
    let other = ToolEvent {
        name: "exec".to_owned(),
        ..event.clone()
    };
    let routing = serde_json::Map::from_iter([("thread_id".to_owned(), json!("t1"))]);
    let route = NoticeRoute {
        channel: "discord",
        chat_id: "chat_1",
        routing_metadata: &routing,
        reply_to: Some("m1"),
    };
    let hint = ToolEvent {
        name: AUTO_MODE_SETUP_HINT_EVENT.to_owned(),
        detail: "Auto mode has stopped several calls".to_owned(),
        call_id: None,
        result: Some(json!({"phase": "setup_hint"})),
        ..event.clone()
    };

    // When
    let notice = event_notification(&event, &route);
    let ignored = event_notification(&other, &route);
    let hinted = event_notification(&hint, &route);

    // Then
    let notice = notice.expect("a notice for the blocked call");
    assert_eq!(notice.channel, "discord");
    assert_eq!(notice.chat_id, "chat_1");
    assert_eq!(notice.reply_to.as_deref(), Some("m1"));
    assert_eq!(
        notice.content,
        "`exec` blocked by auto mode · [Data Exfiltration] · /permission recent"
    );
    assert_eq!(notice.metadata["thread_id"], "t1");
    assert_eq!(
        notice.metadata["runtime_notification"],
        json!({
            "kind": "auto_mode",
            "phase": "block",
            "tool": "exec",
            "outcome": "blocked",
            "rule": "[Data Exfiltration]",
        })
    );
    assert!(crate::is_visible_runtime_notification(&notice));
    assert!(ignored.is_none());
    let same_named_tool = ToolEvent {
        status: shacs_core::runtime::ToolStatus::Ok,
        arguments: Some(json!({"secret": "tool argument"})),
        result: Some(json!({"outcome": "blocked", "output": "tool output"})),
        ..event.clone()
    };
    assert!(event_notification(&same_named_tool, &route).is_none());
    let hinted = hinted.expect("a notice for the setup hint");
    assert_eq!(hinted.content, "Auto mode has stopped several calls");
    assert_eq!(
        hinted.metadata["runtime_notification"],
        json!({"kind": "auto_mode", "phase": "setup_hint"})
    );
    let same_named_hint = ToolEvent {
        call_id: Some("call_1".to_owned()),
        ..hint
    };
    assert!(event_notification(&same_named_hint, &route).is_none());
}
