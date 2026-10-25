#[test]
fn enable_disable_lifecycle_projection_does_not_create_process_truth() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let store = AppRegistryStore::new(root.path().join("data"));
    let bundle = write_bundle(root.path(), "toggle.app", json!({}))?;
    let app_id = store.install_local_bundle(bundle)?.app_id;

    let enabled = store.enable(&app_id)?;
    assert_eq!(enabled.lifecycle_state, AppLifecycleState::Enabled);
    assert!(enabled.process_snapshots.is_empty());

    let disabled = store.disable(&app_id)?;
    assert_eq!(disabled.lifecycle_state, AppLifecycleState::Disabled);
    assert!(disabled.process_snapshots.is_empty());
    Ok(())
}
