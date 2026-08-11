use shacs_api::ChatCompletionAdapter;
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{Config, ConfigBundle, ConfigContext};
use shacs_core::runtime::{
    ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler, ToolBeforeOrderKey,
    TrustedToolBeforeRegistry,
};
use shacs_projection::{HookRuntimeStatus, ResourceActivation, ResourceLoadStatus};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const PLUGIN_ID: &str = "user-trusted";

struct UserDataHandler;

impl ToolBeforeHandler for UserDataHandler {
    fn hook_ref(&self) -> &str {
        "user-trusted:tool:before"
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(PLUGIN_ID)
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(10)
    }

    fn evaluate(&self, _context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        ToolBeforeDecision::Allow
    }
}

#[test]
fn enabled_user_data_plugin_is_active_without_workspace_trust() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    fs::create_dir_all(&workspace)?;
    write_plugin(&data_dir)?;
    let mut registry = TrustedToolBeforeRegistry::new();
    registry.register(PLUGIN_ID, Arc::new(UserDataHandler));

    let adapter = AgentLoopChatCompletionAdapter::from_bundle_with_trusted_extensions(
        bundle(data_dir, workspace),
        false,
        registry,
    )?;
    let projection = adapter.trusted_runtime_projection();

    assert_eq!(projection.hooks().status, HookRuntimeStatus::Active);
    assert_eq!(projection.hooks().registered_handlers, 1);
    assert!(projection.resources().iter().any(|resource| {
        resource.resource_ref == format!("extension:{PLUGIN_ID}")
            && resource.activation == ResourceActivation::Explicit
            && resource.load_status == ResourceLoadStatus::Loaded
    }));
    assert!(projection.resources().iter().any(|resource| {
        resource.resource_ref == "skill:review"
            && resource.activation == ResourceActivation::Explicit
            && resource.load_status == ResourceLoadStatus::Loaded
    }));
    Ok(())
}

#[test]
fn inactive_user_data_plugin_does_not_publish_handler_or_loaded_resource(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    fs::create_dir_all(&workspace)?;
    write_plugin(&data_dir)?;
    let mut registry = TrustedToolBeforeRegistry::new();
    registry.register(PLUGIN_ID, Arc::new(UserDataHandler));

    let mut bundle = bundle(data_dir, workspace);
    bundle.config.plugins.enabled.clear();
    let projection = AgentLoopChatCompletionAdapter::from_bundle_with_trusted_extensions(
        bundle, false, registry,
    )?
    .trusted_runtime_projection();

    assert_eq!(projection.hooks().registered_handlers, 0);
    assert!(projection.resources().iter().all(|resource| {
        resource.resource_ref != format!("extension:{PLUGIN_ID}")
            || resource.activation == ResourceActivation::Inactive
            || resource.load_status != ResourceLoadStatus::Loaded
    }));
    Ok(())
}

fn bundle(data_dir: std::path::PathBuf, workspace: std::path::PathBuf) -> ConfigBundle {
    let mut config = Config::default();
    config.agents.defaults.provider = "custom".to_owned();
    config.agents.defaults.model = "test-model".to_owned();
    config.agents.defaults.workspace = workspace.to_string_lossy().into_owned();
    config.plugins.enabled.push(PLUGIN_ID.to_owned());
    ConfigBundle {
        config,
        context: ConfigContext {
            config_path: data_dir.join("config.json"),
            data_dir,
            workspace,
        },
        migrations: Vec::new(),
    }
}

fn write_plugin(data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let plugin = data_dir.join("plugins").join(PLUGIN_ID);
    fs::create_dir_all(plugin.join("skills/review"))?;
    fs::write(
        plugin.join("skills/review/SKILL.md"),
        "---\ndescription: review\n---\nreview",
    )?;
    fs::write(
        plugin.join("plugin.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "name": PLUGIN_ID,
            "version": "0.1.0",
            "surfaces": {"hooks": ["tool:before"]},
            "permissions": {},
            "entrypoints": {},
            "assets": []
        })
        .to_string(),
    )?;
    Ok(())
}
