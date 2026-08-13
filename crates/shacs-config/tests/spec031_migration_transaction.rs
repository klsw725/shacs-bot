use serde_json::{json, Value};
use shacs_config::{
    apply_config_migration, begin_config_migration_apply, dry_run_config_migration,
    load_config_with_env, recover_config_migration, ConfigError, ConfigMigrationAction,
    LoadOptions,
};
use std::collections::BTreeMap;
use std::fs;

#[test]
fn spec031_no_op_dry_run_and_apply_do_not_write_current_json(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let original = "{\n  \"schemaVersion\": 1,\n  \"providers\": {}\n}\n";
    fs::write(&path, original)?;

    let plan = dry_run_config_migration(&path)?;
    let evidence = apply_config_migration(&path)?;

    assert!(!plan.changed);
    assert_eq!(evidence.action, ConfigMigrationAction::NoOp);
    assert_eq!(fs::read_to_string(path)?, original);
    Ok(())
}

#[test]
fn spec031_apply_preserves_placeholders_locators_profiles_and_workspace_override(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "agents": {"defaults": {"workspace": "~/persisted", "sessionTtlMinutes": 5}},
            "providers": {"openrouter": {
                "apiKey": "${OPENROUTER_API_KEY}",
                "credentialSource": {"schemaVersion": 1, "environment": "OPENROUTER_API_KEY"}
            }},
            "profiles": {
                "providers": {"daily": {"provider": "openrouter"}},
                "selection": {"provider": "daily"}
            }
        }))?,
    )?;

    let evidence = apply_config_migration(&path)?;
    let saved: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(path),
            workspace_override: Some(root.path().join("runtime-workspace")),
            resolve_env: true,
            write_back_migrations: false,
        },
        &BTreeMap::from([(
            "OPENROUTER_API_KEY".to_owned(),
            "resolved-secret".to_owned(),
        )]),
    )?;

    assert_eq!(evidence.action, ConfigMigrationAction::Applied);
    assert_eq!(
        saved["providers"]["openrouter"]["apiKey"],
        "${OPENROUTER_API_KEY}"
    );
    assert_eq!(
        saved["providers"]["openrouter"]["credentialSource"]["environment"],
        "OPENROUTER_API_KEY"
    );
    assert_eq!(saved["profiles"]["selection"]["provider"], "daily");
    assert_eq!(saved["agents"]["defaults"]["workspace"], "~/persisted");
    assert_eq!(
        bundle.context.workspace,
        root.path().join("runtime-workspace")
    );
    assert_eq!(
        bundle.config.providers["openrouter"].api_key.as_deref(),
        Some("resolved-secret")
    );
    let profiles = bundle.config.resolve_profiles()?;
    assert_eq!(profiles.provider.expect("selected provider").name, "daily");
    assert_eq!(
        profiles.source,
        shacs_config::ProfileSelectionSource::Configured
    );
    Ok(())
}

#[test]
fn spec031_interrupted_apply_blocks_mutation_and_recover_restores_original_with_evidence(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let original = serde_json::to_vec_pretty(&json!({
        "agents": {"defaults": {"sessionTtlMinutes": 11}}
    }))?;
    fs::write(&path, &original)?;

    let pending = begin_config_migration_apply(&path)?;
    drop(pending);
    let marker: Value = serde_json::from_slice(&fs::read(
        path.with_extension("json.migration-in-progress"),
    )?)?;
    let blocked = apply_config_migration(&path).expect_err("interrupted marker blocks apply");
    let recovered = recover_config_migration(&path)?;

    assert!(matches!(blocked, ConfigError::MigrationInterrupted { .. }));
    assert_eq!(marker["schemaVersion"], 1);
    assert!(marker["originalDigest"].as_str().is_some());
    assert!(marker["resultDigest"].as_str().is_some());
    assert!(marker.get("rawConfig").is_none());
    assert_eq!(recovered.action, ConfigMigrationAction::Recovered);
    assert!(!recovered.rollback_performed);
    assert_eq!(
        recovered.file_state,
        shacs_config::ConfigMigrationFileState::Original
    );
    assert_eq!(fs::read(&path)?, original);
    assert_eq!(
        apply_config_migration(&path)?.action,
        ConfigMigrationAction::Applied
    );
    Ok(())
}

#[test]
fn spec031_future_schema_is_rejected_without_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let original = b"{\"schemaVersion\":2,\"providers\":{}}\n";
    fs::write(&path, original)?;

    let error = dry_run_config_migration(&path).expect_err("future schema is unsupported");
    let load_error = load_config_with_env(
        LoadOptions {
            config_path: Some(path.clone()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: true,
        },
        &BTreeMap::<String, String>::new(),
    )
    .expect_err("future schema does not load");

    assert!(matches!(
        error,
        ConfigError::UnsupportedSchema {
            found: 2,
            current: 1
        }
    ));
    assert!(matches!(
        load_error,
        ConfigError::UnsupportedSchema {
            found: 2,
            current: 1
        }
    ));
    assert_eq!(fs::read(path)?, original);
    Ok(())
}
