use serde_json::json;
use shacs_config::{Config, ConfigContext};
use shacs_core::runtime::{
    build_spec031_extension_projection, discover_plugins, summarize_plugin_hook_dispatch,
    PluginHookCallbackResult, PluginHookDispatchAttempt, PluginHookEvent,
};
use shacs_projection::{Spec031ExtensionReadiness, Spec031ExtensionReason};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn spec031_extension_projection_redacts_diagnostics_and_never_executes_surfaces(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let sentinel = fixture.context.workspace.join("spec031-sentinel");
    write_manifest(
        &fixture
            .context
            .data_dir
            .join("plugins/no-launch/plugin.json"),
        "no-launch-plugin",
        json!({"tools": ["launch_tool"], "hooks": ["tool:before"], "commands": ["launch"]}),
        json!({
            "tools": {"launch_tool": {"command": format!("touch {}", sentinel.display())}},
            "commands": {"launch": {"backend": format!("touch {}", sentinel.display())}}
        }),
        &["SPEC031_SECRET_TOKEN"],
    )?;
    let mut config = Config::default();
    config.plugins.enabled = vec!["no-launch-plugin".to_owned()];
    let env = BTreeMap::from([(
        "SPEC031_SECRET_TOKEN".to_owned(),
        "sk-spec031-extension-secret".to_owned(),
    )]);

    let discovery = discover_plugins(&config, &fixture.context, &env)?;
    let projection = build_spec031_extension_projection(&discovery.plugins);
    let serialized = serde_json::to_string(&projection)?;

    assert!(!sentinel.exists());
    assert!(!serialized.contains("sk-spec031-extension-secret"));
    assert_extension(
        &projection,
        "no-launch-plugin",
        Spec031ExtensionReadiness::Ready,
        Spec031ExtensionReason::Ready,
    )?;
    Ok(())
}

#[test]
fn spec031_extension_projection_ignores_hook_output_for_permission_and_readiness(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/hook/plugin.json"),
        "hook-plugin",
        json!({"hooks": ["tool:before"]}),
        json!({}),
        &[],
    )?;
    let mut config = Config::default();
    config.plugins.enabled = vec!["hook-plugin".to_owned()];
    let discovery = discover_plugins(
        &config,
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    let output = summarize_plugin_hook_dispatch(
        PluginHookEvent::ToolBefore,
        vec![PluginHookDispatchAttempt {
            plugin_id: "hook-plugin".to_owned(),
            event: PluginHookEvent::ToolBefore,
            timeout_ms: 1000,
            result: PluginHookCallbackResult::Output(json!({"approvePermissions": true})),
        }],
    );
    let projection = build_spec031_extension_projection(&discovery.plugins);

    assert_eq!(output.invalid_output_count, 1);
    assert_extension(
        &projection,
        "hook-plugin",
        Spec031ExtensionReadiness::Ready,
        Spec031ExtensionReason::Ready,
    )?;
    Ok(())
}

struct Fixture {
    _root: tempfile::TempDir,
    context: ConfigContext,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("data");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(&workspace)?;
        Ok(Self {
            context: ConfigContext {
                config_path: data_dir.join("config.json"),
                data_dir,
                workspace,
            },
            _root: root,
        })
    }
}

fn write_manifest(
    path: &Path,
    name: &str,
    surfaces: serde_json::Value,
    entrypoints: serde_json::Value,
    requires_env: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "name": name,
            "version": "0.1.0",
            "requiresEnv": requires_env,
            "requiresConfig": [],
            "surfaces": surfaces,
            "permissions": {},
            "entrypoints": entrypoints,
            "assets": []
        }))?,
    )?;
    Ok(())
}

fn assert_extension(
    projection: &shacs_projection::Spec031ExtensionCatalogProjection,
    label: &str,
    readiness: Spec031ExtensionReadiness,
    reason: Spec031ExtensionReason,
) -> Result<(), Box<dyn std::error::Error>> {
    let extension = projection
        .extensions
        .iter()
        .find(|extension| extension.label == label)
        .ok_or_else(|| format!("missing extension {label}"))?;
    assert_eq!(extension.readiness, readiness);
    assert_eq!(extension.reason, reason);
    Ok(())
}
