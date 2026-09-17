use std::{error::Error, fs, io};

use serde_json::{json, Value};

use crate::OnboardOptions;

#[test]
fn onboard_wizard_consumes_declarations_without_claiming_approval() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let command_marker = root.path().join("credential-command-must-not-run");
    let auth_path = root.path().join("auth.json");
    let auth = b"{}";
    fs::write(&auth_path, auth)?;
    let original = json!({
        "providers": {"openrouter": {"credentialSource": {
            "schemaVersion": 1, "environment": "OWNER_FIXTURE_KEY", "localAuth": false,
            "command": format!("touch {}", command_marker.display())
        }}},
        "profiles": {"providers": {"fixture": {"provider": "openrouter",
            "credentialSource": {"schemaVersion": 1, "environment": "PROFILE_FIXTURE_KEY", "localAuth": true}
        }}, "selection": {"provider": "fixture"}}
    });
    fs::write(&config_path, serde_json::to_vec(&original)?)?;

    let outcome = super::run(
        OnboardOptions {
            config_path: Some(config_path.clone()),
            workspace: Some(root.path().join("workspace")),
            wizard: true,
        },
        io::Cursor::new("finish\n"),
        Vec::new(),
    )?;

    let report = outcome.wizard_report.as_ref().ok_or("missing report")?;
    let facts = serde_json::to_value(&report.external_owner_facts)?;
    assert_eq!(facts[0]["owner"], "spec030");
    assert_eq!(
        facts[0]["projection"]["credential"]["status"],
        "unavailable"
    );
    assert_eq!(facts[1]["owner"], "spec031");
    assert_eq!(facts[1]["profile_selection"], "available");
    let declarations = facts[1]["declarations"]
        .as_array()
        .ok_or("missing declarations")?;
    assert!(declarations
        .iter()
        .any(|value| value["origin"] == "provider"
            && value["environment"] == true
            && value["local_auth"] == false
            && value["command"] == true));
    assert!(declarations
        .iter()
        .any(|value| value["origin"] == "provider_profile"
            && value["environment"] == true
            && value["local_auth"] == true));
    assert!(!serde_json::to_string(&facts)?.contains("approval"));
    assert!(!serde_json::to_string(&facts)?.contains("credential-command-must-not-run"));
    assert!(!serde_json::to_string(&facts)?.contains(root.path().to_str().ok_or("fixture path")?));
    assert!(!command_marker.exists());
    let saved: Value = serde_json::from_slice(&fs::read(&config_path)?)?;
    assert_eq!(
        saved["providers"]["openrouter"],
        original["providers"]["openrouter"]
    );
    assert_eq!(saved["profiles"], original["profiles"]);
    assert_eq!(fs::read(&auth_path)?, auth);
    println!("owner-facts={facts}");
    println!("rendered={}", crate::format_onboard_outcome(outcome));
    root.close()?;
    println!("cleanup=owned-tempdir-closed");
    Ok(())
}

#[test]
fn onboard_wizard_cancel_preserves_config_and_auth_after_interruption() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let auth_path = root.path().join("auth.json");
    let config = b"{\"providers\": {}, \"env\": {\"PRESERVED_KEY\": \"fixture-value\"}}";
    let auth = b"{}";
    fs::write(&config_path, config)?;
    fs::write(&auth_path, auth)?;
    let options = OnboardOptions {
        config_path: Some(config_path.clone()),
        workspace: Some(root.path().join("workspace")),
        wizard: true,
    };
    let partial = super::run(
        options.clone(),
        io::Cursor::new("provider openrouter env FIXTURE_KEY\n"),
        Vec::new(),
    )?;
    assert_eq!(
        partial.wizard_report.ok_or("missing partial")?.status,
        super::OnboardWizardStatus::Partial
    );

    let outcome = super::run(options, io::Cursor::new("help\ncancel\n"), Vec::new())?;

    let report = outcome
        .wizard_report
        .as_ref()
        .ok_or("missing cancellation")?;
    assert_eq!(report.status, super::OnboardWizardStatus::Cancelled);
    assert!(report.resumed);
    assert!(report.external_owner_facts.is_empty());
    assert_eq!(fs::read(&config_path)?, config);
    assert_eq!(fs::read(&auth_path)?, auth);
    assert!(!super::resume::path(&config_path).exists());
    println!(
        "cancelled={}\nconfig=byte-preserved auth=byte-preserved",
        crate::format_onboard_outcome(outcome)
    );
    root.close()?;
    println!("cleanup=owned-tempdir-closed");
    Ok(())
}

#[test]
fn onboard_wizard_missing_selected_profile_is_not_available() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        br#"{"profiles":{"selection":{"provider":"absent-private-profile"}}}"#,
    )?;

    let outcome = super::run(
        OnboardOptions {
            config_path: Some(config_path),
            workspace: Some(root.path().join("workspace")),
            wizard: true,
        },
        io::Cursor::new("finish\n"),
        Vec::new(),
    )?;

    let report = outcome.wizard_report.as_ref().ok_or("missing report")?;
    let facts = serde_json::to_value(&report.external_owner_facts)?;
    assert_eq!(facts[1]["profile_selection"], "missing");
    assert!(!facts.to_string().contains("absent-private-profile"));
    println!("missing-profile={}", facts[1]);
    root.close()?;
    println!("cleanup=owned-tempdir-closed");
    Ok(())
}
