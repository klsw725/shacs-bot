mod readiness_owner_support;
mod readiness_runtime_support;

use readiness_owner_support::{Fixture, TestResult};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn assert_plugin_state(readiness: &Value, expected: &str) {
    let component = readiness["components"]
        .as_array()
        .expect("components")
        .iter()
        .find(|component| component["kind"] == "plugin_app")
        .expect("plugin/app");
    assert_eq!(component["requirement"], "required");
    assert_eq!(component["state"], expected);
    assert_ne!(
        readiness["envelope"]["state"], "ready",
        "no provider credentials were configured"
    );
}

fn write_plugin(root: &Path, limited: bool) -> TestResult {
    fs::create_dir_all(root.join("plugins/controlled"))?;
    let surfaces = if limited {
        serde_json::json!({"hooks": ["tool:before"]})
    } else {
        serde_json::json!({})
    };
    fs::write(
        root.join("plugins/controlled/plugin.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1, "name": "controlled", "version": "1", "surfaces": surfaces
        }))?,
    )?;
    Ok(())
}

#[test]
fn owner_readiness_changes_reach_real_cli_api_and_preserved_bundles() -> TestResult {
    let mut fixture = Fixture::new()?;
    fixture.cli("help", &["--help"])?;
    fixture.start()?;
    assert_plugin_state(&fixture.capture("empty-ready")?, "ready");

    fixture.config.plugins.enabled = vec!["controlled".to_owned()];
    fixture.save_config()?;
    write_plugin(&fixture.root, false)?;
    assert_plugin_state(&fixture.capture("active-ready")?, "ready");

    write_plugin(&fixture.root, true)?;
    assert_plugin_state(&fixture.capture("active-degraded")?, "degraded");

    fs::write(
        fixture.root.join("plugins/controlled/plugin.json"),
        "{corrupt",
    )?;
    assert_plugin_state(&fixture.capture("active-blocked")?, "blocked");

    write_plugin(&fixture.root, false)?;
    fs::remove_file(fixture.root.join("apps/registry.json"))?;
    assert_plugin_state(&fixture.capture("missing-unavailable")?, "unavailable");

    fs::write(fixture.root.join("apps/registry.json"), "{corrupt")?;
    assert_plugin_state(&fixture.capture("failed-unavailable")?, "unavailable");
    Ok(())
}

#[test]
fn runtime_owner_missing_stays_unavailable_on_cli() -> TestResult {
    let fixture = Fixture::new()?;
    let output = fixture.cli("owner-missing", &["runtime", "diagnostics"])?;
    let diagnostics: Value = serde_json::from_str(
        output
            .split_once("\nSpec031 ")
            .ok_or("diagnostics suffix")?
            .0,
    )?;
    let readiness = &diagnostics["runtime"]["spec031_readiness"];
    for kind in ["runtime_controls", "resource_disclosure"] {
        let component = readiness["components"]
            .as_array()
            .ok_or("components")?
            .iter()
            .find(|component| component["kind"] == kind)
            .ok_or("missing required Spec030 observation")?;
        assert_eq!(component["requirement"], "required");
        assert_eq!(component["state"], "unavailable");
        assert_eq!(component["freshness"], "unavailable");
        assert_eq!(component["reason_code"], "missing");
    }
    assert_eq!(readiness["trusted_runtime"]["availability"], "unavailable");
    assert_eq!(readiness["trusted_runtime"]["sandbox"]["status"], "unknown");
    Ok(())
}

#[test]
fn runtime_owner_observations_reach_cli_api_and_bundles_without_self_http() -> TestResult {
    readiness_runtime_support::runtime_owner_parity()
}
