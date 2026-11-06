use super::*;
use crate::consent_prompt::ScriptedPrompt;
use serde_json::json;
use std::error::Error;
use std::path::Path;

const CONFIG: &str = r#"{
  "providers": {"custom": {"apiKey": "${CUSTOM_KEY}"}},
  "permissions": {
    "mode": "auto",
    "deny": ["exec(rm *)"],
    "autoMode": {"environment": ["$defaults", "[Mine] host spec048-reset-value-31"]}
  }
}"#;

fn setup(root: &Path) -> Result<AutoModeWriteOptions, Box<dyn Error>> {
    fs::write(root.join("config.json"), CONFIG)?;
    fs::write(
        root.join(".env.json"),
        r#"{"CUSTOM_KEY": "spec048-reset-value-31"}"#,
    )?;
    Ok(AutoModeWriteOptions {
        config_path: Some(root.join("config.json")),
        ..AutoModeWriteOptions::default()
    })
}

#[test]
fn reset_shows_masked_values_and_removes_only_auto_mode_after_yes() -> Result<(), Box<dyn Error>> {
    for (answer, yes, written) in [
        (Some("n"), false, false),
        (None, false, false),
        (Some("Y"), false, true),
        (None, true, true),
    ] {
        // Given
        let root = tempfile::tempdir()?;
        let options = AutoModeWriteOptions {
            yes,
            ..setup(root.path())?
        };
        let mut prompt = ScriptedPrompt::answering(answer);

        // When
        let reply = reset(&options, &mut prompt)?;

        // Then
        let label = format!("{answer:?} yes={yes}");
        let shown = prompt.shown.join("\n");
        assert!(shown.contains("[secret CUSTOM_KEY]"), "{label}: {shown}");
        assert!(
            !shown.contains("spec048-reset-value-31"),
            "{label}: {shown}"
        );
        let saved: Value = serde_json::from_slice(&fs::read(root.path().join("config.json"))?)?;
        if written {
            assert_eq!(
                reply, "Reset auto mode configuration to defaults.",
                "{label}"
            );
            assert_eq!(saved["permissions"].get("autoMode"), None, "{label}");
            assert_eq!(saved["permissions"]["mode"], "auto", "{label}");
            assert_eq!(
                saved["permissions"]["deny"],
                json!(["exec(rm *)"]),
                "{label}"
            );
            assert_eq!(
                saved["providers"]["custom"]["apiKey"], "${CUSTOM_KEY}",
                "{label}"
            );
        } else {
            assert_eq!(
                fs::read_to_string(root.path().join("config.json"))?,
                CONFIG,
                "{label}"
            );
        }
    }
    Ok(())
}

#[test]
fn reset_without_auto_mode_or_config_changes_nothing() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let missing = AutoModeWriteOptions {
        config_path: Some(root.path().join("missing.json")),
        yes: true,
        ..AutoModeWriteOptions::default()
    };
    let plain = root.path().join("plain.json");
    fs::write(&plain, r#"{"permissions": {"mode": "auto"}}"#)?;

    // When
    let first = reset(&missing, &mut ScriptedPrompt::answering(None))?;
    let second = reset(
        &AutoModeWriteOptions {
            config_path: Some(plain.clone()),
            yes: true,
            ..AutoModeWriteOptions::default()
        },
        &mut ScriptedPrompt::answering(None),
    )?;

    // Then
    assert_eq!(first, NOTHING_TO_RESET);
    assert_eq!(second, NOTHING_TO_RESET);
    assert!(!root.path().join("missing.json").exists());
    assert_eq!(
        fs::read_to_string(plain)?,
        r#"{"permissions": {"mode": "auto"}}"#
    );
    Ok(())
}

#[test]
fn reset_writes_nothing_when_the_config_changed_before_the_answer() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let options = setup(root.path())?;
    let path = root.path().join("config.json");
    let changed = path.clone();
    let mut prompt = ScriptedPrompt::answering(Some("yes"));
    prompt.before_answer = Some(Box::new(move || {
        let _ = fs::write(&changed, r#"{"permissions": {"mode": "default"}}"#);
    }));

    // When
    let result = reset(&options, &mut prompt);

    // Then
    let Err(error) = result else {
        return Err("a changed config was overwritten".into());
    };
    assert!(
        error.to_string().contains("changed while this command ran"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(path)?,
        r#"{"permissions": {"mode": "default"}}"#
    );
    Ok(())
}

#[test]
fn reset_refuses_a_workspace_it_would_not_use() {
    // When
    let parsed = crate::parse_cli_args([
        "permissions",
        "auto-mode",
        "reset",
        "--workspace",
        "/tmp/ws",
    ]);

    // Then
    assert!(parsed.is_err());
}
