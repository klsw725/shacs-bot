use crate::app::{AppError, AppId, AppManifest, AppRegistryStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const APP_AUTHORING_APP_ID_MAX_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdCandidate {
    app_id: AppId,
}

impl AppIdCandidate {
    pub fn parse(value: impl Into<String>) -> Result<Self, AppAuthoringError> {
        let value = value.into();
        if value.len() > APP_AUTHORING_APP_ID_MAX_LEN
            || value.chars().any(|character| {
                !character.is_ascii()
                    || character.is_whitespace()
                    || character.is_control()
                    || matches!(
                        character,
                        '!' | '$'
                            | '&'
                            | '*'
                            | '('
                            | ')'
                            | ';'
                            | '<'
                            | '>'
                            | '|'
                            | '`'
                            | '\''
                            | '"'
                    )
            })
        {
            return Err(AppAuthoringError::InvalidAppId(value));
        }
        let app_id = AppId::parse(value).map_err(AppAuthoringError::App)?;
        Ok(Self { app_id })
    }

    pub fn app_id(&self) -> &AppId {
        &self.app_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAuthoringDraftId(String);

impl AppAuthoringDraftId {
    pub fn for_app_id(app_id: &AppId) -> Self {
        Self(format!("draft-{app_id}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppAuthoringDraftId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppAuthoringState {
    DraftCreated,
    ScaffoldGenerated,
    Conflict,
    Failed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAuthoringDraft {
    pub draft_id: AppAuthoringDraftId,
    pub app_id: AppId,
    pub state: AppAuthoringState,
    pub source_command: String,
    pub current_revision_digest: String,
    pub generated_files: Vec<AppScaffoldFileCandidate>,
    pub warning_summary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScaffoldPlan {
    pub app_id: AppId,
    pub draft_id: AppAuthoringDraftId,
    pub files: Vec<AppScaffoldFileCandidate>,
    pub owner_boundary: String,
    pub risk_label: String,
    pub install_blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScaffoldFileCandidate {
    pub path: String,
    pub kind: String,
    pub digest: String,
    pub redaction_status: String,
    pub overwrite_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAuthoringInitOutcome {
    Created,
    AlreadyExistsSameContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAuthoringInitReport {
    pub outcome: AppAuthoringInitOutcome,
    pub draft_id: AppAuthoringDraftId,
    pub app_id: AppId,
    pub draft_path: PathBuf,
    pub manifest_candidate_path: PathBuf,
    pub readme_candidate_path: PathBuf,
    pub scaffold_plan_path: PathBuf,
    pub draft_metadata_path: PathBuf,
    pub current_revision_digest: String,
    pub generated_files: Vec<AppScaffoldFileCandidate>,
    pub validation_status: String,
    pub next_action: String,
}

pub struct AppAuthoringStore {
    data_dir: PathBuf,
}

impl AppAuthoringStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn authoring_apps_dir(&self) -> PathBuf {
        self.data_dir.join("authoring").join("apps")
    }

    pub fn init_app(
        &self,
        app_id: impl Into<String>,
    ) -> Result<AppAuthoringInitReport, AppAuthoringError> {
        let candidate = AppIdCandidate::parse(app_id)?;
        if AppRegistryStore::new(&self.data_dir)
            .inspect(candidate.app_id())
            .map_err(AppAuthoringError::App)?
            .is_some()
        {
            return Err(AppAuthoringError::InstalledApp(candidate.app_id().clone()));
        }

        let draft_id = AppAuthoringDraftId::for_app_id(candidate.app_id());
        let authoring_apps_dir = self.prepare_authoring_apps_dir()?;
        let draft_path = authoring_apps_dir.join(draft_id.as_str());
        reject_symlink_path(&draft_path)?;
        let scaffold = build_minimal_scaffold(candidate.app_id(), &draft_id)?;

        if draft_path.exists() {
            return self.inspect_existing_draft(candidate.app_id(), draft_id, draft_path, scaffold);
        }

        fs::create_dir_all(&draft_path).map_err(AppAuthoringError::Io)?;
        validate_child_path(&authoring_apps_dir, &draft_path)?;
        self.write_scaffold(
            &draft_path,
            candidate.app_id(),
            &draft_id,
            scaffold,
            AppAuthoringInitOutcome::Created,
        )
    }

    fn prepare_authoring_apps_dir(&self) -> Result<PathBuf, AppAuthoringError> {
        fs::create_dir_all(&self.data_dir).map_err(AppAuthoringError::Io)?;
        let canonical_data_dir = self
            .data_dir
            .canonicalize()
            .map_err(AppAuthoringError::Io)?;
        let authoring_dir = self.data_dir.join("authoring");
        reject_symlink_path(&authoring_dir)?;
        let authoring_apps_dir = authoring_dir.join("apps");
        reject_symlink_path(&authoring_apps_dir)?;
        fs::create_dir_all(&authoring_apps_dir).map_err(AppAuthoringError::Io)?;
        let canonical_authoring_apps_dir = authoring_apps_dir
            .canonicalize()
            .map_err(AppAuthoringError::Io)?;
        if !canonical_authoring_apps_dir.starts_with(&canonical_data_dir) {
            return Err(AppAuthoringError::UnsafePath {
                base: canonical_data_dir,
                path: canonical_authoring_apps_dir,
            });
        }
        Ok(canonical_authoring_apps_dir)
    }

    fn inspect_existing_draft(
        &self,
        app_id: &AppId,
        draft_id: AppAuthoringDraftId,
        draft_path: PathBuf,
        scaffold: MinimalScaffold,
    ) -> Result<AppAuthoringInitReport, AppAuthoringError> {
        let draft_bytes = fs::read(draft_path.join("draft.json")).map_err(AppAuthoringError::Io)?;
        let plan_bytes =
            fs::read(draft_path.join("scaffold-plan.json")).map_err(AppAuthoringError::Io)?;
        let manifest_bytes = fs::read(draft_path.join("candidates").join("manifest.json"))
            .map_err(AppAuthoringError::Io)?;
        let readme_bytes = fs::read(draft_path.join("candidates").join("README.md"))
            .map_err(AppAuthoringError::Io)?;
        if draft_bytes == scaffold.draft_metadata_bytes
            && plan_bytes == scaffold.scaffold_plan_bytes
            && manifest_bytes == scaffold.manifest_bytes
            && readme_bytes == scaffold.readme_bytes
        {
            return self.report(
                draft_path,
                app_id.clone(),
                draft_id,
                scaffold,
                AppAuthoringInitOutcome::AlreadyExistsSameContent,
            );
        }
        Err(AppAuthoringError::Conflict(draft_path))
    }

    fn write_scaffold(
        &self,
        draft_path: &Path,
        app_id: &AppId,
        draft_id: &AppAuthoringDraftId,
        scaffold: MinimalScaffold,
        outcome: AppAuthoringInitOutcome,
    ) -> Result<AppAuthoringInitReport, AppAuthoringError> {
        let candidates_dir = draft_path.join("candidates");
        fs::create_dir_all(&candidates_dir).map_err(AppAuthoringError::Io)?;
        write_create_new(
            &candidates_dir.join("manifest.json"),
            &scaffold.manifest_bytes,
        )?;
        write_create_new(&candidates_dir.join("README.md"), &scaffold.readme_bytes)?;

        write_create_new(
            &draft_path.join("scaffold-plan.json"),
            &scaffold.scaffold_plan_bytes,
        )?;
        write_create_new(
            &draft_path.join("draft.json"),
            &scaffold.draft_metadata_bytes,
        )?;

        self.report(
            draft_path.to_path_buf(),
            app_id.clone(),
            draft_id.clone(),
            scaffold,
            outcome,
        )
    }

    fn report(
        &self,
        draft_path: PathBuf,
        app_id: AppId,
        draft_id: AppAuthoringDraftId,
        scaffold: MinimalScaffold,
        outcome: AppAuthoringInitOutcome,
    ) -> Result<AppAuthoringInitReport, AppAuthoringError> {
        Ok(AppAuthoringInitReport {
            outcome,
            draft_id,
            app_id,
            manifest_candidate_path: draft_path.join("candidates").join("manifest.json"),
            readme_candidate_path: draft_path.join("candidates").join("README.md"),
            scaffold_plan_path: draft_path.join("scaffold-plan.json"),
            draft_metadata_path: draft_path.join("draft.json"),
            draft_path,
            current_revision_digest: scaffold.current_revision_digest,
            generated_files: scaffold.files,
            validation_status: "static scaffold created; no install, enable, start, secret, grant, skill, tool, service, process, MCP, package, or network side effect was performed".to_owned(),
            next_action: "review candidates, then run explicit future install handoff when implemented".to_owned(),
        })
    }
}

struct MinimalScaffold {
    manifest_bytes: Vec<u8>,
    readme_bytes: Vec<u8>,
    scaffold_plan_bytes: Vec<u8>,
    draft_metadata_bytes: Vec<u8>,
    files: Vec<AppScaffoldFileCandidate>,
    current_revision_digest: String,
}

fn build_minimal_scaffold(
    app_id: &AppId,
    draft_id: &AppAuthoringDraftId,
) -> Result<MinimalScaffold, AppAuthoringError> {
    let manifest = AppManifest {
        id: app_id.clone(),
        version: "0.1.0".to_owned(),
        entry: "README.md".to_owned(),
        name: Some(app_id.to_string()),
        skills: Vec::new(),
        resources: Vec::new(),
        permissions: Vec::new(),
        secrets: Vec::new(),
    };
    let manifest_bytes = json_bytes(&manifest)?;
    let readme = format!(
        "# {app_id}\n\nThis is an App Maker authoring draft. Review this scaffold before any explicit install handoff.\n\nDraft id: {draft_id}\n\nNo process, MCP server, package manager, network probe, secret read, grant creation, active skill injection, app registry mutation, install, enable, or start action has been performed.\n"
    );
    let readme_bytes = readme.into_bytes();
    let files = vec![
        file_candidate(
            "candidates/manifest.json",
            "manifest-candidate",
            &manifest_bytes,
        ),
        file_candidate("candidates/README.md", "readme-candidate", &readme_bytes),
    ];
    let mut digest = Sha256::new();
    for file in &files {
        digest.update(file.path.as_bytes());
        digest.update(file.digest.as_bytes());
    }
    let current_revision_digest = format!("sha256:{}", hex_string(&digest.finalize()));
    let plan = AppScaffoldPlan {
        app_id: app_id.clone(),
        draft_id: draft_id.clone(),
        files: files.clone(),
        owner_boundary: "authoring draft only; install, enable, start, grants, secrets, tools, services, and active skills are separate owner boundaries".to_owned(),
        risk_label: "no-run-static-scaffold".to_owned(),
        install_blockers: vec!["manual review and explicit install handoff are required".to_owned()],
    };
    let draft = AppAuthoringDraft {
        draft_id: draft_id.clone(),
        app_id: app_id.clone(),
        state: AppAuthoringState::ScaffoldGenerated,
        source_command: format!("apps init {app_id}"),
        current_revision_digest: current_revision_digest.clone(),
        generated_files: files.clone(),
        warning_summary: Vec::new(),
    };
    Ok(MinimalScaffold {
        manifest_bytes,
        readme_bytes,
        scaffold_plan_bytes: json_bytes(&plan)?,
        draft_metadata_bytes: json_bytes(&draft)?,
        files,
        current_revision_digest,
    })
}

fn file_candidate(path: &str, kind: &str, bytes: &[u8]) -> AppScaffoldFileCandidate {
    AppScaffoldFileCandidate {
        path: path.to_owned(),
        kind: kind.to_owned(),
        digest: format!("sha256:{}", sha256_hex(bytes)),
        redaction_status: "redacted".to_owned(),
        overwrite_policy: "create-new".to_owned(),
    }
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, AppAuthoringError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(AppAuthoringError::Json)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), AppAuthoringError> {
    let parent = path.parent().ok_or_else(|| {
        AppAuthoringError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path has no parent directory",
        ))
    })?;
    fs::create_dir_all(parent).map_err(AppAuthoringError::Io)?;
    reject_symlink_path(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(AppAuthoringError::Io)?;
    file.write_all(bytes).map_err(AppAuthoringError::Io)?;
    file.flush().map_err(AppAuthoringError::Io)?;
    file.sync_all().map_err(AppAuthoringError::Io)?;
    Ok(())
}

fn reject_symlink_path(path: &Path) -> Result<(), AppAuthoringError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppAuthoringError::UnsafePath {
            base: path.parent().unwrap_or(path).to_path_buf(),
            path: path.to_path_buf(),
        }),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn validate_child_path(base: &Path, path: &Path) -> Result<(), AppAuthoringError> {
    let canonical_base = base.canonicalize().map_err(AppAuthoringError::Io)?;
    let canonical_path = path.canonicalize().map_err(AppAuthoringError::Io)?;
    if !canonical_path.starts_with(&canonical_base) {
        return Err(AppAuthoringError::UnsafePath {
            base: canonical_base,
            path: canonical_path,
        });
    }
    Ok(())
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

#[derive(Debug)]
pub enum AppAuthoringError {
    App(AppError),
    Io(io::Error),
    Json(serde_json::Error),
    InvalidAppId(String),
    InstalledApp(AppId),
    Conflict(PathBuf),
    UnsafePath { base: PathBuf, path: PathBuf },
}

impl fmt::Display for AppAuthoringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::App(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "app authoring I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "app authoring JSON failed: {error}"),
            Self::InvalidAppId(value) => write!(
                formatter,
                "invalid app authoring id `{}`",
                escape_for_display(value)
            ),
            Self::InstalledApp(app_id) => write!(
                formatter,
                "app `{app_id}` is already installed; use a future explicit edit flow"
            ),
            Self::Conflict(path) => write!(
                formatter,
                "app authoring draft at `{}` has different content",
                escape_path_for_display(path)
            ),
            Self::UnsafePath { base, path } => write!(
                formatter,
                "app authoring path `{}` escapes base `{}`",
                escape_path_for_display(path),
                escape_path_for_display(base)
            ),
        }
    }
}

impl std::error::Error for AppAuthoringError {}

fn escape_for_display(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

fn escape_path_for_display(path: &Path) -> String {
    escape_for_display(&path.to_string_lossy())
}
