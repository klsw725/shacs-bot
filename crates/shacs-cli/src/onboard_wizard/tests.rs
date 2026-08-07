use std::error::Error;
use std::{fs, io};

use serde_json::{json, Value};

use super::{config_apply, io_loop, readiness, resume, OnboardWizardStatus};
use crate::{format_onboard_outcome, OnboardOptions};

#[test]
fn onboard_wizard_completes_with_secret_ref_and_owner_facts() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("workspace");
    let mut output = Vec::new();

    let outcome = super::run(
        OnboardOptions {
            config_path: Some(config_path.clone()),
            workspace: Some(workspace),
            wizard: true,
        },
        io::Cursor::new("provider openrouter env OPENROUTER_API_KEY\nfinish\n"),
        &mut output,
    )?;

    let report = outcome.wizard_report.as_ref().ok_or("missing report")?;
    assert_eq!(report.status, OnboardWizardStatus::Complete);
    assert!(report.external_owner_facts.iter().any(
        |fact| fact.owner == "spec031" && fact.reason_code == "missing_external_owner_evidence"
    ));
    let saved: Value = serde_json::from_str(&fs::read_to_string(&config_path)?)?;
    assert_eq!(
        saved["providers"]["openrouter"]["apiKeyRef"]["locator"]["name"],
        json!("OPENROUTER_API_KEY")
    );
    assert_eq!(
        saved["providers"]["openrouter"]["apiKeyRef"]["owner"],
        json!("spec031-config-profile")
    );
    assert_eq!(
        saved["providers"]["openrouter"]["apiKeyRef"]["staleness_token"],
        json!("sha256:spec031-open")
    );
    assert!(!fs::read_to_string(&config_path)?.contains("sk-live-secret"));
    let rendered = format_onboard_outcome(outcome);
    assert!(rendered.contains("External owner facts:"));
    assert!(!String::from_utf8(output)?.contains("sk-live-secret"));
    Ok(())
}

#[test]
fn onboard_wizard_fails_closed_for_all_existing_key_aliases() -> Result<(), Box<dyn Error>> {
    for alias in ["apiKey", "api_key", "apiKeyRef", "api_key_ref"] {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        fs::write(
            &config_path,
            serde_json::to_string_pretty(
                &json!({"providers": {"openrouter": {alias: "sk-live-secret"}}}),
            )?,
        )?;
        let mut output = Vec::new();

        let error = super::run(
            OnboardOptions {
                config_path: Some(config_path.clone()),
                workspace: Some(root.path().join("workspace")),
                wizard: true,
            },
            io::Cursor::new("provider openrouter env OPENROUTER_API_KEY\nfinish\n"),
            &mut output,
        )
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();

        assert!(error.contains("will not overwrite"));
        assert!(fs::read_to_string(config_path)?.contains("sk-live-secret"));
        assert!(!String::from_utf8(output)?.contains("sk-live-secret"));
    }
    Ok(())
}

#[test]
fn onboard_wizard_resume_marker_is_typed_and_idempotent() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("workspace");
    let mut first_output = Vec::new();

    let partial = super::run(
        OnboardOptions { config_path: Some(config_path.clone()), workspace: Some(workspace.clone()), wizard: true },
        io::Cursor::new("provider openrouter env OPENROUTER_API_KEY\nprovider openrouter env OPENROUTER_API_KEY\n"),
        &mut first_output,
    )?;

    assert_eq!(
        partial
            .wizard_report
            .as_ref()
            .ok_or("missing partial")?
            .status,
        OnboardWizardStatus::Partial
    );
    let marker = resume::path(&config_path);
    let marker_value: Value = serde_json::from_str(&fs::read_to_string(&marker)?)?;
    assert_eq!(
        marker_value["provider_secret_refs"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        marker_value["provider_secret_refs"][0]["source_kind"],
        json!("env")
    );
    assert_eq!(
        marker_value["provider_secret_refs"][0]["locator"],
        json!("OPENROUTER_API_KEY")
    );

    let mut second_output = Vec::new();
    let resumed = super::run(
        OnboardOptions {
            config_path: Some(config_path.clone()),
            workspace: Some(workspace),
            wizard: true,
        },
        io::Cursor::new("finish\n"),
        &mut second_output,
    )?;

    let report = resumed.wizard_report.as_ref().ok_or("missing resumed")?;
    assert_eq!(report.status, OnboardWizardStatus::Complete);
    assert!(report.resumed);
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn onboard_wizard_rejects_raw_like_refs_without_echo_or_marker() -> Result<(), Box<dyn Error>> {
    let invalid_refs = [
        "sk-live-secret",
        "https://user:pass@example.test/token",
        "TOKEN=value",
        "openrouter_api_key",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
        "A1B2C3D4E5F6G7H8I9J0K1L2",
        "AAA.BBB.CCC",
        "SK_LIVE_SECRET",
        "OPENROUTER__API_KEY",
        "_OPENROUTER_API_KEY",
        "OPENROUTER_API_KEY_",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLM",
    ];
    for invalid_ref in invalid_refs {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let mut output = Vec::new();
        let input = format!("provider openrouter env {invalid_ref}\n");
        let error = super::run(
            OnboardOptions {
                config_path: Some(config_path.clone()),
                workspace: Some(root.path().join("workspace")),
                wizard: true,
            },
            io::Cursor::new(input),
            &mut output,
        )
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(!error.contains(invalid_ref));
        assert!(!String::from_utf8(output)?.contains(invalid_ref));
        assert!(!resume::path(&config_path).exists());
    }
    Ok(())
}

#[test]
fn onboard_wizard_accepts_bounded_semantic_env_refs() -> Result<(), Box<dyn Error>> {
    for valid_ref in ["OPENROUTER_API_KEY", "ANTHROPIC_API_KEY", "MODEL2_API_KEY"] {
        config_apply::parse_env_ref(valid_ref)?;
    }
    Ok(())
}

#[test]
fn onboard_wizard_owner_facts_are_canonical_missing_external_evidence() {
    let facts = readiness::external_owner_facts();
    assert!(facts
        .iter()
        .any(|fact| fact.owner == "spec030" && fact.capability == "approval"));
    assert!(facts
        .iter()
        .any(|fact| fact.owner == "spec031" && fact.capability == "readiness"));
    assert!(facts
        .iter()
        .all(|fact| fact.state == "unavailable"
            && fact.reason_code == "missing_external_owner_evidence"));
}

#[test]
fn onboard_wizard_malformed_command_is_sanitized() {
    let raw = "sk-live-secret";
    let error = io_loop::parse_command(&format!("provider openrouter raw {raw}"))
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(!error.contains(raw));
}
