use serde_json::json;
use shacs_app::app::{AppLifecycleState, AppRegistryStore};
use shacs_app::app_authoring::{AppAuthoringInitOutcome, AppAuthoringStore};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn app_authoring_init_is_draft_only_and_does_not_install_registry_entry(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let authoring = AppAuthoringStore::new(&data_dir);
    let registry = AppRegistryStore::new(&data_dir);

    let report = authoring.init_app("spec030.clock")?;
    let entries = registry.list()?;

    assert_eq!(report.outcome, AppAuthoringInitOutcome::Created);
    assert_eq!(report.app_id.as_str(), "spec030.clock");
    assert!(report.manifest_candidate_path.exists());
    assert!(report.readme_candidate_path.exists());
    assert!(report.validation_status.contains("no install"));
    assert!(entries.is_empty());
    assert!(!data_dir.join("apps/registry.json").exists());
    Ok(())
}

#[test]
fn app_registry_list_and_inspect_project_installed_state_without_process_truth(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let store = AppRegistryStore::new(&data_dir);
    let bundle = write_bundle(&data_dir, "spec030.clock", json!({}))?;

    let installed = store.install_local_bundle(bundle)?;
    let listed = store.list()?;
    let inspected = store
        .inspect(&installed.app_id)?
        .ok_or("expected installed app")?;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].app_id, installed.app_id);
    assert_eq!(inspected.lifecycle_state, AppLifecycleState::Installed);
    assert!(inspected.process_snapshots.is_empty());
    assert!(inspected.grant_reference.is_none());
    assert!(!store.ledger_dir().exists());
    Ok(())
}

fn write_bundle(
    data_dir: &Path,
    app_id: &str,
    overrides: serde_json::Value,
) -> Result<PathBuf, Box<dyn Error>> {
    let bundle = data_dir.join("apps").join(format!("{app_id}.shacsapp"));
    fs::create_dir_all(&bundle)?;
    fs::write(bundle.join("README.md"), "# spec030 clock")?;
    let mut manifest = json!({
        "id": app_id,
        "version": "0.1.0",
        "entry": "README.md"
    });
    if let (Some(target), Some(source)) = (manifest.as_object_mut(), overrides.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(bundle)
}
