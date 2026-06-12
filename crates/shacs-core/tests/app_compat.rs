use shacs_core::app::{AppId, AppRegistryStore};
use shacs_core::app_authoring::{AppAuthoringStore, AppIdCandidate};
use std::error::Error;

#[test]
fn app_compat_public_paths_still_work() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");

    let app_id = AppId::parse("compat.app")?;
    assert_eq!(app_id.as_str(), "compat.app");

    let registry = AppRegistryStore::new(&data_dir).load()?;
    assert!(registry.entries.is_empty());

    let candidate = AppIdCandidate::parse("compat.app")?;
    assert_eq!(candidate.app_id(), &app_id);
    assert!(AppAuthoringStore::new(&data_dir)
        .authoring_apps_dir()
        .ends_with("authoring/apps"));

    Ok(())
}
