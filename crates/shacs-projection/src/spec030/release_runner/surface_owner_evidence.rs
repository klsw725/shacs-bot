use super::model::{
    Spec030ReleaseArtifactError, Spec030ReleaseRunnerMode, Spec030SurfaceOwnerEvidence,
    Spec030SurfaceOwnerReadiness, Spec030SurfaceOwnerShutdown,
};
use super::source_manifest::sha256_bytes;
use crate::release_evidence::EvidenceWriter;
use serde::Serialize;
use std::path::Path;

pub(super) const SURFACE_OWNER_SCHEMA: &str = "spec030.surface_owner.v1";

#[derive(Serialize)]
struct ReceiptPayload<'a> {
    schema: &'a str,
    production_owner: bool,
    owner_pid: u32,
    spawn: &'a super::model::Spec030SurfaceOwnerSpawnSpec,
    argv: &'a [String],
    bind_host: &'a str,
    requested_port: u16,
    bound_port: u16,
    readiness: Spec030SurfaceOwnerReadiness,
    shutdown: Spec030SurfaceOwnerShutdown,
    temp_root: &'a str,
    temp_root_removed: bool,
    stdout_path: &'a str,
    stderr_path: &'a str,
    stdout_sha256: &'a str,
    stderr_sha256: &'a str,
}

pub(super) fn fixture(
    writer: &EvidenceWriter,
) -> Result<Spec030SurfaceOwnerEvidence, Spec030ReleaseArtifactError> {
    let stdout = b"fixture production owner ready\n";
    let stderr = b"fixture production owner stopped\n";
    writer
        .write_new("surface/owner.stdout", stdout)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new("surface/owner.stderr", stderr)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let spawn = super::model::Spec030SurfaceOwnerSpawnSpec {
        executable: "spec030-success-fixture-owner".to_owned(),
        config_path: "surface/config.json".to_owned(),
        workspace_path: "surface/workspace".to_owned(),
        bind: "127.0.0.1:49152".to_owned(),
        allow_api_side_effects: true,
    };
    let argv = spawn.argv();
    signed(Spec030SurfaceOwnerEvidence {
        schema: SURFACE_OWNER_SCHEMA.to_owned(),
        production_owner: false,
        owner_pid: u32::MAX,
        spawn,
        argv,
        bind_host: "127.0.0.1".to_owned(),
        requested_port: 0,
        bound_port: 49_152,
        readiness: Spec030SurfaceOwnerReadiness::Observed,
        shutdown: Spec030SurfaceOwnerShutdown::Reaped,
        temp_root: "surface/owner-tmp".to_owned(),
        temp_root_removed: true,
        stdout_path: "surface/owner.stdout".to_owned(),
        stderr_path: "surface/owner.stderr".to_owned(),
        stdout_sha256: sha256_bytes(stdout),
        stderr_sha256: sha256_bytes(stderr),
        receipt_sha256: String::new(),
    })
}

pub(super) fn signed(
    mut evidence: Spec030SurfaceOwnerEvidence,
) -> Result<Spec030SurfaceOwnerEvidence, Spec030ReleaseArtifactError> {
    evidence.receipt_sha256 = receipt_hash(&evidence)?;
    Ok(evidence)
}

