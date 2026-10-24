use serde_json::json;
use shacs_config::{Config, ConfigContext};
use shacs_core::runtime::{discover_plugins, PluginBlockReason, PluginManifestSource, PluginState};
use shacs_core::tools::ToolRegistry;
use shacs_skills::{discover_skill_registry, SkillRegistryOptions};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn spec025_plugin_manifest_not_enabled_by_default_from_user_data_root(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/review/plugin.json"),
        json!({
            "schemaVersion": 1,
            "name": "review",
            "version": "0.1.0",
            "description": "Review helper",
            "surfaces": {"tools": ["review_comment"]},
            "requiresEnv": [],
            "requiresConfig": [],
            "permissions": {},
            "entrypoints": {"tools": {"review_comment": {"command": "review"}}},
            "assets": []
        }),
    )?;

    let discovery = discover_plugins(
        &Config::default(),
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    assert_eq!(discovery.plugins.len(), 1);
    let plugin = &discovery.plugins[0];
    assert_eq!(plugin.id, "review");
    assert_eq!(plugin.state, PluginState::NotEnabled);
    assert_eq!(plugin.source, PluginManifestSource::UserData);
    assert!(plugin
        .digest
        .as_deref()
        .is_some_and(|digest| digest.starts_with("sha256:") && digest.len() == 71));
    Ok(())
}

