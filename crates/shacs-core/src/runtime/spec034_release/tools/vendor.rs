use super::*;
use serde::Deserialize;
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const VENDOR_TIMEOUT: Duration = Duration::from_secs(600);

pub(super) struct VendorBinding {
    pub(super) lock_digest: String,
    pub(super) tree_digest: String,
    pub(super) inventory_digest: String,
}

#[derive(Deserialize)]
struct CargoLock {
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

#[derive(Deserialize)]
struct CargoChecksum {
    package: Option<String>,
    files: BTreeMap<String, String>,
}

pub(super) fn prepare(
    cargo: &Path,
    manifest: &Path,
    home: &Path,
    ledger: &ExecutionLedger,
) -> Result<(PathBuf, VendorBinding), Spec034ReleaseArtifactError> {
    let workspace = manifest
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let lock_path = workspace.join("Cargo.lock");
    let lock_bytes = std::fs::read(&lock_path).map_err(Spec034ReleaseArtifactError::Io)?;
    let packages = lock_packages(&lock_bytes)?;
    let vendor = home
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .join("vendor");
    let stdout = tempfile::tempfile().map_err(Spec034ReleaseArtifactError::Io)?;
    let stderr = tempfile::tempfile().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut command = minimal_command(cargo, workspace);
    command.args([
        "vendor",
        "--locked",
        "--versioned-dirs",
        vendor
            .to_str()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
    ]);
    if let Some(cargo_home) = ambient_cargo_home() {
        command.env("CARGO_HOME", cargo_home);
    }
    let mut child = super::spawn::spawn_verified(&command, &stdout, &stderr, ledger)?;
    let deadline = Instant::now()
        .checked_add(VENDOR_TIMEOUT)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let primary = loop {
        if let Err(error) = ledger.verify() {
            break Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(Spec034ReleaseArtifactError::Io(error)),
        }
        if Instant::now() >= deadline {
            break Err(Spec034ReleaseArtifactError::CommandFailed);
        }
        std::thread::yield_now();
    };
    let cleanup = child.terminate_and_reap();
    let status = Spec034ReleaseArtifactError::combine(primary, cleanup)?;
    if !status.success() {
        return Err(Spec034ReleaseArtifactError::CommandFailed);
    }
    let inventory_digest = validate_vendor(&vendor, &packages)?;
    set_read_only_closure(&vendor)?;
    let handle = File::open(&vendor).map_err(Spec034ReleaseArtifactError::Io)?;
    let tree_digest = super::super::source_descriptor::digest_tree(
        &handle,
        super::super::source_descriptor::TreeKind::Cache,
    )?;
    Ok((
        vendor,
        VendorBinding {
            lock_digest: digest_bytes(&lock_bytes),
            tree_digest,
            inventory_digest,
        },
    ))
}

fn lock_packages(bytes: &[u8]) -> Result<BTreeMap<String, String>, Spec034ReleaseArtifactError> {
    let lock: CargoLock = toml::from_str(
        std::str::from_utf8(bytes).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
    )
    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    lock.package
        .into_iter()
        .filter(|package| package.source.as_deref().is_some_and(|source| source.starts_with("registry+")))
        .map(|package| {
            Ok((
                format!("{}-{}", package.name, package.version),
                package.checksum.ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
            ))
        })
        .collect()
}

fn validate_vendor(
    vendor: &Path,
    expected: &BTreeMap<String, String>,
) -> Result<String, Spec034ReleaseArtifactError> {
    let mut observed = BTreeSet::new();
    let mut digest = sha2::Sha256::new();
    for entry in sorted_entries(vendor)? {
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(Spec034ReleaseArtifactError::Io)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let package_checksum = expected.get(&name).ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
        let checksum_bytes = std::fs::read(entry.path().join(".cargo-checksum.json"))
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let checksum: CargoChecksum = serde_json::from_slice(&checksum_bytes)
            .map_err(Spec034ReleaseArtifactError::Json)?;
        if checksum.package.as_deref() != Some(package_checksum) {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let mut files = BTreeSet::new();
        validate_files(&entry.path(), &entry.path(), &checksum.files, &mut files, &mut digest)?;
        let expected_files = checksum.files.keys().cloned().collect::<BTreeSet<_>>();
        if files != expected_files {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        observed.insert(name);
    }
    if observed != expected.keys().cloned().collect() {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn validate_files(
    root: &Path,
    directory: &Path,
    expected: &BTreeMap<String, String>,
    observed: &mut BTreeSet<String>,
    digest: &mut sha2::Sha256,
) -> Result<(), Spec034ReleaseArtifactError> {
    for entry in sorted_entries(directory)? {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        if metadata.is_dir() {
            validate_files(root, &path, expected, observed, digest)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?
            .to_str()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
            .to_owned();
        if relative == ".cargo-checksum.json" {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(Spec034ReleaseArtifactError::Io)?;
        let actual = digest_bytes(&bytes).trim_start_matches("sha256:").to_owned();
        if expected.get(&relative) != Some(&actual) {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        digest.update(relative.as_bytes());
        digest.update(actual.as_bytes());
        observed.insert(relative);
    }
    Ok(())
}

fn sorted_entries(path: &Path) -> Result<Vec<std::fs::DirEntry>, Spec034ReleaseArtifactError> {
    let mut entries = std::fs::read_dir(path)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

fn ambient_cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}
