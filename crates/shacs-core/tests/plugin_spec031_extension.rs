use serde_json::json;
use shacs_config::{Config, ConfigContext};
use shacs_core::runtime::{build_spec031_extension_projection, discover_plugins};
use shacs_projection::{Spec031ExtensionReadiness, Spec031ExtensionReason};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn spec031_extension_projection_uses_discovered_owner_records_for_all_readiness_states(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/ready/plugin.json"),
        "ready-plugin",
        json!({"tools": ["ready_tool"], "hooks": ["tool:before"]}),
        json!({"tools": {"ready_tool": {"command": "touch SHOULD_NOT_RUN"}}}),
        &[],
    )?;
    write_manifest(
        &fixture
            .context
            .data_dir
            .join("plugins/degraded/plugin.json"),
        "degraded-plugin",
        json!({"commands": ["status"]}),
        json!({"commands": {"status": {"backend": "exec"}}}),
        &[],
    )?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/blocked/plugin.json"),
        "blocked-plugin",
        json!({"hooks": ["llm:before"]}),
        json!({}),
        &["SPEC031_MISSING_TOKEN"],
    )?;
    write_manifest(
        &fixture
            .context
            .data_dir
            .join("plugins/disabled/plugin.json"),
        "disabled-plugin",
        json!({"tools": ["disabled_tool"]}),
        json!({}),
        &[],
    )?;
    write_manifest(
        &fixture
            .context
            .data_dir
            .join("plugins/unavailable/plugin.json"),
        "unavailable-plugin",
        json!({"skills": ["note"]}),
        json!({}),
        &[],
    )?;
    fs::create_dir_all(fixture.context.data_dir.join("plugins/malformed"))?;
    fs::write(
        fixture
            .context
            .data_dir
            .join("plugins/malformed/plugin.json"),
        "{ malformed",
    )?;
    let mut config = Config::default();
    config.plugins.enabled = vec![
        "ready-plugin".to_owned(),
        "degraded-plugin".to_owned(),
        "blocked-plugin".to_owned(),
    ];
    config.plugins.disabled = vec!["disabled-plugin".to_owned()];

    let discovery = discover_plugins(
        &config,
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;
    let projection = build_spec031_extension_projection(&discovery.plugins);

    assert_extension(
        &projection,
        "ready-plugin",
        Spec031ExtensionReadiness::Ready,
        Spec031ExtensionReason::Ready,
    )?;
    assert_extension(
        &projection,
        "degraded-plugin",
        Spec031ExtensionReadiness::Degraded,
        Spec031ExtensionReason::Degraded,
    )?;
    assert_extension(
        &projection,
        "blocked-plugin",
        Spec031ExtensionReadiness::Blocked,
        Spec031ExtensionReason::Blocked,
    )?;
    assert_extension(
        &projection,
        "disabled-plugin",
        Spec031ExtensionReadiness::Unavailable,
        Spec031ExtensionReason::Unavailable,
    )?;
    assert_extension(
        &projection,
        "unavailable-plugin",
        Spec031ExtensionReadiness::Unavailable,
        Spec031ExtensionReason::Unavailable,
    )?;
    assert_extension(
        &projection,
        "malformed",
        Spec031ExtensionReadiness::Blocked,
        Spec031ExtensionReason::Blocked,
    )?;
    assert!(projection
        .extensions
        .iter()
        .all(|extension| extension.extension_ref.starts_with("ext_sha256:")));
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
