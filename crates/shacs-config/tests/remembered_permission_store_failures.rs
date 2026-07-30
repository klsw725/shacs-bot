use shacs_config::{RememberedPermissionFileStore, RememberedPermissionStoreErrorKind};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;

#[path = "remembered_permission_store_support/mod.rs"]
mod support;

use support::{exec_rule, StoreFixture};

#[test]
fn remembered_permission_store_rejects_symlink_regular_file_and_malformed_inputs(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);
    fs::write(fixture.data_dir.join("target.json"), "{}")?;
    #[cfg(unix)]
    {
        symlink(fixture.data_dir.join("target.json"), store.path())?;
        let error = store.load().expect_err("symlink must be rejected");
        assert_eq!(error.kind(), RememberedPermissionStoreErrorKind::Symlink);
        assert!(!error
            .to_string()
            .contains(fixture.data_dir.to_string_lossy().as_ref()));
        fs::remove_file(store.path())?;
    }

    fs::create_dir(store.path())?;
    let error = store.load().expect_err("directory must be rejected");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::NotRegularFile
    );
    fs::remove_dir(store.path())?;

    fs::write(store.path(), r#"{"schemaVersion":2,"projects":{}}"#)?;
    let before = fs::read(store.path())?;
    let error = store
        .mutate(|permissions| {
            permissions.upsert_rule(fixture.workspace_id.clone(), exec_rule("cargo", 1));
            Ok(())
        })
        .expect_err("unknown schema must fail closed");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::UnknownSchemaVersion
    );
    assert_eq!(fs::read(store.path())?, before);

    fs::write(
        store.path(),
        r#"{"schemaVersion":1,"rawArguments":"--secret-sentinel","projects":{}}"#,
    )?;
    let error = store.load().expect_err("raw argument must be rejected");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::ForbiddenRawField
    );
    assert!(!error.to_string().contains("--secret-sentinel"));
    Ok(())
}

#[test]
fn remembered_permission_store_rejects_oversized_file_without_modifying_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);
    let oversized = vec![b' '; 1_048_577];
    fs::write(store.path(), &oversized)?;

    let error = store
        .mutate(|permissions| {
            permissions.upsert_rule(fixture.workspace_id.clone(), exec_rule("cargo", 1));
            Ok(())
        })
        .expect_err("oversized store must fail closed");

    assert_eq!(error.kind(), RememberedPermissionStoreErrorKind::Oversized);
    assert_eq!(fs::read(store.path())?, oversized);
    Ok(())
}