#[test]
fn spec025_plugin_manifest_config_enabled_and_disabled_wins(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_minimal_manifest(
        &fixture.context.data_dir.join("plugins/enabled/plugin.json"),
        "enabled-plugin",
    )?;
    write_minimal_manifest(
        &fixture
            .context
            .data_dir
            .join("plugins/disabled/plugin.json"),
        "disabled-plugin",
    )?;

    let mut config = Config::default();
    config.plugins.enabled = vec!["enabled-plugin".to_owned(), "disabled-plugin".to_owned()];
    config.plugins.disabled = vec!["disabled-plugin".to_owned()];
    let discovery = discover_plugins(
        &config,
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    assert_eq!(
        state_for(&discovery.plugins, "enabled-plugin")?,
        PluginState::Enabled
    );
    assert_eq!(
        state_for(&discovery.plugins, "disabled-plugin")?,
        PluginState::Disabled
    );
    Ok(())
}

#[test]
fn spec025_workspace_local_enabled_plugin_requires_trusted_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_minimal_manifest(
        &fixture
            .context
            .workspace
            .join(".shacs-bot/plugins/local/plugin.json"),
        "local-plugin",
    )?;

    let mut config = Config::default();
    config.plugins.enabled = vec!["local-plugin".to_owned()];
    let blocked = discover_plugins(
        &config,
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;
    let plugin = plugin_for(&blocked.plugins, "local-plugin")?;
    assert_eq!(plugin.state, PluginState::Blocked);
    assert!(plugin
        .block_reasons
        .contains(&PluginBlockReason::UntrustedWorkspace));

    config.plugins.trusted_workspaces =
        vec![fixture.context.workspace.to_string_lossy().to_string()];
    let trusted = discover_plugins(
        &config,
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;
    assert_eq!(
        state_for(&trusted.plugins, "local-plugin")?,
        PluginState::Enabled
    );
    Ok(())
}

#[test]
fn spec025_enabled_plugin_missing_refs_blocks_without_exposing_values(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/refs/plugin.json"),
        json!({
            "schemaVersion": 1,
            "name": "refs-plugin",
            "version": "0.1.0",
            "requiresEnv": ["PRESENT_ENV", "MISSING_ENV"],
            "requiresConfig": ["PRESENT_CONFIG", "MISSING_CONFIG"],
            "surfaces": {},
            "permissions": {},
            "entrypoints": {},
            "assets": []
        }),
    )?;

    let mut config = Config::default();
    config.plugins.enabled = vec!["refs-plugin".to_owned()];
    config.env.insert(
        "PRESENT_CONFIG".to_owned(),
        "config-secret-value".to_owned(),
    );
    let env = BTreeMap::from([("PRESENT_ENV".to_owned(), "env-secret-value".to_owned())]);
    let discovery = discover_plugins(&config, &fixture.context, &env)?;

    let plugin = plugin_for(&discovery.plugins, "refs-plugin")?;
    assert_eq!(plugin.state, PluginState::Blocked);
    assert_eq!(plugin.missing_env, ["MISSING_ENV"]);
    assert_eq!(plugin.missing_config, ["MISSING_CONFIG"]);
    let diagnostics = plugin.diagnostics.join("\n");
    assert!(diagnostics.contains("MISSING_ENV"));
    assert!(diagnostics.contains("MISSING_CONFIG"));
    assert!(!diagnostics.contains("env-secret-value"));
    assert!(!diagnostics.contains("config-secret-value"));
    Ok(())
}

#[test]
fn spec025_toml_manifest_uses_same_discovery_and_ref_gates(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_file(
        &fixture.context.data_dir.join("plugins/toml/plugin.toml"),
        r#"
schema_version = 1
name = "toml-plugin"
version = "0.1.0"
description = "TOML plugin"
requires_env = ["PRESENT_ENV", "MISSING_ENV"]
requires_config = ["PRESENT_CONFIG"]

[surfaces]
tools = ["toml_tool"]
hooks = ["llm:before"]

[permissions]

[entrypoints.tools.toml_tool]
command = "toml-tool"

[assets]
"#,
    )?;

    let mut config = Config::default();
    config.plugins.enabled = vec!["toml-plugin".to_owned()];
    config.env.insert(
        "PRESENT_CONFIG".to_owned(),
        "config-secret-value".to_owned(),
    );
    let env = BTreeMap::from([("PRESENT_ENV".to_owned(), "env-secret-value".to_owned())]);
    let discovery = discover_plugins(&config, &fixture.context, &env)?;

    let plugin = plugin_for(&discovery.plugins, "toml-plugin")?;
    assert_eq!(
        plugin
            .manifest_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("plugin.toml")
    );
    assert_eq!(plugin.state, PluginState::Blocked);
    assert_eq!(plugin.missing_env, ["MISSING_ENV"]);
    assert_eq!(plugin.missing_config, Vec::<String>::new());
    assert!(plugin
        .block_reasons
        .contains(&PluginBlockReason::MissingEnvironmentRefs));
    assert!(plugin
        .digest
        .as_deref()
        .is_some_and(|digest| digest.starts_with("sha256:") && digest.len() == 71));
    let manifest = plugin.manifest.as_ref().ok_or("missing parsed manifest")?;
    assert_eq!(manifest.name, "toml-plugin");
    assert_eq!(manifest.surfaces["tools"], json!(["toml_tool"]));
    assert_eq!(manifest.surfaces["hooks"], json!(["llm:before"]));
    let diagnostics = plugin.diagnostics.join("\n");
    assert!(diagnostics.contains("MISSING_ENV"));
    assert!(!diagnostics.contains("env-secret-value"));
    assert!(!diagnostics.contains("config-secret-value"));
    Ok(())
}

#[test]
fn spec025_broken_version_unsafe_path_and_invalid_toml_manifest_are_blocked(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/v2/plugin.json"),
        json!({
            "schemaVersion": 2,
            "name": "v2-plugin",
            "version": "0.1.0",
            "surfaces": {},
            "requiresEnv": [],
            "requiresConfig": [],
            "permissions": {},
            "entrypoints": {},
            "assets": []
        }),
    )?;
    write_manifest(
        &fixture.context.data_dir.join("plugins/broken/plugin.json"),
        json!({}),
    )?;
    write_file(
        &fixture.context.data_dir.join("plugins/toml/plugin.toml"),
        "name = \"toml\"",
    )?;
    make_symlink_dir(
        &fixture.context.data_dir.join("plugins/linked"),
        &fixture.context.data_dir.join("plugins/v2"),
    )?;

    let discovery = discover_plugins(
        &Config::default(),
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    assert!(plugin_for(&discovery.plugins, "v2-plugin")?
        .block_reasons
        .contains(&PluginBlockReason::UnsupportedSchemaVersion));
    assert!(plugin_for(&discovery.plugins, "broken")?
        .block_reasons
        .contains(&PluginBlockReason::BrokenManifest));
    assert!(plugin_for(&discovery.plugins, "toml")?
        .block_reasons
        .contains(&PluginBlockReason::BrokenManifest));
    assert!(plugin_for(&discovery.plugins, "linked")?
        .block_reasons
        .contains(&PluginBlockReason::UnsafePath));
    Ok(())
}

#[test]
fn spec025_malformed_toml_manifest_is_blocked_with_digest() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    write_file(
        &fixture
            .context
            .data_dir
            .join("plugins/malformed/plugin.toml"),
        "schema_version =\nname = \"malformed\"",
    )?;

    let discovery = discover_plugins(
        &Config::default(),
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    let plugin = plugin_for(&discovery.plugins, "malformed")?;
    assert_eq!(plugin.state, PluginState::Blocked);
    assert!(plugin
        .block_reasons
        .contains(&PluginBlockReason::BrokenManifest));
    assert_eq!(plugin.manifest, None);
    assert!(plugin
        .digest
        .as_deref()
        .is_some_and(|digest| digest.starts_with("sha256:") && digest.len() == 71));
    assert!(plugin
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.contains("failed to parse plugin manifest TOML")));
    Ok(())
}

