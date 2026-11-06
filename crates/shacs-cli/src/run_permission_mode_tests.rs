use super::*;
use crate::tests::scripted_agent_loop_adapter;
use crate::{complete_direct_message, AskOptions};
use serde_json::{json, Value};
use shacs_command::SetupHintSwitch;
use shacs_core::runtime::RuleEffect;
use shacs_core::tools::ToolRegistry;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::thread;

const NOTICE: &str = "Auto mode is on because permissions.mode is not set";

fn adapter(
    root: &Path,
    config: Value,
    answers: usize,
) -> Result<AgentLoopChatCompletionAdapter, Box<dyn Error>> {
    let answer = LlmResponse {
        content: Some("done".to_owned()),
        finish_reason: "stop".to_owned(),
        ..LlmResponse::default()
    };
    scripted_agent_loop_adapter(
        root,
        config,
        ToolRegistry::new(),
        vec![answer; answers],
        PermissionMode::Default,
    )
}

fn interactive(adapter: &AgentLoopChatCompletionAdapter) -> AgentLoopConfig {
    adapter.loop_config_for(RunKind::Interactive)
}

#[test]
fn a_config_without_a_mode_starts_interactive_runs_in_auto_and_others_in_default(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let unset = adapter(
        root.path(),
        json!({"permissions": {"deny": ["exec(rm *)"]}}),
        0,
    )?;

    // When
    let background = unset.loop_config();
    let turn = interactive(&unset);
    let child = unset.subagent_execution_config(&turn);

    // Then
    assert_eq!(
        background.permission_mode_snapshot.mode,
        PermissionMode::Default
    );
    assert_eq!(
        background.permission_mode_snapshot.source.as_deref(),
        Some("default_fallback")
    );
    assert_eq!(turn.permission_mode_snapshot.mode, PermissionMode::Auto);
    assert_eq!(
        turn.permission_mode_snapshot.source.as_deref(),
        Some("interactive_default")
    );
    assert!(turn.permission_auto_approval.enabled);
    assert_eq!(
        turn.permission_ceiling_snapshot
            .as_ref()
            .map(|ceiling| ceiling.parent_mode),
        Some(PermissionMode::Auto)
    );
    assert_eq!(
        child.permission_mode_snapshot,
        turn.permission_mode_snapshot
    );
    assert_eq!(
        child
            .permission_ceiling_snapshot
            .map(|ceiling| ceiling.parent_mode),
        Some(PermissionMode::Auto)
    );
    let rules = turn
        .permission_environment
        .as_ref()
        .map(|environment| environment.user_rules.rule_texts(RuleEffect::Deny));
    assert_eq!(rules, Some(vec!["exec(rm *)"]));
    Ok(())
}

#[test]
fn a_written_unusable_or_unreadable_config_keeps_interactive_runs_in_default(
) -> Result<(), Box<dyn Error>> {
    for written in [
        json!({"permissions": {"mode": "default"}}),
        json!({"permissions": {"mode": "defualt"}}),
        json!({"permissions": {"autoMode": {"environment": "not a list"}}}),
    ] {
        // Given
        let root = tempfile::tempdir()?;
        let adapter = adapter(root.path(), written.clone(), 0)?;

        // When
        let turn = interactive(&adapter);

        // Then
        assert_eq!(
            turn.permission_mode_snapshot.mode,
            PermissionMode::Default,
            "{written}"
        );
        assert!(!turn.auto_mode_setup_hint, "{written}");
    }
    // Given: a config `load_config` cannot read.
    let root = tempfile::tempdir()?;
    let unreadable = adapter(
        root.path(),
        json!({"permissions": {"mode": "auto", "deny": ["exec(rm *)"]}, "agents": "not an object"}),
        0,
    )?;

    // When
    let turn = interactive(&unreadable);

    // Then: neither the written auto mode nor its rules apply.
    assert_eq!(
        turn.permission_mode_snapshot.source.as_deref(),
        Some("default_fallback")
    );
    let rules = turn
        .permission_environment
        .as_ref()
        .map(|environment| environment.user_rules.rule_texts(RuleEffect::Deny));
    assert_eq!(rules, Some(Vec::<&str>::new()));
    Ok(())
}

#[test]
fn the_setup_hint_needs_auto_mode_no_written_environment_and_no_off_marker(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let unset = adapter(root.path(), json!({}), 0)?;
    let setter = unset
        .loop_config()
        .auto_mode_setup_hint_setter
        .ok_or("no setup hint setter")?;
    let written_root = tempfile::tempdir()?;
    let written = adapter(
        written_root.path(),
        json!({"permissions": {"autoMode": {"environment": ["$defaults"]}}}),
        0,
    )?;

    // When
    let first = interactive(&unset).auto_mode_setup_hint;
    setter(SetupHintSwitch::Off)?;
    let after_off = interactive(&unset).auto_mode_setup_hint;
    setter(SetupHintSwitch::On)?;
    let after_on = interactive(&unset).auto_mode_setup_hint;

    // Then
    assert!(first && !after_off && after_on);
    assert!(!unset.loop_config().auto_mode_setup_hint);
    assert!(!interactive(&written).auto_mode_setup_hint);
    Ok(())
}

#[test]
fn the_first_default_notice_goes_once_and_never_without_its_marker() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let unset = adapter(root.path(), json!({}), 2)?;
    let not_a_folder = root.path().join("config.json");
    let ask = |message: &str| AskOptions {
        message: message.to_owned(),
        ..AskOptions::default()
    };

    // When
    let first = complete_direct_message(&unset, &ask("hello"))?;
    let second = complete_direct_message(&unset, &ask("again"))?;
    let unmarked = auto_mode_markers::claim_default_notice(&not_a_folder);

    // Then
    assert!(first.starts_with(NOTICE), "{first}");
    assert_eq!(second, "done");
    assert!(root
        .path()
        .join("runtime/auto-mode-default-notice")
        .exists());
    assert!(unmarked.is_err(), "{unmarked:?}");
    Ok(())
}

#[test]
fn runs_that_start_together_claim_the_notice_once() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let folder = root.path().to_path_buf();

    // When
    let claims = (0..8)
        .map(|_| {
            let folder = folder.clone();
            thread::spawn(move || auto_mode_markers::claim_default_notice(&folder))
        })
        .collect::<Vec<_>>();
    let mut sent = 0;
    for claim in claims {
        if claim.join().map_err(|_| "claim thread panicked")?? {
            sent += 1;
        }
    }

    // Then
    assert_eq!(sent, 1);
    Ok(())
}
