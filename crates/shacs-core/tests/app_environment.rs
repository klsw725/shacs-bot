use serde_json::json;
use shacs_core::app::{
    AppBundlePath, AppError, AppId, AppLifecycleState, AppManifest, AppRegistryStore,
    TaskLedgerEntry,
};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn manifest_parse_validation_and_traversal_rejection() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bundle = write_bundle(root.path(), "demo.app", json!({}))?;

    let validated = AppManifest::load_from_bundle(&AppBundlePath::new(&bundle))?;
    assert_eq!(validated.manifest.id, AppId::parse("demo.app")?);
    assert_eq!(validated.resource_summaries.len(), 1);

    let unsafe_bundle = write_bundle(
        root.path(),
        "unsafe.app",
        json!({
            "skills": ["../outside.md"]
        }),
    )?;
    let error = AppManifest::load_from_bundle(&AppBundlePath::new(unsafe_bundle))
        .err()
        .ok_or("expected traversal validation failure")?;
    assert!(matches!(error, AppError::UnsafeBundlePath(path) if path == "../outside.md"));
    Ok(())
}

#[test]
fn id_collision_and_digest_mismatch_are_detected() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let store = AppRegistryStore::new(&data_dir);
    let bundle = write_bundle(root.path(), "collision.app", json!({}))?;
    let first = store.install_in_workspace(root.path(), &bundle)?;

    let collision_workspace = root.path().join("other");
    let collision_bundle = write_bundle(&collision_workspace, "collision.app", json!({}))?;
    let error = store
        .install_in_workspace(&collision_workspace, &collision_bundle)
        .err()
        .ok_or("expected id collision")?;
    assert!(matches!(error, AppError::AppIdCollision(app_id) if app_id == first.app_id));

    write_manifest(
        &bundle,
        "collision.app",
        json!({ "resources": ["asset.txt"] }),
    )?;
    fs::write(bundle.join("asset.txt"), "changed")?;
    let error = store
        .install_in_workspace(root.path(), &bundle)
        .err()
        .ok_or("expected digest mismatch")?;
    assert!(matches!(error, AppError::DigestMismatch(app_id) if app_id == first.app_id));
    Ok(())
}

#[test]
fn install_records_registry_without_runtime_side_effects() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let store = AppRegistryStore::new(&data_dir);
    let bundle = write_bundle(root.path(), "quiet.app", json!({}))?;

    let entry = store.install_in_workspace(root.path(), bundle)?;
    assert_eq!(entry.lifecycle_state, AppLifecycleState::Installed);
    assert!(entry.grant_reference.is_none());
    assert!(entry.process_snapshots.is_empty());
    assert!(store.registry_path().exists());
    assert!(!store.ledger_dir().exists());
    Ok(())
}

#[test]
fn install_in_workspace_rejects_outside_bundle_before_manifest_reading(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = root.path().join("outside").join("rogue.app.shacsapp");

    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&bundle)?;

    let error = store
        .install_in_workspace(&workspace, &bundle)
        .err()
        .ok_or("expected workspace rejection")?;
    let expected = workspace
        .canonicalize()?
        .join(".shacs/apps/rogue.app.shacsapp");
    let actual = bundle.canonicalize()?;
    assert!(
        matches!(error, AppError::InvalidBundleLocation { expected: actual_expected, actual: actual_path } if actual_expected == expected && actual_path == actual)
    );
    Ok(())
}

#[test]
fn uninstall_in_workspace_rejects_poisoned_bundle_path_at_core_level() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let workspace = root.path();
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = write_bundle(workspace, "poison.app", json!({}))?;
    let app_id = store.install_in_workspace(root.path(), &bundle)?.app_id;
    let outside = root.path().join("outside-delete-target");

    fs::create_dir_all(&outside)?;
    fs::write(outside.join("keep.txt"), "keep")?;

    let mut registry = store.load()?;
    let entry = registry
        .entries
        .get_mut(&app_id)
        .ok_or("expected installed entry")?;
    entry.bundle_path = outside.clone();
    store.save(&registry)?;

    let error = store
        .uninstall_in_workspace(workspace, &app_id)
        .err()
        .ok_or("expected poisoned registry rejection")?;
    let canonical_outside = outside.canonicalize()?;
    assert!(
        matches!(error, AppError::InvalidBundleLocation { actual, .. } if actual == canonical_outside)
    );
    assert!(outside.join("keep.txt").exists());
    Ok(())
}

