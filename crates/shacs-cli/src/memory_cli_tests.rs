use super::super::{
    production_tool_registry, provider_credential_runtime, spec030_fact_store_for_bundle,
};
use super::{run, MemoryOptions};
use crate::CliError;
use serde_json::{json, Value};
use shacs_config::{Config, ConfigBundle, ConfigContext, ConfigError};
use std::error::Error;
use std::fs;
use std::path::Path;

fn bundle(root: &Path, config: Config) -> Result<ConfigBundle, Box<dyn Error>> {
    let data_dir = root.join("data");
    let workspace = root.join("workspace");
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(workspace.join("memory"))?;
    fs::write(
        workspace.join("memory").join("MEMORY.md"),
        "- 치과 예약은 화요일 오후 3시\n- 점심 메뉴\n",
    )?;
    let config_path = data_dir.join("config.json");
    fs::write(&config_path, serde_json::to_string(&config)?)?;
    Ok(ConfigBundle {
        config,
        context: ConfigContext {
            config_path,
            data_dir,
            workspace,
        },
        migrations: Vec::new(),
    })
}

#[test]
fn the_memory_search_tool_and_cli_give_the_same_result() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), Config::default())?;
    let credential_runtime =
        provider_credential_runtime(&bundle, spec030_fact_store_for_bundle(&bundle));

    // When
    let tool = production_tool_registry(&bundle, false, &credential_runtime)?
        .registry
        .execute("memory_search", json!({"query": "치과 예약", "limit": 3}))
        .into_text();
    let cli = run(MemoryOptions::Search {
        config_path: Some(bundle.context.config_path.clone()),
        workspace_override: Some(bundle.context.workspace.clone()),
        query: "치과 예약".to_owned(),
        limit: 3,
        json: true,
    })?;

    // Then
    assert_eq!(cli, tool);
    let result: Value = serde_json::from_str(&cli)?;
    assert_eq!(
        result["results"][0]["text"],
        "- 치과 예약은 화요일 오후 3시"
    );
    assert_eq!(result["semantic"]["reason"], "model_missing");
    Ok(())
}

#[test]
fn every_production_tool_list_has_memory_search_unless_its_settings_are_invalid(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let valid = bundle(root.path(), Config::default())?;
    let mut small_limit = valid.clone();
    small_limit.config.agents.defaults.max_tool_result_chars = 999;
    let mut unknown_model = valid.clone();
    unknown_model.config.agents.defaults.memory_search.model = "someone/else".to_owned();
    let credential_runtime =
        provider_credential_runtime(&valid, spec030_fact_store_for_bundle(&valid));

    // When / Then
    for side_effects in [false, true] {
        let tooling = production_tool_registry(&valid, side_effects, &credential_runtime)?;
        assert!(tooling.registry.has("memory_search"), "{side_effects}");
    }
    for (refused, expected) in [
        (&small_limit, "maxToolResultChars"),
        (&unknown_model, "dragonkue/multilingual-e5-small-ko-v2"),
    ] {
        let failure = production_tool_registry(refused, true, &credential_runtime).err();
        assert!(
            matches!(&failure, Some(CliError::Config(ConfigError::Invalid(reason))) if reason.contains(expected)),
            "{expected}"
        );
    }
    Ok(())
}
