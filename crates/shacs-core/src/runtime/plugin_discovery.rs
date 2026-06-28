use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_config::{Config, ConfigContext, EnvSource};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SUPPORTED_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginManifestSource {
    UserData,
    WorkspaceLocal,
}

impl PluginManifestSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserData => "user_data",
            Self::WorkspaceLocal => "workspace_local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    NotEnabled,
    Enabled,
    Disabled,
    Blocked,
}

impl PluginState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotEnabled => "not_enabled",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginBlockReason {
    BrokenManifest,
    DuplicateManifestName,
    UnsupportedSchemaVersion,
    UnsupportedManifestFormat,
    UnsafePath,
    UntrustedWorkspace,
    MissingEnvironmentRefs,
    MissingConfigRefs,
}

impl PluginBlockReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BrokenManifest => "broken_manifest",
            Self::DuplicateManifestName => "duplicate_manifest_name",
            Self::UnsupportedSchemaVersion => "unsupported_schema_version",
            Self::UnsupportedManifestFormat => "unsupported_manifest_format",
            Self::UnsafePath => "unsafe_path",
            Self::UntrustedWorkspace => "untrusted_workspace",
            Self::MissingEnvironmentRefs => "missing_environment_refs",
            Self::MissingConfigRefs => "missing_config_refs",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    #[serde(alias = "schema_version")]
    pub schema_version: u64,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub surfaces: Value,
    #[serde(default)]
    #[serde(alias = "requires_env")]
    pub requires_env: Vec<String>,
    #[serde(default)]
    #[serde(alias = "requires_config")]
    pub requires_config: Vec<String>,
    #[serde(default)]
    pub permissions: Value,
    #[serde(default)]
    pub entrypoints: Value,
    #[serde(default)]
    pub assets: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPlugin {
    pub id: String,
    pub state: PluginState,
    pub source: PluginManifestSource,
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub digest: Option<String>,
    pub manifest: Option<PluginManifest>,
    pub missing_env: Vec<String>,
    pub missing_config: Vec<String>,
    pub block_reasons: Vec<PluginBlockReason>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDiscovery {
    pub plugins: Vec<DiscoveredPlugin>,
}

#[derive(Debug)]
pub enum PluginDiscoveryError {
    Io(io::Error),
}

impl std::fmt::Display for PluginDiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "plugin discovery I/O failed: {error}"),
        }
    }
}

impl std::error::Error for PluginDiscoveryError {}

impl From<io::Error> for PluginDiscoveryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn discover_plugins<E>(
    config: &Config,
    context: &ConfigContext,
    env: &E,
) -> Result<PluginDiscovery, PluginDiscoveryError>
where
    E: EnvSource,
{
    let mut plugins = Vec::new();
    discover_root(
        &context.data_dir.join("plugins"),
        PluginManifestSource::UserData,
        config,
        context,
        env,
        &mut plugins,
    )?;
    discover_root(
        &context.workspace.join(".shacs-bot").join("plugins"),
        PluginManifestSource::WorkspaceLocal,
        config,
        context,
        env,
        &mut plugins,
    )?;
    block_duplicate_manifest_names(&mut plugins);
    plugins.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.root.cmp(&right.root))
    });
    Ok(PluginDiscovery { plugins })
}

fn block_duplicate_manifest_names(plugins: &mut [DiscoveredPlugin]) {
    let mut by_id: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, plugin) in plugins.iter().enumerate() {
        if plugin.manifest.is_some() && !plugin.id.is_empty() {
            by_id.entry(plugin.id.clone()).or_default().push(index);
        }
    }

    for (id, indexes) in by_id {
        if indexes.len() < 2 {
            continue;
        }
        let mut roots = indexes
            .iter()
            .map(|index| plugins[*index].root.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        roots.sort();
        let diagnostic = format!(
            "duplicate plugin manifest name `{id}` found in: {}",
            roots.join(", ")
        );
        for index in indexes {
            let plugin = &mut plugins[index];
            plugin.state = PluginState::Blocked;
            if !plugin
                .block_reasons
                .contains(&PluginBlockReason::DuplicateManifestName)
            {
                plugin
                    .block_reasons
                    .push(PluginBlockReason::DuplicateManifestName);
            }
            if !plugin.diagnostics.iter().any(|value| value == &diagnostic) {
                plugin.diagnostics.push(diagnostic.clone());
            }
        }
    }
}

