use super::{dependencies, File, Path, PathBuf, Spec034ReleaseArtifactError};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;
use std::process::Command;
use std::sync::OnceLock;

pub(in crate::runtime::spec034_release) fn tool_cache_key(
) -> Result<String, Spec034ReleaseArtifactError> {
    static CACHE_KEY: OnceLock<String> = OnceLock::new();
    if let Some(key) = CACHE_KEY.get() {
        return Ok(key.clone());
    }
    let (cargo, rustc, rustdoc) = rust_tool_candidates();
    let mut digest = Sha256::new();
    digest.update(b"spec034.tool-cache.v2\0");
    digest.update(if cfg!(test) { &b"test\0"[..] } else { &b"release\0"[..] });
    let mut dependencies = BTreeSet::new();
    for (role, candidates) in [("cargo", cargo), ("rustc", rustc), ("rustdoc", rustdoc)] {
        let path = candidates
            .into_iter()
            .find(|path| path.is_file())
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        digest.update(role.as_bytes());
        digest.update([0]);
        hash_file_into(&mut digest, &path)?;
        dependencies.extend(dependencies::inventory(&path)?);
    }
    for dependency in dependencies {
        digest.update(dependency.as_os_str().as_encoded_bytes());
        digest.update([0]);
        hash_file_into(&mut digest, &dependency)?;
    }
    let git = ["/opt/homebrew/bin/git", "/usr/bin/git"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    hash_file_into(&mut digest, &git)?;
    let key = format!("{:x}", digest.finalize());
    let _ = CACHE_KEY.set(key.clone());
    Ok(key)
}

fn hash_file_into(
    digest: &mut Sha256,
    path: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    let mut file = File::open(path).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(Spec034ReleaseArtifactError::Io)?;
        if read == 0 {
            return Ok(());
        }
        digest.update(&buffer[..read]);
    }
}

pub(super) fn rust_tool_candidates() -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    let rustup = ["cargo", "rustc", "rustdoc"].map(rustup_which);
    if let [Some(cargo), Some(rustc), Some(rustdoc)] = rustup {
        return (vec![cargo], vec![rustc], vec![rustdoc]);
    }
    let mut cargo = Vec::new();
    if let Some(path) = option_env!("CARGO") {
        cargo.push(PathBuf::from(path));
    }
    cargo.extend(["/opt/homebrew/bin/cargo", "/usr/bin/cargo"].map(PathBuf::from));
    let rustc = cargo
        .iter()
        .filter_map(|path| path.parent().map(|parent| parent.join("rustc")))
        .collect();
    let rustdoc = cargo
        .iter()
        .filter_map(|path| path.parent().map(|parent| parent.join("rustdoc")))
        .collect();
    (cargo, rustc, rustdoc)
}

fn rustup_which(name: &str) -> Option<PathBuf> {
    let output = Command::new("rustup")
        .args(["which", name])
        .env_clear()
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 4096 {
        return None;
    }
    let path = PathBuf::from(std::str::from_utf8(&output.stdout).ok()?.trim());
    path.is_file().then_some(path)
}
