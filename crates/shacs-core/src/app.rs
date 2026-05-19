use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use shacs_utils::redaction::{is_sensitive_key, redact_value, REDACTED};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APP_BUNDLE_EXTENSION: &str = "shacsapp";
pub const APP_MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct AppId(String);

impl AppId {
    pub fn parse(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        if !is_valid_app_id(&value) {
            return Err(AppError::InvalidAppId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AppId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppBundlePath {
    path: PathBuf,
}

impl AppBundlePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.path.join(APP_MANIFEST_FILE)
    }

    pub fn for_workspace(workspace: &Path, app_id: &AppId) -> Self {
        Self::new(
            workspace
                .join(".shacs")
                .join("apps")
                .join(format!("{app_id}.{APP_BUNDLE_EXTENSION}")),
        )
    }

    pub fn app_id_from_bundle_name(&self) -> Result<AppId, AppError> {
        let bundle_path = self.as_path();
        if bundle_path.extension().and_then(|value| value.to_str()) != Some(APP_BUNDLE_EXTENSION) {
            return Err(AppError::InvalidBundleExtension(bundle_path.to_path_buf()));
        }
        let bundle_name = bundle_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let app_id = bundle_name
            .strip_suffix(&format!(".{APP_BUNDLE_EXTENSION}"))
            .unwrap_or_default();
        AppId::parse(app_id.to_owned())
    }

    pub fn validate_workspace_location(
        &self,
        workspace: &Path,
        app_id: &AppId,
    ) -> Result<(), AppError> {
        let actual = self.path.canonicalize().map_err(AppError::Io)?;
        let expected = workspace
            .canonicalize()
            .map_err(AppError::Io)?
            .join(".shacs")
            .join("apps")
            .join(format!("{app_id}.{APP_BUNDLE_EXTENSION}"));
        if actual != expected {
            return Err(AppError::InvalidBundleLocation { expected, actual });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppManifest {
    pub id: AppId,
    pub version: String,
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<PermissionRequestSummary>,
    #[serde(default)]
    pub secrets: Vec<SecretRequestSummary>,
}

impl AppManifest {
    pub fn load_from_bundle(bundle: &AppBundlePath) -> Result<ValidatedAppBundle, AppError> {
        let canonical_bundle_path = canonical_bundle_root(bundle)?;
        let manifest_path =
            canonical_path_inside_bundle(&canonical_bundle_path, APP_MANIFEST_FILE)?;
        let manifest_bytes = fs::read(&manifest_path).map_err(AppError::Io)?;
        let manifest = serde_json::from_slice::<Self>(&manifest_bytes).map_err(AppError::Json)?;
        let canonical_bundle = AppBundlePath::new(canonical_bundle_path);
        validate_manifest_for_bundle(&manifest, &canonical_bundle)?;
        let resource_summaries = summarize_declared_resources(&manifest, &canonical_bundle)?;
        let digest = compute_app_digest(&manifest_bytes, &resource_summaries);
        Ok(ValidatedAppBundle {
            manifest,
            bundle_path: canonical_bundle,
            manifest_path,
            digest,
            resource_summaries,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAppBundle {
    pub manifest: AppManifest,
    pub bundle_path: AppBundlePath,
    pub manifest_path: PathBuf,
    pub digest: String,
    pub resource_summaries: Vec<AppResourceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestSummary {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRequestSummary {
    pub key: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppResourceSummary {
    pub kind: String,
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppLifecycleState {
    Installed,
    Enabled,
    Disabled,
    Unavailable,
    Uninstalling,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRegistryEntry {
    pub app_id: AppId,
    pub version: String,
    pub digest: String,
    pub bundle_path: PathBuf,
    pub lifecycle_state: AppLifecycleState,
    #[serde(default)]
    pub permission_requests: Vec<PermissionRequestSummary>,
    #[serde(default)]
    pub secret_requests: Vec<SecretRequestSummary>,
    #[serde(default)]
    pub resource_summaries: Vec<AppResourceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_reference: Option<String>,
    #[serde(default)]
    pub unavailable_reasons: Vec<String>,
    #[serde(default)]
    pub process_snapshots: Vec<AppProcessSnapshot>,
    #[serde(default)]
    pub installed_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRegistry {
    #[serde(default)]
    pub entries: BTreeMap<AppId, AppRegistryEntry>,
}

impl AppRegistry {
    pub fn list(&self) -> Vec<&AppRegistryEntry> {
        self.entries.values().collect()
    }

    pub fn inspect(&self, app_id: &AppId) -> Option<&AppRegistryEntry> {
        self.entries.get(app_id)
    }
}

pub struct AppRegistryStore {
    data_dir: PathBuf,
}

impl AppRegistryStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn registry_path(&self) -> PathBuf {
        self.data_dir.join("apps").join("registry.json")
    }

    pub fn ledger_dir(&self) -> PathBuf {
        self.data_dir.join("runtime").join("app-ledger")
    }

    pub fn load(&self) -> Result<AppRegistry, AppError> {
        let path = self.registry_path();
        if !path.exists() {
            return Ok(AppRegistry::default());
        }
        let bytes = fs::read(path).map_err(AppError::Io)?;
        serde_json::from_slice(&bytes).map_err(AppError::Json)
    }

    pub fn save(&self, registry: &AppRegistry) -> Result<(), AppError> {
        write_json_atomic(&self.registry_path(), registry)
    }

    pub fn install_in_workspace(
        &self,
        workspace: &Path,
        bundle_path: impl Into<PathBuf>,
    ) -> Result<AppRegistryEntry, AppError> {
        let bundle = AppBundlePath::new(bundle_path);
        let app_id = bundle.app_id_from_bundle_name()?;
        bundle.validate_workspace_location(workspace, &app_id)?;
        let validated = AppManifest::load_from_bundle(&bundle)?;
        self.install_validated(validated)
    }

    fn install_validated(
        &self,
        validated: ValidatedAppBundle,
    ) -> Result<AppRegistryEntry, AppError> {
        let mut registry = self.load()?;
        if let Some(existing) = registry.entries.get(&validated.manifest.id) {
            if existing.bundle_path != validated.bundle_path.as_path() {
                return Err(AppError::AppIdCollision(validated.manifest.id.clone()));
            }
            if existing.digest != validated.digest {
                return Err(AppError::DigestMismatch(validated.manifest.id.clone()));
            }
            return Ok(existing.clone());
        }
        let unavailable_reasons = missing_secret_reasons(&validated.manifest.secrets);
        let lifecycle_state = if unavailable_reasons.is_empty() {
            AppLifecycleState::Installed
        } else {
            AppLifecycleState::Unavailable
        };
        let grant_reference = grant_reference_summary(
            &validated.manifest.id,
            &validated.digest,
            &validated.manifest.permissions,
            &validated.manifest.secrets,
        );
        let entry = AppRegistryEntry {
            app_id: validated.manifest.id.clone(),
            version: validated.manifest.version,
            digest: validated.digest,
            bundle_path: validated.bundle_path.as_path().to_path_buf(),
            lifecycle_state,
            permission_requests: validated.manifest.permissions,
            secret_requests: validated.manifest.secrets,
            resource_summaries: validated.resource_summaries,
            grant_reference,
            unavailable_reasons,
            process_snapshots: Vec::new(),
            installed_at_unix_ms: unix_ms_now(),
        };
        registry.entries.insert(entry.app_id.clone(), entry.clone());
        self.save(&registry)?;
        Ok(entry)
    }

    pub fn list(&self) -> Result<Vec<AppRegistryEntry>, AppError> {
        Ok(self.load()?.entries.into_values().collect())
    }

    pub fn inspect(&self, app_id: &AppId) -> Result<Option<AppRegistryEntry>, AppError> {
        Ok(self.load()?.entries.remove(app_id))
    }

    pub fn enable(&self, app_id: &AppId) -> Result<AppRegistryEntry, AppError> {
        self.update_entry(app_id, |entry| {
            entry.unavailable_reasons = missing_secret_reasons(&entry.secret_requests);
            entry.lifecycle_state = if entry.unavailable_reasons.is_empty() {
                AppLifecycleState::Enabled
            } else {
                AppLifecycleState::Unavailable
            };
        })
    }

    pub fn disable(&self, app_id: &AppId) -> Result<AppRegistryEntry, AppError> {
        self.update_entry(app_id, |entry| {
            entry.lifecycle_state = AppLifecycleState::Disabled;
        })
    }

    fn uninstall(&self, app_id: &AppId) -> Result<Option<AppRegistryEntry>, AppError> {
        let Some(entry) = self.mark_uninstalling(app_id)? else {
            return Ok(None);
        };
        let mut registry = self.load()?;
        let removed = registry.entries.remove(app_id);
        if entry.bundle_path.exists() {
            fs::remove_dir_all(&entry.bundle_path).map_err(AppError::Io)?;
        }
        self.save(&registry)?;
        Ok(removed)
    }

    pub fn uninstall_in_workspace(
        &self,
        workspace: &Path,
        app_id: &AppId,
    ) -> Result<Option<AppRegistryEntry>, AppError> {
        let Some(entry) = self.inspect(app_id)? else {
            return Ok(None);
        };
        AppBundlePath::new(&entry.bundle_path).validate_workspace_location(workspace, app_id)?;
        self.uninstall(app_id)
    }

    pub fn mark_uninstalling(&self, app_id: &AppId) -> Result<Option<AppRegistryEntry>, AppError> {
        let mut registry = self.load()?;
        let Some(entry) = registry.entries.get_mut(app_id) else {
            return Ok(None);
        };
        entry.lifecycle_state = AppLifecycleState::Uninstalling;
        let entry = entry.clone();
        self.save(&registry)?;
        Ok(Some(entry))
    }

    pub fn persist_ledger_entry(&self, entry: &TaskLedgerEntry) -> Result<PathBuf, AppError> {
        let sanitized = entry.sanitized()?;
        let path = self
            .ledger_dir()
            .join(format!("{}.json", sanitized.receipt_id));
        write_json_atomic(&path, &sanitized)?;
        Ok(path)
    }

    fn update_entry(
        &self,
        app_id: &AppId,
        update: impl FnOnce(&mut AppRegistryEntry),
    ) -> Result<AppRegistryEntry, AppError> {
        let mut registry = self.load()?;
        let Some(entry) = registry.entries.get_mut(app_id) else {
            return Err(AppError::UnknownApp(app_id.clone()));
        };
        update(entry);
        let entry = entry.clone();
        self.save(&registry)?;
        Ok(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct AppProcessId(String);

impl AppProcessId {
    pub fn parse(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        if value.trim().is_empty() || value.contains('/') || value.contains('\\') {
            return Err(AppError::InvalidProcessId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AppProcessId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProcessSnapshot {
    pub process_id: AppProcessId,
    pub app_id: AppId,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub originating_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_scope: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_grant_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_handle_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLedgerEntry {
    pub receipt_id: String,
    pub app_id: AppId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<AppProcessId>,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
    #[serde(default)]
    pub details: Value,
}

impl TaskLedgerEntry {
    pub fn sanitized(&self) -> Result<Self, AppError> {
        if !is_valid_receipt_id(&self.receipt_id) {
            return Err(AppError::InvalidReceiptId(self.receipt_id.clone()));
        }
        reject_raw_secret_fields(&self.details)?;
        let mut entry = self.clone();
        entry.decision = redact_scalar(&entry.decision);
        entry.device_reference = entry.device_reference.map(|value| redact_scalar(&value));
        entry.port_reference = entry.port_reference.map(|value| redact_scalar(&value));
        entry.grant_reference = entry.grant_reference.map(|value| redact_scalar(&value));
        entry.artifact_reference = entry.artifact_reference.map(|value| redact_scalar(&value));
        entry.details = redact_value(&entry.details);
        Ok(entry)
    }
}

#[derive(Debug)]
pub enum AppError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidAppId(String),
    InvalidBundleName { app_id: AppId, bundle_name: String },
    InvalidBundleLocation { expected: PathBuf, actual: PathBuf },
    InvalidBundleExtension(PathBuf),
    InvalidManifest(String),
    UnsafeBundlePath(String),
    AppIdCollision(AppId),
    DigestMismatch(AppId),
    UnknownApp(AppId),
    InvalidProcessId(String),
    InvalidReceiptId(String),
    RawSecretField(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "app I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "app JSON failed: {error}"),
            Self::InvalidAppId(value) => write!(formatter, "invalid app id `{value}`"),
            Self::InvalidBundleName {
                app_id,
                bundle_name,
            } => write!(
                formatter,
                "bundle basename `{bundle_name}` does not match app id `{app_id}`"
            ),
            Self::InvalidBundleLocation { expected, actual } => write!(
                formatter,
                "app bundle `{}` is outside expected local bundle path `{}`",
                actual.display(),
                expected.display()
            ),
            Self::InvalidBundleExtension(path) => write!(
                formatter,
                "app bundle `{}` must end with .{APP_BUNDLE_EXTENSION}",
                path.display()
            ),
            Self::InvalidManifest(message) => write!(formatter, "invalid app manifest: {message}"),
            Self::UnsafeBundlePath(path) => {
                write!(formatter, "unsafe bundle resource path `{path}`")
            }
            Self::AppIdCollision(app_id) => write!(
                formatter,
                "app id `{app_id}` is already registered from another bundle"
            ),
            Self::DigestMismatch(app_id) => write!(
                formatter,
                "app `{app_id}` manifest digest differs from registry"
            ),
            Self::UnknownApp(app_id) => write!(formatter, "unknown app `{app_id}`"),
            Self::InvalidProcessId(value) => write!(formatter, "invalid app process id `{value}`"),
            Self::InvalidReceiptId(value) => {
                write!(formatter, "invalid app ledger receipt id `{value}`")
            }
            Self::RawSecretField(key) => write!(
                formatter,
                "app ledger receipt contains raw secret-looking field `{key}`"
            ),
        }
    }
}

impl std::error::Error for AppError {}

fn is_valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
        && !value.contains('/')
        && !value.contains('\\')
}

fn validate_manifest_for_bundle(
    manifest: &AppManifest,
    bundle: &AppBundlePath,
) -> Result<(), AppError> {
    if manifest.version.trim().is_empty() {
        return Err(AppError::InvalidManifest("version is required".to_owned()));
    }
    if manifest.entry.trim().is_empty() {
        return Err(AppError::InvalidManifest("entry is required".to_owned()));
    }
    let bundle_path = bundle.as_path();
    if bundle_path.extension().and_then(|value| value.to_str()) != Some(APP_BUNDLE_EXTENSION) {
        return Err(AppError::InvalidBundleExtension(bundle_path.to_path_buf()));
    }
    let expected_name = format!("{}.{}", manifest.id, APP_BUNDLE_EXTENSION);
    let bundle_name = bundle_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    if bundle_name != expected_name {
        return Err(AppError::InvalidBundleName {
            app_id: manifest.id.clone(),
            bundle_name,
        });
    }
    validate_bundle_relative_path(&manifest.entry)?;
    for path in manifest.skills.iter().chain(manifest.resources.iter()) {
        validate_bundle_relative_path(path)?;
    }
    for permission in &manifest.permissions {
        if permission.id.trim().is_empty() {
            return Err(AppError::InvalidManifest(
                "permission id is required".to_owned(),
            ));
        }
    }
    for secret in &manifest.secrets {
        if secret.key.trim().is_empty() {
            return Err(AppError::InvalidManifest(
                "secret key is required".to_owned(),
            ));
        }
    }
    Ok(())
}

fn canonical_bundle_root(bundle: &AppBundlePath) -> Result<PathBuf, AppError> {
    let bundle_path = bundle.as_path();
    if bundle_path.extension().and_then(|value| value.to_str()) != Some(APP_BUNDLE_EXTENSION) {
        return Err(AppError::InvalidBundleExtension(bundle_path.to_path_buf()));
    }
    bundle_path.canonicalize().map_err(AppError::Io)
}

fn validate_bundle_relative_path(path: &str) -> Result<(), AppError> {
    let candidate = Path::new(path);
    if path.trim().is_empty() || candidate.is_absolute() {
        return Err(AppError::UnsafeBundlePath(path.to_owned()));
    }
    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(AppError::UnsafeBundlePath(path.to_owned()));
    }
    Ok(())
}

fn canonical_bundle_read_path(
    bundle: &AppBundlePath,
    relative_path: &str,
) -> Result<PathBuf, AppError> {
    validate_bundle_relative_path(relative_path)?;
    let bundle_root = bundle.as_path().canonicalize().map_err(AppError::Io)?;
    canonical_path_inside_bundle(&bundle_root, relative_path)
}

fn canonical_path_inside_bundle(
    bundle_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, AppError> {
    let target = bundle_root
        .join(relative_path)
        .canonicalize()
        .map_err(AppError::Io)?;
    if !target.starts_with(bundle_root) {
        return Err(AppError::UnsafeBundlePath(relative_path.to_owned()));
    }
    Ok(target)
}

fn summarize_declared_resources(
    manifest: &AppManifest,
    bundle: &AppBundlePath,
) -> Result<Vec<AppResourceSummary>, AppError> {
    let mut declared = vec![("entry", manifest.entry.as_str())];
    declared.extend(manifest.skills.iter().map(|path| ("skill", path.as_str())));
    declared.extend(
        manifest
            .resources
            .iter()
            .map(|path| ("resource", path.as_str())),
    );
    let mut summaries = Vec::with_capacity(declared.len());
    for (kind, relative_path) in declared {
        let path = canonical_bundle_read_path(bundle, relative_path)?;
        let bytes = fs::read(&path).map_err(AppError::Io)?;
        summaries.push(AppResourceSummary {
            kind: kind.to_owned(),
            path: relative_path.to_owned(),
            size_bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }
    summaries.sort_by(|left, right| left.kind.cmp(&right.kind).then(left.path.cmp(&right.path)));
    Ok(summaries)
}

fn grant_reference_summary(
    app_id: &AppId,
    digest: &str,
    permissions: &[PermissionRequestSummary],
    secrets: &[SecretRequestSummary],
) -> Option<String> {
    if permissions.is_empty() && secrets.is_empty() {
        return None;
    }
    let digest_prefix = if digest.len() > 12 {
        &digest[..12]
    } else {
        digest
    };
    Some(format!("local-grant-request:{app_id}:{digest_prefix}"))
}

fn compute_app_digest(manifest_bytes: &[u8], resource_summaries: &[AppResourceSummary]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"shacs-app-digest-v1\nmanifest\n");
    hasher.update(manifest_bytes);
    hasher.update(b"\nresources\n");
    for summary in resource_summaries {
        hasher.update(summary.kind.as_bytes());
        hasher.update(b"\0");
        hasher.update(summary.path.as_bytes());
        hasher.update(b"\0");
        hasher.update(summary.size_bytes.to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(summary.sha256.as_bytes());
        hasher.update(b"\n");
    }
    hex_string(&hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_string(&hasher.finalize())
}

fn hex_string(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn missing_secret_reasons(secrets: &[SecretRequestSummary]) -> Vec<String> {
    secrets
        .iter()
        .filter(|secret| secret.required)
        .map(|secret| format!("required secret `{}` is unavailable", secret.key))
        .collect()
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path has no parent directory",
        ))
    })?;
    fs::create_dir_all(parent).map_err(AppError::Io)?;
    let tmp_path = unique_tmp_path(path);
    let write_result = (|| -> Result<(), AppError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(AppError::Io)?;
        serde_json::to_writer_pretty(&mut file, value).map_err(AppError::Json)?;
        file.write_all(b"\n").map_err(AppError::Io)?;
        file.flush().map_err(AppError::Io)?;
        file.sync_all().map_err(AppError::Io)?;
        fs::rename(&tmp_path, path).map_err(AppError::Io)?;
        fsync_dir(parent).map_err(AppError::Io)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_result
}

fn fsync_dir(path: &Path) -> io::Result<()> {
    let dir = File::open(path)?;
    dir.sync_all()
}

fn unique_tmp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("registry.json");
    path.with_file_name(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()))
}

fn unix_ms_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn is_valid_receipt_id(value: &str) -> bool {
    is_valid_app_id(value)
}

fn reject_raw_secret_fields(value: &Value) -> Result<(), AppError> {
    match value {
        Value::Object(object) => reject_raw_secret_object(object),
        Value::Array(items) => {
            for item in items {
                reject_raw_secret_fields(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn reject_raw_secret_object(object: &Map<String, Value>) -> Result<(), AppError> {
    for (key, value) in object {
        if is_sensitive_key(key) && value.as_str().is_some_and(|text| text != REDACTED) {
            return Err(AppError::RawSecretField(key.clone()));
        }
        reject_raw_secret_fields(value)?;
    }
    Ok(())
}

fn redact_scalar(value: &str) -> String {
    match redact_value(&Value::String(value.to_owned())) {
        Value::String(text) => text,
        _ => REDACTED.to_owned(),
    }
}