fn discover_root<E>(
    root: &Path,
    source: PluginManifestSource,
    config: &Config,
    context: &ConfigContext,
    env: &E,
    plugins: &mut Vec<DiscoveredPlugin>,
) -> Result<(), PluginDiscoveryError>
where
    E: EnvSource,
{
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        plugins.push(blocked_without_manifest(
            root,
            root,
            source,
            PluginBlockReason::UnsafePath,
            "plugin root must be a real directory".to_owned(),
        ));
        return Ok(());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let plugin_root = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let entry_metadata = fs::symlink_metadata(&plugin_root)?;
        if entry_metadata.file_type().is_symlink() {
            plugins.push(blocked_without_manifest(
                &plugin_root,
                &plugin_root,
                source,
                PluginBlockReason::UnsafePath,
                format!("plugin root `{name}` must not be a symlink"),
            ));
            continue;
        }
        if !entry_metadata.is_dir() {
            continue;
        }

        let manifest_path = plugin_root.join("plugin.json");
        match fs::symlink_metadata(&manifest_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                plugins.push(blocked_without_manifest(
                    &plugin_root,
                    &manifest_path,
                    source,
                    PluginBlockReason::UnsafePath,
                    "plugin.json manifest must not be a symlink".to_owned(),
                ));
                continue;
            }
            Ok(metadata) if metadata.is_file() => {
                plugins.push(discover_manifest(
                    &plugin_root,
                    &manifest_path,
                    PluginManifestFormat::Json,
                    source,
                    config,
                    context,
                    env,
                ));
                continue;
            }
            Ok(_) => {
                plugins.push(blocked_without_manifest(
                    &plugin_root,
                    &manifest_path,
                    source,
                    PluginBlockReason::UnsafePath,
                    "plugin.json manifest must be a regular file".to_owned(),
                ));
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let toml_path = plugin_root.join("plugin.toml");
        match fs::symlink_metadata(&toml_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                plugins.push(blocked_without_manifest(
                    &plugin_root,
                    &toml_path,
                    source,
                    PluginBlockReason::UnsafePath,
                    "plugin.toml manifest must not be a symlink".to_owned(),
                ))
            }
            Ok(metadata) if metadata.is_file() => plugins.push(discover_manifest(
                &plugin_root,
                &toml_path,
                PluginManifestFormat::Toml,
                source,
                config,
                context,
                env,
            )),
            Ok(_) => plugins.push(blocked_without_manifest(
                &plugin_root,
                &toml_path,
                source,
                PluginBlockReason::UnsafePath,
                "plugin.toml manifest must be a regular file".to_owned(),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginManifestFormat {
    Json,
    Toml,
}

impl PluginManifestFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Toml => "TOML",
        }
    }
}

fn discover_manifest<E>(
    root: &Path,
    manifest_path: &Path,
    format: PluginManifestFormat,
    source: PluginManifestSource,
    config: &Config,
    context: &ConfigContext,
    env: &E,
) -> DiscoveredPlugin
where
    E: EnvSource,
{
    let mut block_reasons = Vec::new();
    let mut diagnostics = Vec::new();
    if !safe_manifest_path(root, manifest_path) {
        return blocked_without_manifest(
            root,
            manifest_path,
            source,
            PluginBlockReason::UnsafePath,
            "plugin manifest path must stay inside a real plugin root".to_owned(),
        );
    }

    let raw = match fs::read(manifest_path) {
        Ok(raw) => raw,
        Err(error) => {
            return blocked_without_manifest(
                root,
                manifest_path,
                source,
                PluginBlockReason::BrokenManifest,
                format!("failed to read plugin manifest: {error}"),
            );
        }
    };
    let digest = Some(format!("sha256:{}", sha256_hex(&raw)));
    let manifest = match parse_manifest(&raw, format) {
        Ok(manifest) => manifest,
        Err(error) => {
            let mut plugin = blocked_without_manifest(
                root,
                manifest_path,
                source,
                PluginBlockReason::BrokenManifest,
                format!(
                    "failed to parse plugin manifest {}: {error}",
                    format.label()
                ),
            );
            plugin.digest = digest;
            return plugin;
        }
    };

    let id = manifest.name.trim().to_owned();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        block_reasons.push(PluginBlockReason::BrokenManifest);
        diagnostics.push("plugin manifest name must be a non-empty safe identifier".to_owned());
    }
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        block_reasons.push(PluginBlockReason::UnsupportedSchemaVersion);
        diagnostics.push(format!(
            "unsupported plugin schemaVersion {}; supported version is {SUPPORTED_SCHEMA_VERSION}",
            manifest.schema_version
        ));
    }

    let disabled = contains_name(&config.plugins.disabled, &id);
    let enabled = contains_name(&config.plugins.enabled, &id) && !disabled;
    let missing_env = missing_env_refs(&manifest.requires_env, env);
    let missing_config = missing_config_refs(&manifest.requires_config, config);
    if enabled && !missing_env.is_empty() {
        block_reasons.push(PluginBlockReason::MissingEnvironmentRefs);
        diagnostics.push(format!(
            "missing required environment refs: {}",
            missing_env.join(", ")
        ));
    }
    if enabled && !missing_config.is_empty() {
        block_reasons.push(PluginBlockReason::MissingConfigRefs);
        diagnostics.push(format!(
            "missing required config refs: {}",
            missing_config.join(", ")
        ));
    }
    if enabled
        && source == PluginManifestSource::WorkspaceLocal
        && !config.plugins.trusts_workspace(&context.workspace)
    {
        block_reasons.push(PluginBlockReason::UntrustedWorkspace);
        diagnostics
            .push("workspace-local plugin requires an explicit trustedWorkspaces match".to_owned());
    }

    let state = if disabled {
        PluginState::Disabled
    } else if !block_reasons.is_empty() {
        PluginState::Blocked
    } else if enabled {
        PluginState::Enabled
    } else {
        PluginState::NotEnabled
    };

    DiscoveredPlugin {
        id,
        state,
        source,
        root: root.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        digest,
        manifest: Some(manifest),
        missing_env,
        missing_config,
        block_reasons,
        diagnostics,
    }
}

