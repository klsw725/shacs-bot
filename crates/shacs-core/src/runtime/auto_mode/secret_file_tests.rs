use super::*;
use std::error::Error;

#[test]
fn auth_store_files_are_secret_under_any_spelling_symlink_or_hard_link(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data = fs::canonicalize(root.path())?.join("data");
    fs::create_dir_all(data.join("auth"))?;
    fs::write(data.join("auth.json"), "{}")?;
    fs::write(data.join("auth/codex.json"), "{}")?;
    fs::write(data.join(".auth.json.tmp-7"), "{}")?;
    fs::write(data.join("config.json"), "{}")?;
    let workspace = data.with_file_name("ws");
    fs::create_dir(&workspace)?;
    std::os::unix::fs::symlink(data.join("auth.json"), workspace.join("linked.json"))?;
    fs::hard_link(data.join("auth/codex.json"), workspace.join("copy.json"))?;

    // When
    let secrets = SecretFiles::new(&data, &data.join("config.json"));

    // Then
    for secret in [
        data.join("auth.json"),
        data.join("auth/codex.json"),
        data.join(".auth.json.tmp-7"),
        data.join("auth/new.json"),
        data.join("sub/../auth.json"),
        workspace.join("linked.json"),
        workspace.join("copy.json"),
    ] {
        assert!(secrets.contains(&secret), "{}", secret.display());
    }
    assert!(!secrets.contains(&data.join("config.json")));
    assert!(!secrets.contains(&workspace.join("auth.json")));
    Ok(())
}

#[test]
fn secret_names_include_absolute_relative_and_dotted_names() {
    // Given
    let secrets = SecretFiles::new(
        Path::new("/nonexistent/data"),
        Path::new("/nonexistent/data/config.json"),
    );

    // When
    let names = secrets.names(&[PathBuf::from("/nonexistent")]);

    // Then
    for expected in [
        "auth.json",
        ".auth.json.tmp-",
        "/nonexistent/data/auth.json",
        "/nonexistent/data/auth",
        "data/auth.json",
        "data/auth",
    ] {
        assert!(
            names.contains(&expected.to_owned()),
            "{expected} in {names:?}"
        );
    }
}

#[test]
fn the_env_file_migration_backups_and_secrets_marker_are_secret() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data = fs::canonicalize(root.path())?.join("data");
    let workspace = data.with_file_name("ws");
    fs::create_dir_all(&data)?;
    fs::create_dir(&workspace)?;
    let secret_names = [
        ".env.json",
        "..env.json.tmp-12-34",
        "settings.json.migration-backup",
        "settings.json.secrets-migration-in-progress",
        "settings.json.secrets-migration-backup",
        "settings.json.secrets-migration-env-backup",
        ".settings.json.secrets-migration-backup.tmp-5",
    ];
    for name in secret_names {
        fs::write(data.join(name), "{}")?;
    }
    fs::write(data.join("settings.json"), "{}")?;
    fs::write(data.join("settings.json.migration-in-progress"), "{}")?;
    fs::hard_link(data.join(".env.json"), workspace.join("copied-env.json"))?;

    // When
    let secrets = SecretFiles::new(&data, &data.join("settings.json"));
    let names = secrets.names(std::slice::from_ref(&data));

    // Then
    for name in secret_names {
        assert!(secrets.contains(&data.join(name)), "{name}");
    }
    assert!(secrets.contains(&workspace.join("copied-env.json")));
    assert!(!secrets.contains(&data.join("settings.json")));
    assert!(!secrets.contains(&data.join("settings.json.migration-in-progress")));
    assert!(!secrets.contains(&workspace.join(".env.json")));
    for expected in [".env.json", "settings.json.secrets-migration-backup"] {
        assert!(
            names.contains(&expected.to_owned()),
            "{expected} in {names:?}"
        );
    }
    assert_eq!(secrets.display_name(&data.join(".env.json")), ".env.json");
    Ok(())
}

#[test]
fn hard_links_are_checked_against_the_store_as_it_is_now() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let data = fs::canonicalize(root.path())?.join("data");
    let workspace = data.with_file_name("ws");
    fs::create_dir_all(data.join("auth"))?;
    fs::create_dir(&workspace)?;
    fs::write(data.join("auth.json"), "{}")?;
    let secrets = SecretFiles::new(&data, &data.join("config.json"));
    fs::write(data.join(".auth.json.tmp-1"), "{\"new\": true}")?;
    fs::rename(data.join(".auth.json.tmp-1"), data.join("auth.json"))?;
    fs::hard_link(data.join("auth.json"), workspace.join("replaced.json"))?;
    let deep = (0..10).fold(data.join("auth"), |folder, level| {
        folder.join(format!("d{level}"))
    });
    fs::create_dir_all(&deep)?;
    fs::write(deep.join("token.json"), "{}")?;
    fs::hard_link(deep.join("token.json"), workspace.join("deep.json"))?;
    for index in 0..300 {
        fs::write(data.join("auth").join(format!("f{index:03}.json")), "{}")?;
    }
    fs::hard_link(data.join("auth/f299.json"), workspace.join("late.json"))?;
    fs::write(workspace.join("own.txt"), "mine")?;
    fs::hard_link(workspace.join("own.txt"), workspace.join("own-link.txt"))?;

    // When / Then
    for linked in ["replaced.json", "deep.json", "late.json"] {
        assert!(secrets.contains(&workspace.join(linked)), "{linked}");
    }
    assert!(!secrets.contains(&workspace.join("own-link.txt")));
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_unreadable_store_makes_every_linked_file_secret() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    // Given
    let root = tempfile::tempdir()?;
    let data = fs::canonicalize(root.path())?.join("data");
    let workspace = data.with_file_name("ws");
    fs::create_dir_all(data.join("auth"))?;
    fs::create_dir(&workspace)?;
    fs::write(workspace.join("own.txt"), "mine")?;
    fs::hard_link(workspace.join("own.txt"), workspace.join("own-link.txt"))?;
    fs::write(workspace.join("single.txt"), "mine")?;
    fs::set_permissions(data.join("auth"), fs::Permissions::from_mode(0o000))?;
    let secrets = SecretFiles::new(&data, &data.join("config.json"));
    let readable = fs::read_dir(data.join("auth")).is_ok();

    // When
    let linked = secrets.contains(&workspace.join("own-link.txt"));
    let single = secrets.contains(&workspace.join("single.txt"));
    fs::set_permissions(data.join("auth"), fs::Permissions::from_mode(0o700))?;

    // Then
    assert!(linked || readable);
    assert!(!single);
    Ok(())
}
