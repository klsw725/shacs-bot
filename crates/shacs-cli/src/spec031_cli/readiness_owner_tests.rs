use crate::{runtime_inspect, RuntimeInspectOptions};
use serde_json::json;
use shacs_app::app::{AppId, AppRegistry, AppRegistryStore};
use shacs_app::app_lifecycle::{AppLifecycleAction, AppProcessState, AppSupervisorJournal};
use shacs_config::{save_config_to_path, Config};
use shacs_projection::{
    Spec031Availability, Spec031ReadinessComponentKind, Spec031ReadinessRequirement,
};
use std::{error::Error, fs, path::Path};

type TestResult = Result<(), Box<dyn Error>>;

fn fixture() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("workspace/.shacs-bot/plugins"))?;
    fs::create_dir_all(root.path().join("plugins"))?;
    let mut config = Config::default();
    config.agents.defaults.workspace = root.path().join("workspace").to_string_lossy().into_owned();
    save_config_to_path(&config, &root.path().join("config.json"))?;
    AppRegistryStore::new(root.path()).save(&AppRegistry::default())?;
    Ok(root)
}

fn assert_state(root: &Path, expected: Spec031Availability) -> TestResult {
    let inspect = runtime_inspect(RuntimeInspectOptions {
        config_path: Some(root.join("config.json")),
        workspace_override: Some(root.join("workspace")),
    })?;
    let report = super::readiness::report(&inspect)?;
    let actual = report
        .components()
        .iter()
        .find(|component| component.kind == Spec031ReadinessComponentKind::PluginApp)
        .ok_or("missing plugin/app component")?;
    assert_eq!(actual.requirement, Spec031ReadinessRequirement::Required);
    assert_eq!(actual.state, expected);
    Ok(())
}

fn plugin(root: &Path, manifest: serde_json::Value) -> TestResult {
    let config_path = root.join("config.json");
    let mut config: Config = serde_json::from_slice(&fs::read(&config_path)?)?;
    config.plugins.enabled = vec!["controlled".to_owned()];
    save_config_to_path(&config, &config_path)?;
    fs::create_dir(root.join("plugins/controlled"))?;
    fs::write(
        root.join("plugins/controlled/plugin.json"),
        serde_json::to_vec(&manifest)?,
    )?;
    Ok(())
}

fn app(root: &Path, state: Option<AppProcessState>) -> TestResult {
    let store = AppRegistryStore::new(root);
    let registry = serde_json::from_value(json!({"entries": {"controlled": {
        "appId": "controlled", "version": "1", "digest": "controlled-descriptor",
        "bundlePath": root.join("apps/controlled.app"), "lifecycleState": "enabled"
    }}}))?;
    store.save(&registry)?;
    if let Some(state) = state {
        let journal = AppSupervisorJournal::new(root.join("apps"));
        let requested = journal.request(&AppId::parse("controlled")?, AppLifecycleAction::Start)?;
        if state != AppProcessState::Starting {
            journal.complete(
                &requested,
                state,
                "controlled-descriptor",
                "controlled-owner-fixture",
                vec![],
            )?;
        }
    }
    Ok(())
}

#[test]
fn ready_when_authoritative_owner_collections_are_empty() -> TestResult {
    let root = fixture()?;
    assert_state(root.path(), Spec031Availability::Ready)
}

#[test]
fn unavailable_when_app_registry_evidence_is_absent() -> TestResult {
    let root = fixture()?;
    fs::remove_file(root.path().join("apps/registry.json"))?;
    assert_state(root.path(), Spec031Availability::Unavailable)
}

#[test]
fn unavailable_when_app_registry_lookup_fails() -> TestResult {
    let root = fixture()?;
    fs::write(root.path().join("apps/registry.json"), "{corrupt")?;
    assert_state(root.path(), Spec031Availability::Unavailable)
}

#[test]
fn unavailable_when_plugin_enumeration_evidence_is_absent() -> TestResult {
    let root = fixture()?;
    fs::remove_dir(root.path().join("plugins"))?;
    fs::remove_dir(root.path().join("workspace/.shacs-bot/plugins"))?;
    assert_state(root.path(), Spec031Availability::Unavailable)
}

#[test]
fn ready_when_enabled_plugin_owner_checks_succeed() -> TestResult {
    let root = fixture()?;
    plugin(
        root.path(),
        json!({"schemaVersion": 1, "name": "controlled", "version": "1"}),
    )?;
    assert_state(root.path(), Spec031Availability::Ready)
}

#[test]
fn degraded_when_enabled_plugin_runtime_reports_missing_entrypoint() -> TestResult {
    let root = fixture()?;
    plugin(
        root.path(),
        json!({"schemaVersion": 1, "name": "controlled", "version": "1",
        "surfaces": {"hooks": ["tool:before"]}}),
    )?;
    assert_state(root.path(), Spec031Availability::Degraded)
}

#[test]
fn blocked_when_enabled_plugin_owner_gate_fails() -> TestResult {
    let root = fixture()?;
    plugin(
        root.path(),
        json!({"schemaVersion": 1, "name": "controlled", "version": "1",
        "requiresConfig": ["CONTROLLED_MISSING_REF"]}),
    )?;
    assert_state(root.path(), Spec031Availability::Blocked)
}

#[test]
fn unavailable_when_configured_plugin_is_not_discovered() -> TestResult {
    let root = fixture()?;
    plugin(
        root.path(),
        json!({"schemaVersion": 1, "name": "different", "version": "1"}),
    )?;
    assert_state(root.path(), Spec031Availability::Unavailable)
}

#[test]
fn ready_when_enabled_app_owner_reports_running() -> TestResult {
    let root = fixture()?;
    app(root.path(), Some(AppProcessState::Running))?;
    assert_state(root.path(), Spec031Availability::Ready)
}

#[test]
fn degraded_when_enabled_app_owner_reports_starting() -> TestResult {
    let root = fixture()?;
    app(root.path(), Some(AppProcessState::Starting))?;
    assert_state(root.path(), Spec031Availability::Degraded)
}

#[test]
fn blocked_when_enabled_app_owner_reports_failed() -> TestResult {
    let root = fixture()?;
    app(root.path(), Some(AppProcessState::Failed))?;
    assert_state(root.path(), Spec031Availability::Blocked)
}

#[test]
fn unavailable_when_enabled_app_has_no_lifecycle_evidence() -> TestResult {
    let root = fixture()?;
    app(root.path(), None)?;
    assert_state(root.path(), Spec031Availability::Unavailable)
}

#[test]
fn unavailable_when_app_lifecycle_lookup_fails() -> TestResult {
    let root = fixture()?;
    app(root.path(), Some(AppProcessState::Running))?;
    fs::write(
        root.path()
            .join("apps/app-supervisor/controlled/receipts/corrupt.json"),
        "{",
    )?;
    assert_state(root.path(), Spec031Availability::Unavailable)
}
