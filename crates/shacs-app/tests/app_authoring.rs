use serde_json::{json, Value};
use shacs_app::app::{AppId, AppRegistryStore};
use shacs_app::app_authoring::{
    AppAuthoringError, AppAuthoringInitOutcome, AppAuthoringStore, AppIdCandidate,
};
use std::error::Error;
use std::fs;
use std::path::Path;

#[test]
fn apps_init_rejects_invalid_app_id() -> Result<(), Box<dyn Error>> {
    for value in [
        "",
        ".",
        "..",
        "demo/app",
        "demo\\app",
        "demo app",
        "demo\napp",
        "Demo",
        "demo$app",
        "demo＊app",
    ] {
        assert!(
            AppIdCandidate::parse(value).is_err(),
            "expected `{value}` to be invalid"
        );
    }

    for value in ["demo.app", "demo_app", "demo-1"] {
        let candidate = AppIdCandidate::parse(value)?;
        assert_eq!(candidate.app_id().as_str(), value);
    }
    Ok(())
}

#[test]
fn apps_init_creates_draft_under_authoring_store() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let report = AppAuthoringStore::new(&data_dir).init_app("demo.app")?;

    assert_eq!(report.outcome, AppAuthoringInitOutcome::Created);
    assert_eq!(report.app_id, AppId::parse("demo.app")?);
    assert_eq!(report.draft_id.as_str(), "draft-demo.app");
    let authoring_apps_dir = data_dir.join("authoring").join("apps").canonicalize()?;
    assert!(report.draft_path.starts_with(authoring_apps_dir));
    assert!(!data_dir.join("apps/demo.app.shacsapp").exists());
    Ok(())
}

#[test]
fn apps_init_writes_minimal_candidates() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let report = AppAuthoringStore::new(&data_dir).init_app("demo_app")?;

    assert!(report.draft_metadata_path.exists());
    assert!(report.scaffold_plan_path.exists());
    assert!(report.manifest_candidate_path.exists());
    assert!(report.readme_candidate_path.exists());
    assert!(!report
        .draft_path
        .join("candidates/skills/SKILL.md")
        .exists());
    assert!(!data_dir.join("skills").exists());

    let manifest: Value = serde_json::from_slice(&fs::read(&report.manifest_candidate_path)?)?;
    assert_eq!(manifest["id"], json!("demo_app"));
    assert_eq!(manifest["version"], json!("0.1.0"));
    assert_eq!(manifest["entry"], json!("README.md"));
    assert!(fs::read_to_string(&report.readme_candidate_path)?.contains("No process"));
    assert!(report.current_revision_digest.starts_with("sha256:"));
    Ok(())
}

#[test]
fn apps_init_existing_same_content_is_idempotent() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let store = AppAuthoringStore::new(&data_dir);
    let first = store.init_app("demo.app")?;
    let second = store.init_app("demo.app")?;

    assert_eq!(
        second.outcome,
        AppAuthoringInitOutcome::AlreadyExistsSameContent
    );
    assert_eq!(first.draft_path, second.draft_path);
    assert_eq!(
        first.current_revision_digest,
        second.current_revision_digest
    );
    Ok(())
}

#[test]
fn apps_init_existing_different_content_conflicts() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let store = AppAuthoringStore::new(&data_dir);
    let first = store.init_app("demo.app")?;
    fs::write(&first.readme_candidate_path, "changed")?;

    let error = store
        .init_app("demo.app")
        .err()
        .ok_or("expected draft conflict")?;
    assert!(matches!(error, AppAuthoringError::Conflict(path) if path == first.draft_path));
    Ok(())
}

#[test]
fn apps_init_existing_different_metadata_conflicts() -> Result<(), Box<dyn Error>> {
    for metadata_file in ["draft.json", "scaffold-plan.json"] {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("data");
        let store = AppAuthoringStore::new(&data_dir);
        let first = store.init_app("demo.app")?;
        fs::write(first.draft_path.join(metadata_file), "{}\n")?;

        let error = store
            .init_app("demo.app")
            .err()
            .ok_or("expected draft metadata conflict")?;
        assert!(
            matches!(error, AppAuthoringError::Conflict(path) if path == first.draft_path),
            "expected {metadata_file} change to conflict"
        );
    }
    Ok(())
}

#[test]
fn apps_init_blocks_installed_app_id_without_registry_mutation() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let store = AppRegistryStore::new(&data_dir);
    let bundle = write_bundle(root.path(), "demo.app")?;
    let installed = store.install_local_bundle(&bundle)?;
    let registry_path = store.registry_path();
    let before = fs::read(&registry_path)?;

    let error = AppAuthoringStore::new(&data_dir)
        .init_app("demo.app")
        .err()
        .ok_or("expected installed app blocker")?;
    assert!(matches!(error, AppAuthoringError::InstalledApp(app_id) if app_id == installed.app_id));
    assert_eq!(fs::read(&registry_path)?, before);
    assert!(!data_dir.join("authoring/apps/draft-demo.app").exists());
    Ok(())
}

#[test]
fn apps_init_does_not_mutate_app_registry() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let registry_path = AppRegistryStore::new(&data_dir).registry_path();

    AppAuthoringStore::new(&data_dir).init_app("demo.app")?;

    assert!(!registry_path.exists());
    assert!(!data_dir.join("apps/demo.app.shacsapp").exists());
    assert!(!data_dir.join("runtime").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn apps_init_rejects_authoring_symlink_escape() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs as unix_fs;

    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    let outside = root.path().join("outside");
    fs::create_dir_all(&outside)?;
    fs::create_dir_all(&data_dir)?;
    unix_fs::symlink(&outside, data_dir.join("authoring"))?;

    let error = AppAuthoringStore::new(&data_dir)
        .init_app("demo.app")
        .err()
        .ok_or("expected unsafe authoring path")?;
    assert!(matches!(error, AppAuthoringError::UnsafePath { .. }));
    assert!(!outside.join("apps/draft-demo.app").exists());
    Ok(())
}

fn write_bundle(root: &Path, app_id: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let bundle = root
        .join("data")
        .join("apps")
        .join(format!("{app_id}.shacsapp"));
    fs::create_dir_all(&bundle)?;
    fs::write(bundle.join("entry.md"), "# entry")?;
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "id": app_id,
            "version": "1.0.0",
            "entry": "entry.md"
        }))?,
    )?;
    Ok(bundle)
}