fn parse_manifest(
    raw: &[u8],
    format: PluginManifestFormat,
) -> Result<PluginManifest, Box<dyn std::error::Error + Send + Sync>> {
    match format {
        PluginManifestFormat::Json => Ok(serde_json::from_slice::<PluginManifest>(raw)?),
        PluginManifestFormat::Toml => {
            let text = std::str::from_utf8(raw)?;
            Ok(toml::from_str::<PluginManifest>(text)?)
        }
    }
}

fn blocked_without_manifest(
    root: &Path,
    manifest_path: &Path,
    source: PluginManifestSource,
    reason: PluginBlockReason,
    diagnostic: String,
) -> DiscoveredPlugin {
    let id = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown-plugin")
        .to_owned();
    DiscoveredPlugin {
        id,
        state: PluginState::Blocked,
        source,
        root: root.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        digest: None,
        manifest: None,
        missing_env: Vec::new(),
        missing_config: Vec::new(),
        block_reasons: vec![reason],
        diagnostics: vec![diagnostic],
    }
}

fn contains_name(names: &[String], id: &str) -> bool {
    names.iter().any(|name| name == id)
}

fn missing_env_refs<E>(refs: &[String], env: &E) -> Vec<String>
where
    E: EnvSource,
{
    sorted_missing(refs, |name| {
        env.var(name).is_some_and(|value| !value.is_empty())
    })
}

fn missing_config_refs(refs: &[String], config: &Config) -> Vec<String> {
    sorted_missing(refs, |name| {
        config.env.get(name).is_some_and(|value| !value.is_empty())
    })
}

fn sorted_missing<F>(refs: &[String], present: F) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    refs.iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| !present(value))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn safe_manifest_path(root: &Path, manifest_path: &Path) -> bool {
    let Ok(root_metadata) = fs::symlink_metadata(root) else {
        return false;
    };
    let Ok(manifest_metadata) = fs::symlink_metadata(manifest_path) else {
        return false;
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return false;
    }
    if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
        return false;
    }
    let root = canonical_or_original(root);
    let manifest = canonical_or_original(manifest_path);
    manifest.starts_with(root)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
