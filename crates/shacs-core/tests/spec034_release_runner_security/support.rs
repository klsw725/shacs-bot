use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    run_spec034_release_runner_with_linker_image, CommittedPublicationResult, Spec034ReleaseConfig,
    Spec034ReleaseMode,
};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const DOCUMENTS: [&str; 8] = [
    "results.json",
    "coverage-matrix.json",
    "review-records.json",
    "owner-audits.json",
    "failure-triage.json",
    "reproducibility-observations.json",
    "cleanup-receipt.json",
    "summary.json",
];
static RELEASE_EVIDENCE: OnceLock<Result<ReleaseEvidence, String>> = OnceLock::new();
static BASELINE_GENERATIONS: AtomicUsize = AtomicUsize::new(0);

pub struct ReleaseEvidence {
    _root: tempfile::TempDir,
    pub evidence: PathBuf,
    pub repo: PathBuf,
    pub publication: CommittedPublicationResult,
}

pub fn release_evidence() -> Result<&'static ReleaseEvidence, Box<dyn Error>> {
    RELEASE_EVIDENCE
        .get_or_init(|| create_release_evidence().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| error.clone().into())
}

pub fn baseline_generation_count() -> usize {
    BASELINE_GENERATIONS.load(Ordering::Acquire)
}

fn create_release_evidence() -> Result<ReleaseEvidence, Box<dyn Error>> {
    BASELINE_GENERATIONS.fetch_add(1, Ordering::AcqRel);
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let cache = evidence
        .parent()
        .ok_or("release parent missing")?
        .join("cache");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let publication = run_spec034_release_runner_with_linker_image(
        &Spec034ReleaseConfig {
            run_id: "spec034-success-fixture".to_owned(),
            repo_root: repo.clone(),
            evidence_root: evidence.clone(),
            cache_root: Some(cache.clone()),
            mode: Spec034ReleaseMode::SuccessFixture,
            command_timeout: Duration::from_secs(600),
        },
        Path::new(env!("CARGO_BIN_EXE_spec034-release-runner")),
    )?;
    restore_cache_root(&cache)?;
    std::fs::remove_dir_all(&cache)?;
    Ok(ReleaseEvidence {
        _root: root,
        evidence,
        repo,
        publication,
    })
}

#[cfg(unix)]
pub fn restore_cache_root(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        return Ok(());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            restore_cache_root(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn restore_cache_root(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

pub fn replace_run_ids(root: &Path, run_id: &str) -> Result<(), Box<dyn Error>> {
    for locator in DOCUMENTS {
        let path = root.join(locator);
        let mut value = read_json(&path)?;
        value["run_id"] = json!(run_id);
        write_json(&path, &value)?;
        rebind_digest(root, locator)?;
    }
    let path = root.join("manifest.json");
    let mut manifest = read_json(&path)?;
    manifest["run_id"] = json!(run_id);
    write_json(&path, &manifest)?;
    Ok(())
}

pub fn rebind_digest(root: &Path, locator: &str) -> Result<(), Box<dyn Error>> {
    let path = root.join("manifest.json");
    let mut manifest = read_json(&path)?;
    let row = manifest["artifact_digests"]
        .as_array_mut()
        .and_then(|rows| rows.iter_mut().find(|row| row["locator"] == locator))
        .ok_or("artifact digest row missing")?;
    row["digest"] = json!(digest(&root.join(locator))?);
    write_json(&path, &manifest)
}

pub fn digest(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(format!("sha256:{:x}", Sha256::digest(std::fs::read(path)?)))
}

pub fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

pub fn write_json(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
