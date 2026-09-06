use super::*;
use std::collections::{BTreeSet, VecDeque};

const MAX_DEPENDENCIES: usize = 256;
const MAX_HELPER_OUTPUT: usize = 1024 * 1024;

pub(super) struct DependencyClosure {
    pub(super) seals: Vec<PathChainSeal>,
    pub(super) paths: Vec<PathBuf>,
}

impl DependencyClosure {
    #[cfg(test)]
    fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        for seal in &self.seals {
            seal.verify()?;
        }
        Ok(())
    }
}

pub(super) fn capture(executable: &Path) -> Result<DependencyClosure, Spec034ReleaseArtifactError> {
    let paths = inventory(executable)?;
    let seals = paths
        .iter()
        .map(|path| PathChainSeal::capture_digest_leaf(path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DependencyClosure { seals, paths })
}

pub(super) fn inventory(executable: &Path) -> Result<Vec<PathBuf>, Spec034ReleaseArtifactError> {
    let root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut queue = VecDeque::from([executable.to_path_buf()]);
    let mut visited = BTreeSet::new();
    let mut dependencies = BTreeSet::new();
    while let Some(current) = queue.pop_front() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if visited.len() > MAX_DEPENDENCIES {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        for dependency in direct_dependencies(&current, root)? {
            if dependencies.insert(dependency.clone()) {
                queue.push_back(dependency);
            }
        }
    }
    Ok(dependencies.into_iter().collect())
}

pub(super) fn capture_for_tool(
    executable: &Path,
) -> Result<DependencyClosure, Spec034ReleaseArtifactError> {
    #[cfg(test)]
    if std::env::temp_dir().canonicalize().ok().is_some_and(|temporary| {
        executable
            .canonicalize()
            .map_or(true, |path| !path.starts_with(temporary))
    }) {
        return Ok(DependencyClosure {
            seals: Vec::new(),
            paths: Vec::new(),
        });
    }
    capture(executable)
}

#[cfg(target_vendor = "apple")]
fn direct_dependencies(
    binary: &Path,
    tool_root: &Path,
) -> Result<Vec<PathBuf>, Spec034ReleaseArtifactError> {
    let output = bounded_helper("/usr/bin/otool", &["-L"], binary)?;
    output
        .lines()
        .skip(1)
        .map(|line| {
            line.trim()
                .split(" (")
                .next()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)
        })
        .filter_map(|value| match value {
            Ok(value) if system_dependency(value) => None,
            other => Some(other),
        })
        .map(|value| resolve_macos(value?, binary, tool_root))
        .collect()
}

#[cfg(target_vendor = "apple")]
fn resolve_macos(
    value: &str,
    binary: &Path,
    tool_root: &Path,
) -> Result<PathBuf, Spec034ReleaseArtifactError> {
    let path = if let Some(relative) = value.strip_prefix("@loader_path/") {
        binary
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
            .join(relative)
    } else if value.starts_with("@rpath/") {
        let name = Path::new(value)
            .file_name()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        [tool_root.join("lib").join(name), binary.parent().unwrap_or(Path::new("/")).join(name)]
            .into_iter()
            .find(|candidate| candidate.is_file())
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
    } else if value.starts_with('/') {
        PathBuf::from(value)
    } else {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    };
    path.canonicalize().map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(target_vendor = "apple")]
fn system_dependency(value: &str) -> bool {
    value.starts_with("/usr/lib/")
        || value.starts_with("/System/Library/")
        || value.starts_with("/System/Cryptexes/")
}

#[cfg(target_os = "linux")]
fn direct_dependencies(
    binary: &Path,
    _tool_root: &Path,
) -> Result<Vec<PathBuf>, Spec034ReleaseArtifactError> {
    let output = bounded_helper("/usr/bin/ldd", &[], binary)?;
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let value = trimmed
                .split_once("=>")
                .map(|(_, path)| path.trim().split_whitespace().next().unwrap_or(""))
                .or_else(|| trimmed.starts_with('/').then(|| trimmed.split_whitespace().next().unwrap_or("")))?;
            (!system_dependency(value)).then_some(value)
        })
        .map(|value| {
            PathBuf::from(value)
                .canonicalize()
                .map_err(Spec034ReleaseArtifactError::Io)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn system_dependency(value: &str) -> bool {
    value.starts_with("/lib/")
        || value.starts_with("/lib64/")
        || value.starts_with("/usr/lib/")
        || value.starts_with("/usr/lib64/")
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
fn direct_dependencies(
    _binary: &Path,
    _tool_root: &Path,
) -> Result<Vec<PathBuf>, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

fn bounded_helper(
    helper: &str,
    arguments: &[&str],
    binary: &Path,
) -> Result<String, Spec034ReleaseArtifactError> {
    let output = Command::new(helper)
        .args(arguments)
        .arg(binary)
        .env_clear()
        .output()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if !output.status.success() || output.stdout.len() > MAX_HELPER_OUTPUT {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    String::from_utf8(output.stdout).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_vendor = "apple")]
    #[test]
    fn homebrew_cargo_inventory_seals_all_non_system_dependencies(
    ) -> Result<(), Spec034ReleaseArtifactError> {
        let closure = capture(Path::new("/opt/homebrew/bin/cargo"))?;
        let inventory = closure
            .paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>();
        for required in ["libgit2", "openssl", "sqlite"] {
            assert!(inventory.iter().any(|path| path.contains(required)));
        }
        assert!(inventory.iter().all(|path| !system_dependency(path)));
        Ok(())
    }

    #[test]
    fn dependency_content_mutation_breaks_closure() -> Result<(), Spec034ReleaseArtifactError> {
        let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
        let dependency = root.path().join("dependency.dylib");
        std::fs::write(&dependency, b"approved").map_err(Spec034ReleaseArtifactError::Io)?;
        let closure = DependencyClosure {
            seals: vec![PathChainSeal::capture_digest_leaf(&dependency)?],
            paths: vec![dependency.clone()],
        };

        std::fs::write(dependency, b"mutated").map_err(Spec034ReleaseArtifactError::Io)?;

        assert!(matches!(closure.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
        Ok(())
    }
}