#[cfg(unix)]
#[test]
fn spec025_plugin_manifest_symlink_file_is_blocked_without_reading_target(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let target = fixture.context.data_dir.join("outside-manifest.json");
    write_minimal_manifest(&target, "outside-target")?;
    let plugin_dir = fixture.context.data_dir.join("plugins/manifest-symlink");
    fs::create_dir_all(&plugin_dir)?;
    make_symlink_file(&plugin_dir.join("plugin.json"), &target)?;

    let discovery = discover_plugins(
        &Config::default(),
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    let plugin = plugin_for(&discovery.plugins, "manifest-symlink")?;
    assert_eq!(plugin.state, PluginState::Blocked);
    assert!(plugin
        .block_reasons
        .contains(&PluginBlockReason::UnsafePath));
    assert_eq!(plugin.digest, None);
    assert_eq!(plugin.manifest, None);
    assert!(plugin_for(&discovery.plugins, "outside-target").is_err());
    Ok(())
}

#[test]
fn spec025_duplicate_manifest_names_are_blocked_deterministically(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    write_minimal_manifest(
        &fixture.context.data_dir.join("plugins/alpha/plugin.json"),
        "duplicate-plugin",
    )?;
    write_minimal_manifest(
        &fixture
            .context
            .workspace
            .join(".shacs-bot/plugins/beta/plugin.json"),
        "duplicate-plugin",
    )?;

    let discovery = discover_plugins(
        &Config::default(),
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;

    let duplicates = discovery
        .plugins
        .iter()
        .filter(|plugin| plugin.id == "duplicate-plugin")
        .collect::<Vec<_>>();
    assert_eq!(duplicates.len(), 2);
    assert!(duplicates
        .iter()
        .all(|plugin| plugin.state == PluginState::Blocked));
    assert!(duplicates.iter().all(|plugin| plugin
        .block_reasons
        .contains(&PluginBlockReason::DuplicateManifestName)));
    assert!(duplicates.iter().all(|plugin| plugin
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.contains("duplicate plugin manifest name"))));
    Ok(())
}

#[test]
fn spec025_plugin_manifest_discovery_does_not_register_active_tool_or_skill(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let plugin_root = fixture.context.data_dir.join("plugins/surface");
    write_manifest(
        &plugin_root.join("plugin.json"),
        json!({
            "schemaVersion": 1,
            "name": "surface-plugin",
            "version": "0.1.0",
            "surfaces": {"skills": ["surface-skill"], "tools": ["surface_tool"]},
            "requiresEnv": [],
            "requiresConfig": [],
            "permissions": {},
            "entrypoints": {"skills": {"surface-skill": "skills/surface-skill/SKILL.md"}, "tools": {"surface_tool": {"command": "surface"}}},
            "assets": []
        }),
    )?;
    write_file(
        &plugin_root.join("skills/surface-skill/SKILL.md"),
        "---\nname: surface-skill\ndescription: plugin skill\n---\nbody",
    )?;
    let mut config = Config::default();
    config.plugins.enabled = vec!["surface-plugin".to_owned()];

    let discovery = discover_plugins(
        &config,
        &fixture.context,
        &BTreeMap::<String, String>::new(),
    )?;
    assert_eq!(
        state_for(&discovery.plugins, "surface-plugin")?,
        PluginState::Enabled
    );

    let tools = ToolRegistry::new();
    assert!(!tools.has("surface_tool"));
    let mut options = SkillRegistryOptions::new(&fixture.context.workspace);
    options.plugin_roots = vec![plugin_root.join("skills")];
    let registry = discover_skill_registry(options)?;
    assert!(registry.find("surface-skill").is_none());
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

fn write_minimal_manifest(path: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    write_manifest(
        path,
        json!({
            "schemaVersion": 1,
            "name": name,
            "version": "0.1.0",
            "surfaces": {},
            "requiresEnv": [],
            "requiresConfig": [],
            "permissions": {},
            "entrypoints": {},
            "assets": []
        }),
    )
}

fn write_manifest(path: &Path, value: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    write_file(path, &serde_json::to_string_pretty(&value)?)
}

fn write_file(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

fn state_for(
    plugins: &[shacs_core::runtime::DiscoveredPlugin],
    id: &str,
) -> Result<PluginState, String> {
    Ok(plugin_for(plugins, id)?.state)
}

fn plugin_for<'a>(
    plugins: &'a [shacs_core::runtime::DiscoveredPlugin],
    id: &str,
) -> Result<&'a shacs_core::runtime::DiscoveredPlugin, String> {
    plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .ok_or_else(|| format!("missing plugin {id}"))
}

#[cfg(unix)]
fn make_symlink_dir(link: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(unix)]
fn make_symlink_file(link: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
fn make_symlink_dir(link: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::os::windows::fs::symlink_dir(target, link)?;
    Ok(())
}