#[test]
fn enable_disable_lifecycle_projection_does_not_create_process_truth() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = write_bundle(root.path(), "toggle.app", json!({}))?;
    let app_id = store.install_in_workspace(root.path(), bundle)?.app_id;

    let enabled = store.enable(&app_id)?;
    assert_eq!(enabled.lifecycle_state, AppLifecycleState::Enabled);
    assert!(enabled.process_snapshots.is_empty());

    let disabled = store.disable(&app_id)?;
    assert_eq!(disabled.lifecycle_state, AppLifecycleState::Disabled);
    assert!(disabled.process_snapshots.is_empty());
    Ok(())
}

#[test]
fn missing_secret_request_maps_to_unavailable_without_secret_value() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = write_bundle(
        root.path(),
        "secret.app",
        json!({
            "secrets": [{"key": "OPENAI_API_KEY", "required": true, "reason": "model access"}]
        }),
    )?;

    let entry = store.install_in_workspace(root.path(), bundle)?;
    assert_eq!(entry.lifecycle_state, AppLifecycleState::Unavailable);
    assert!(entry
        .grant_reference
        .as_deref()
        .is_some_and(|reference| { reference.starts_with("local-grant-request:secret.app:") }));
    assert_eq!(entry.secret_requests[0].key, "OPENAI_API_KEY");
    assert!(entry.unavailable_reasons[0].contains("OPENAI_API_KEY"));
    let encoded = serde_json::to_string(&entry)?;
    assert!(!encoded.contains("sk-secret-value"));
    Ok(())
}

#[test]
fn denied_permission_receipt_is_redacted_and_registry_truth_is_preserved(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = write_bundle(
        root.path(),
        "ledger.app",
        json!({
            "permissions": [{"id": "fs.read", "reason": "read selected files"}]
        }),
    )?;
    let entry = store.install_in_workspace(root.path(), bundle)?;
    assert!(entry
        .grant_reference
        .as_deref()
        .is_some_and(|reference| { reference.starts_with("local-grant-request:ledger.app:") }));
    let app_id = entry.app_id;

    let receipt = TaskLedgerEntry {
        receipt_id: "denied-1".to_owned(),
        app_id: app_id.clone(),
        process_id: None,
        decision: "permission denied".to_owned(),
        device_reference: None,
        port_reference: None,
        grant_reference: None,
        artifact_reference: Some("Authorization: Bearer ghp_secret_token".to_owned()),
        details: json!({"permission": "fs.read", "reason": "user denied"}),
    };
    let path = store.persist_ledger_entry(&receipt)?;
    let saved = fs::read_to_string(path)?;
    assert!(saved.contains("[REDACTED]"));
    assert!(!saved.contains("ghp_secret_token"));
    assert_eq!(
        store.inspect(&app_id)?.map(|entry| entry.app_id),
        Some(app_id)
    );

    let rejected = TaskLedgerEntry {
        receipt_id: "raw-secret".to_owned(),
        app_id: AppId::parse("ledger.app")?,
        process_id: None,
        decision: "denied".to_owned(),
        device_reference: None,
        port_reference: None,
        grant_reference: None,
        artifact_reference: None,
        details: json!({"api_key": "sk-secret-value"}),
    };
    let error = store
        .persist_ledger_entry(&rejected)
        .err()
        .ok_or("expected raw secret-looking field rejection")?;
    assert!(matches!(error, AppError::RawSecretField(key) if key == "api_key"));
    Ok(())
}