pub(super) fn validate(
    evidence: &Spec030SurfaceOwnerEvidence,
    mode: Spec030ReleaseRunnerMode,
    root: &Path,
    repo_root: &Path,
) -> Result<(), Spec030ReleaseArtifactError> {
    let valid_mode = match mode {
        Spec030ReleaseRunnerMode::SuccessFixture => !evidence.production_owner,
        Spec030ReleaseRunnerMode::CurrentWorktree => evidence.production_owner,
    };
    if evidence.schema != SURFACE_OWNER_SCHEMA
        || !valid_mode
        || evidence.owner_pid == 0
        || evidence.bind_host != "127.0.0.1"
        || evidence.requested_port != 0
        || evidence.bound_port == 0
        || evidence.bound_port == 8080
        || evidence.readiness != Spec030SurfaceOwnerReadiness::Observed
        || evidence.shutdown != Spec030SurfaceOwnerShutdown::Reaped
        || !evidence.temp_root_removed
        || super::cleanup::process_is_live(evidence.owner_pid)
        || evidence.receipt_sha256 != receipt_hash(evidence)?
    {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    if mode == Spec030ReleaseRunnerMode::CurrentWorktree {
        let expected = expected_spawn(root, repo_root, evidence.bound_port);
        if evidence.spawn != expected || evidence.argv != evidence.spawn.argv() {
            return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
        }
        validate_surface_binding(root, evidence)?;
    }
    let temp_root = relative(root, &evidence.temp_root)?;
    let stdout = std::fs::read(relative(root, &evidence.stdout_path)?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let stderr = std::fs::read(relative(root, &evidence.stderr_path)?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    if temp_root.exists()
        || sha256_bytes(&stdout) != evidence.stdout_sha256
        || sha256_bytes(&stderr) != evidence.stderr_sha256
    {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(())
}

fn receipt_hash(
    evidence: &Spec030SurfaceOwnerEvidence,
) -> Result<String, Spec030ReleaseArtifactError> {
    serde_json::to_vec(&ReceiptPayload {
        schema: &evidence.schema,
        production_owner: evidence.production_owner,
        owner_pid: evidence.owner_pid,
        spawn: &evidence.spawn,
        argv: &evidence.argv,
        bind_host: &evidence.bind_host,
        requested_port: evidence.requested_port,
        bound_port: evidence.bound_port,
        readiness: evidence.readiness,
        shutdown: evidence.shutdown,
        temp_root: &evidence.temp_root,
        temp_root_removed: evidence.temp_root_removed,
        stdout_path: &evidence.stdout_path,
        stderr_path: &evidence.stderr_path,
        stdout_sha256: &evidence.stdout_sha256,
        stderr_sha256: &evidence.stderr_sha256,
    })
    .map(|bytes| sha256_bytes(&bytes))
    .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)
}

fn expected_spawn(
    root: &Path,
    repo_root: &Path,
    port: u16,
) -> super::model::Spec030SurfaceOwnerSpawnSpec {
    super::model::Spec030SurfaceOwnerSpawnSpec {
        executable: repo_root
            .join("crates/target/debug")
            .join(format!("shacs-bot{}", std::env::consts::EXE_SUFFIX))
            .display()
            .to_string(),
        config_path: root.join("surface/config.json").display().to_string(),
        workspace_path: root.join("surface/workspace").display().to_string(),
        bind: format!("127.0.0.1:{port}"),
        allow_api_side_effects: true,
    }
}

fn validate_surface_binding(
    root: &Path,
    evidence: &Spec030SurfaceOwnerEvidence,
) -> Result<(), Spec030ReleaseArtifactError> {
    let config = std::fs::read(relative(root, "surface/config.json")?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let config = serde_json::from_slice::<serde_json::Value>(&config)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let credential = &config["providers"]["openai"]["credentialSource"];
    if config["api"]["host"] != "127.0.0.1"
        || config["api"]["port"] != evidence.bound_port
        || config["agents"]["defaults"]["provider"] != "openai"
        || config["agents"]["defaults"]["model"] != "gpt-4o"
        || config["agents"]["defaults"]["workspace"] != evidence.spawn.workspace_path
        || config["tools"]["exec"]["sandbox"] != "bwrap"
        || credential["schemaVersion"] != 1
        || credential["environment"] != super::surface_runner::OWNER_CREDENTIAL_ENV
        || credential["localAuth"] != false
        || config["providers"]["openai"].get("apiKey").is_some()
    {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(())
}

fn relative(root: &Path, value: &str) -> Result<std::path::PathBuf, Spec030ReleaseArtifactError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || !path.starts_with("surface")
    {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(root.join(path))
}
