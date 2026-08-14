use super::*;
use sha2::{Digest, Sha256};
use std::path::Component;

const SOURCE_ROOTS: [&str; 4] = ["README.md", "crates/Cargo.lock", "crates", "docs"];
const EXCLUDED_TOOLING_ROOTS: [&str; 2] = ["docs/refs", "docs/specs/.codegraph"];

pub(super) fn collect(
    repo_root: &Path,
) -> Result<Spec033SourceManifest, Spec033ReleaseArtifactError> {
    let mut files = Vec::new();
    for source in SOURCE_ROOTS {
        let path = repo_root.join(source);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Spec033ReleaseArtifactError::InvalidConfig
            } else {
                Spec033ReleaseArtifactError::Io(error)
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Spec033ReleaseArtifactError::InvalidConfig);
        }
        if metadata.is_dir() {
            collect_dir(repo_root, &path, &mut files)?;
        } else if metadata.is_file() {
            push_file(repo_root, &path, &mut files)?;
        } else {
            return Err(Spec033ReleaseArtifactError::InvalidConfig);
        }
    }
    files.sort_by(|left, right| left.locator.cmp(&right.locator));
    files.dedup_by(|left, right| left.locator == right.locator);
    let mut digest = Sha256::new();
    for file in &files {
        digest.update(file.locator.as_bytes());
        digest.update([0]);
        digest.update(file.digest.as_bytes());
        digest.update(b"\n");
    }
    Ok(Spec033SourceManifest {
        digest: format!("sha256:{:x}", digest.finalize()),
        files,
    })
}

fn collect_dir(
    repo_root: &Path,
    directory: &Path,
    files: &mut Vec<Spec033DigestRow>,
) -> Result<(), Spec033ReleaseArtifactError> {
    for entry in std::fs::read_dir(directory).map_err(Spec033ReleaseArtifactError::Io)? {
        let entry = entry.map_err(Spec033ReleaseArtifactError::Io)?;
        let path = entry.path();
        if EXCLUDED_TOOLING_ROOTS
            .iter()
            .any(|root| path.starts_with(repo_root.join(root)))
        {
            continue;
        }
        let file_type = entry.file_type().map_err(Spec033ReleaseArtifactError::Io)?;
        if file_type.is_symlink() {
            return Err(Spec033ReleaseArtifactError::InvalidConfig);
        }
        if file_type.is_dir() {
            if entry.file_name() != "target" {
                collect_dir(repo_root, &path, files)?;
            }
        } else if file_type.is_file() && source_file(&path) {
            push_file(repo_root, &path, files)?;
        }
    }
    Ok(())
}

fn push_file(
    repo_root: &Path,
    path: &Path,
    files: &mut Vec<Spec033DigestRow>,
) -> Result<(), Spec033ReleaseArtifactError> {
    let relative = path
        .strip_prefix(repo_root)
        .map_err(|_| Spec033ReleaseArtifactError::InvalidConfig)?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    files.push(Spec033DigestRow {
        locator: relative.to_string_lossy().replace('\\', "/"),
        digest: release_artifacts::digest_file(path)?,
    });
    Ok(())
}

fn source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("rs" | "toml" | "lock" | "md" | "json" | "yml" | "yaml" | "sh" | "py")
    ) || path.file_name().is_some_and(|name| name == "Cargo.lock")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn collect_rejects_symlink_source_file() -> Result<(), Box<dyn std::error::Error>> {
        // Given
        let root = tempfile::tempdir()?;
        write_source_fixture(root.path())?;
        std::fs::write(root.path().join("outside.md"), b"outside")?;
        std::fs::remove_file(root.path().join("README.md"))?;
        std::os::unix::fs::symlink(
            root.path().join("outside.md"),
            root.path().join("README.md"),
        )?;

        // When
        let result = collect(root.path());

        // Then
        assert!(matches!(
            result,
            Err(Spec033ReleaseArtifactError::InvalidConfig)
        ));
        Ok(())
    }

    #[test]
    fn collect_rejects_symlink_source_directory() -> Result<(), Box<dyn std::error::Error>> {
        // Given
        let root = tempfile::tempdir()?;
        write_source_fixture(root.path())?;
        let outside = root.path().join("outside-source");
        std::fs::create_dir(&outside)?;
        std::fs::write(outside.join("linked.rs"), b"pub fn linked() {}")?;
        std::os::unix::fs::symlink(&outside, root.path().join("crates/linked"))?;

        // When
        let result = collect(root.path());

        // Then
        assert!(matches!(
            result,
            Err(Spec033ReleaseArtifactError::InvalidConfig)
        ));
        Ok(())
    }

    #[test]
    fn collect_rejects_retargeted_docs_symlink() -> Result<(), Box<dyn std::error::Error>> {
        // Given
        let root = tempfile::tempdir()?;
        write_source_fixture(root.path())?;
        let docs_link = root.path().join("docs/guide.md");
        let first_target = root.path().join("first.md");
        let second_target = root.path().join("second.md");
        std::fs::write(&first_target, b"first")?;
        std::fs::write(&second_target, b"second")?;
        std::os::unix::fs::symlink(&first_target, &docs_link)?;

        // When
        let first_result = collect(root.path());
        std::fs::remove_file(&docs_link)?;
        std::os::unix::fs::symlink(&second_target, &docs_link)?;
        let retargeted_result = collect(root.path());

        // Then
        assert!(matches!(
            first_result,
            Err(Spec033ReleaseArtifactError::InvalidConfig)
        ));
        assert!(matches!(
            retargeted_result,
            Err(Spec033ReleaseArtifactError::InvalidConfig)
        ));
        Ok(())
    }

    #[test]
    fn collect_excludes_generated_docs_symlink_roots() -> Result<(), Box<dyn std::error::Error>> {
        // Given
        let root = tempfile::tempdir()?;
        write_source_fixture(root.path())?;
        std::fs::create_dir_all(root.path().join("docs/specs"))?;
        let tooling = root.path().join("tooling");
        std::fs::create_dir(&tooling)?;
        std::os::unix::fs::symlink(&tooling, root.path().join("docs/refs"))?;
        std::os::unix::fs::symlink(&tooling, root.path().join("docs/specs/.codegraph"))?;

        // When
        let manifest = collect(root.path())?;

        // Then
        assert!(manifest.files.iter().all(|file| {
            !file.locator.starts_with("docs/refs/")
                && !file.locator.starts_with("docs/specs/.codegraph/")
        }));
        Ok(())
    }

    fn write_source_fixture(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir(root.join("crates"))?;
        std::fs::create_dir(root.join("docs"))?;
        std::fs::write(root.join("README.md"), b"fixture")?;
        std::fs::write(root.join("crates/Cargo.lock"), b"fixture")?;
        Ok(())
    }
}