#[test]
fn uninstall_removes_registry_and_bundle_but_preserves_ledger_references(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = write_bundle(root.path(), "remove.app", json!({}))?;
    let app_id = store.install_in_workspace(root.path(), &bundle)?.app_id;
    let receipt = TaskLedgerEntry {
        receipt_id: "remove-1".to_owned(),
        app_id: app_id.clone(),
        process_id: None,
        decision: "completed".to_owned(),
        device_reference: None,
        port_reference: None,
        grant_reference: None,
        artifact_reference: Some("artifact://old".to_owned()),
        details: json!({"summary": "historical reference"}),
    };
    let ledger_path = store.persist_ledger_entry(&receipt)?;

    let uninstalling = store
        .mark_uninstalling(&app_id)?
        .ok_or("expected uninstalling transition")?;
    assert_eq!(
        uninstalling.lifecycle_state,
        AppLifecycleState::Uninstalling
    );
    assert_eq!(
        store.inspect(&app_id)?.map(|entry| entry.lifecycle_state),
        Some(AppLifecycleState::Uninstalling)
    );

    let removed = store
        .uninstall_in_workspace(root.path(), &app_id)?
        .ok_or("expected removed entry")?;
    assert_eq!(removed.app_id, app_id);
    assert!(!bundle.exists());
    assert!(store.inspect(&removed.app_id)?.is_none());
    assert!(ledger_path.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn bundle_reads_reject_symlink_escapes_before_digesting() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs as unix_fs;

    let root = tempfile::tempdir()?;
    let bundle = write_bundle(
        root.path(),
        "links.app",
        json!({ "resources": ["asset.txt"] }),
    )?;
    let outside = root.path().join("outside.txt");
    fs::write(&outside, "outside")?;
    fs::remove_file(bundle.join("asset.txt")).ok();
    unix_fs::symlink(&outside, bundle.join("asset.txt"))?;

    let error = AppManifest::load_from_bundle(&AppBundlePath::new(&bundle))
        .err()
        .ok_or("expected symlink escape rejection")?;
    assert!(matches!(error, AppError::UnsafeBundlePath(path) if path == "asset.txt"));

    let manifest = root.path().join("outside-manifest.json");
    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&json!({
            "id": "links.app",
            "version": "1.0.0",
            "entry": "entry.md"
        }))?,
    )?;
    fs::remove_file(bundle.join("manifest.json"))?;
    unix_fs::symlink(&manifest, bundle.join("manifest.json"))?;
    let error = AppManifest::load_from_bundle(&AppBundlePath::new(&bundle))
        .err()
        .ok_or("expected manifest symlink escape rejection")?;
    assert!(matches!(error, AppError::UnsafeBundlePath(path) if path == "manifest.json"));
    Ok(())
}

#[test]
fn manifest_required_field_omissions_fail() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for (app_id, field) in [
        ("missing-id.app", "id"),
        ("missing-version.app", "version"),
        ("missing-entry.app", "entry"),
    ] {
        let bundle = write_bundle(root.path(), app_id, json!({}))?;
        let mut manifest = json!({
            "id": app_id,
            "version": "1.0.0",
            "entry": "entry.md"
        });
        manifest
            .as_object_mut()
            .ok_or("expected manifest object")?
            .remove(field);
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;

        let error = AppManifest::load_from_bundle(&AppBundlePath::new(&bundle))
            .err()
            .ok_or("expected missing required field failure")?;
        assert!(matches!(error, AppError::Json(_)));
    }
    Ok(())
}

fn write_bundle(
    root: &Path,
    app_id: &str,
    overrides: serde_json::Value,
) -> Result<PathBuf, Box<dyn Error>> {
    let apps_dir = root.join(".shacs").join("apps");
    write_bundle_at(&apps_dir, app_id, overrides)
}

fn write_bundle_at(
    apps_dir: &Path,
    app_id: &str,
    overrides: serde_json::Value,
) -> Result<PathBuf, Box<dyn Error>> {
    let bundle = apps_dir.join(format!("{app_id}.shacsapp"));
    fs::create_dir_all(&bundle)?;
    fs::write(bundle.join("entry.md"), "# entry")?;
    write_manifest(&bundle, app_id, overrides)?;
    Ok(bundle)
}

fn write_manifest(
    bundle: &Path,
    app_id: &str,
    overrides: serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    let mut manifest = json!({
        "id": app_id,
        "version": "1.0.0",
        "entry": "entry.md"
    });
    merge_json(&mut manifest, overrides);
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn merge_json(target: &mut serde_json::Value, overrides: serde_json::Value) {
    if let (Some(target), Some(overrides)) = (target.as_object_mut(), overrides.as_object()) {
        for (key, value) in overrides {
            target.insert(key.clone(), value.clone());
        }
    }
}
