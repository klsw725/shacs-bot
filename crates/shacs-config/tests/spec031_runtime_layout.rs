use shacs_config::{
    config_context, ensure_runtime_dirs, runtime_layout, RuntimeLayoutEntryKind, RuntimeLayoutOwner,
};

#[test]
fn canonical_runtime_layout_matrix_drives_required_directory_creation(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let context = config_context(
        Some(root.path().join("instance/config.json")),
        Some(root.path().join("workspace")),
    );

    // When
    let created = ensure_runtime_dirs(&context)?;

    // Then
    let matrix = runtime_layout(&context);
    let names = matrix
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "config",
            "auth",
            "sessions",
            "media",
            "logs",
            "channels",
            "skills",
            "cache",
            "tmp",
            "snapshots",
        ]
    );
    assert!(matrix.iter().all(|entry| match entry.kind {
        RuntimeLayoutEntryKind::File => !created.contains(&entry.path),
        RuntimeLayoutEntryKind::Directory => {
            created.contains(&entry.path) && entry.path.is_dir()
        }
    }));
    assert_eq!(matrix[0].owner, RuntimeLayoutOwner::UserConfig);
    assert_eq!(matrix[1].owner, RuntimeLayoutOwner::CredentialStore);
    assert_eq!(matrix[2].owner, RuntimeLayoutOwner::SessionStore);
    assert_eq!(matrix[6].owner, RuntimeLayoutOwner::UserSkills);
    assert!(matrix[3..6]
        .iter()
        .chain(matrix[7..].iter())
        .all(|entry| entry.owner == RuntimeLayoutOwner::RuntimeProcess));
    assert!(matrix.iter().all(|entry| match entry.owner {
        RuntimeLayoutOwner::RuntimeProcess =>
            entry.marker_path.is_some()
                && entry.mutation == shacs_config::RuntimeLayoutMutation::OwnerAdmitted,
        RuntimeLayoutOwner::UserConfig
        | RuntimeLayoutOwner::CredentialStore
        | RuntimeLayoutOwner::SessionStore
        | RuntimeLayoutOwner::UserSkills => entry.marker_path.is_none(),
    }));
    assert!(matrix
        .iter()
        .all(|entry| entry.cleanup == shacs_config::RuntimeLayoutCleanup::Preserve));
    Ok(())
}
