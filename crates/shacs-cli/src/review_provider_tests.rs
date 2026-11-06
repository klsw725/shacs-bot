use super::*;
use std::error::Error;
use std::fs;

#[test]
fn building_the_provider_writes_nothing_even_for_a_config_that_needs_migrating(
) -> Result<(), Box<dyn Error>> {
    // Given: a config without `schemaVersion`, which loading migrates.
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let written = r#"{"agents": {"defaults": {"provider": "custom", "model": "m"}},
 "providers": {"custom": {"apiKey": "sk-isolated", "apiBase": "http://127.0.0.1:1/v1"}},
 "permissions": {"deny": ["exec(rm *)"], "autoMode": {"classifierModel": "${SPEC048_REVIEW_MODEL}"}}}"#;
    fs::write(&config_path, written)?;
    fs::write(
        root.path().join(".env.json"),
        r#"{"SPEC048_REVIEW_MODEL": "small-review-model"}"#,
    )?;

    // When
    let provider = ReviewProvider::for_config(Some(config_path.clone()), None)?;

    // Then
    assert_eq!(provider.model, "small-review-model");
    assert_eq!(fs::read_to_string(&config_path)?, written);
    Ok(())
}

#[test]
fn an_expired_oauth_sign_in_is_refused_without_a_lock_or_a_refresh() -> Result<(), Box<dyn Error>> {
    // Given: a Codex sign-in that expired long ago.
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        r#"{"agents": {"defaults": {"provider": "openai_codex", "model": "gpt-5.4"}}}"#,
    )?;
    let auth = r#"{"openai_codex": {"type": "oauth", "access": "old", "refresh": "refresh-1", "expires": 1}}"#;
    fs::write(root.path().join("auth.json"), auth)?;
    let before = folder_names(root.path())?;

    // When
    let provider = ReviewProvider::for_config(Some(config_path), None)?;
    let answered = provider.answer("system", "user".to_owned(), 16);

    // Then
    assert!(answered.is_err(), "{answered:?}");
    assert_eq!(fs::read_to_string(root.path().join("auth.json"))?, auth);
    assert_eq!(folder_names(root.path())?, before);
    Ok(())
}

fn folder_names(folder: &std::path::Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    Ok(names)
}
