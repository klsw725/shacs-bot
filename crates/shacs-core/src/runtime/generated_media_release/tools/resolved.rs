use super::*;

impl ResolvedTool {
    #[cfg(test)]
    pub fn cargo() -> Result<Self, Spec034ReleaseArtifactError> {
        let mut candidates = Vec::new();
        if let Some(path) = option_env!("CARGO") {
            candidates.push(PathBuf::from(path));
        }
        candidates.extend(["/opt/homebrew/bin/cargo", "/usr/bin/cargo"].map(PathBuf::from));
        Self::resolve("cargo", candidates)
    }

    pub fn git() -> Result<Self, Spec034ReleaseArtifactError> {
        Self::resolve(
            "git",
            ["/opt/homebrew/bin/git", "/usr/bin/git"]
                .map(PathBuf::from)
                .to_vec(),
        )
    }

    pub(super) fn resolve(
        name: &str,
        candidates: Vec<PathBuf>,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let (control, root) = controlled_temp_root()?;
        let tools = root.path().join("tools");
        std::fs::create_dir(&tools).map_err(Spec034ReleaseArtifactError::Io)?;
        let mut tool = Self::resolve_into(name, candidates, &tools)?;
        tool._root = Some(root);
        tool._control = Some(control);
        Ok(tool)
    }

    pub(super) fn resolve_into(
        name: &str,
        candidates: Vec<PathBuf>,
        tools: &Path,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let original = candidates
            .into_iter()
            .find(|path| path.is_absolute() && path.is_file())
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let descriptor = rustix::fs::open(
            &original,
            rustix::fs::OFlags::RDONLY
                .union(rustix::fs::OFlags::NOFOLLOW)
                .union(rustix::fs::OFlags::CLOEXEC),
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let mut source: File = descriptor.into();
        let metadata = source.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if !metadata.is_file() || metadata.len() > MAX_TOOL_BYTES {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut source)
            .take(MAX_TOOL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let after = source.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if !same_file_snapshot(&metadata, &after) || bytes.len() as u64 != metadata.len() {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let path = tools.join(name);
        match std::fs::read(&path) {
            Ok(existing) if existing == bytes => {}
            Ok(_) => return Err(Spec034ReleaseArtifactError::DigestMismatch),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut copied = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(Spec034ReleaseArtifactError::Io)?;
                copied.write_all(&bytes).map_err(Spec034ReleaseArtifactError::Io)?;
                copied.sync_all().map_err(Spec034ReleaseArtifactError::Io)?;
                preserve_executable_mode(&path, &metadata)?;
            }
            Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
        }
        let dependencies = super::dependencies::capture_for_tool(&original)?;
        let mut runtime_seals = super::runtime_libraries::prepare(name, &original, tools)?;
        runtime_seals.extend(dependencies.seals);
        let output = minimal_command(&path, Path::new("."))
            .arg("--version")
            .output()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if !output.status.success() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let version = String::from_utf8(output.stdout)
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?
            .trim()
            .to_owned();
        if version.len() > 128 || !version.starts_with(name) {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        Ok(Self {
            seal: PathChainSeal::capture_leaf(&path)?,
            runtime_seals,
            runtime_inventory: dependencies.paths,
            path,
            identity: PortableToolIdentity {
                name: name.to_owned(),
                version,
                executable_digest: digest_bytes(&bytes),
            },
            _root: None,
            _control: None,
        })
    }

    pub fn command(&self, cwd: &Path) -> Command {
        minimal_command(&self.path, cwd)
    }

    pub fn identity(&self) -> &PortableToolIdentity {
        &self.identity
    }

    #[cfg(test)]
    pub(in crate::runtime::generated_media_release) fn path_for_test(&self) -> &Path {
        &self.path
    }

    pub(in crate::runtime::generated_media_release) fn verify(
        &self,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.seal.verify()?;
        for seal in &self.runtime_seals {
            seal.verify()?;
        }
        Ok(())
    }

    pub(super) fn reseal(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        self.seal = PathChainSeal::capture_digest_leaf(&self.path)?;
        for seal in &mut self.runtime_seals {
            seal.reseal()?;
        }
        Ok(())
    }

}

#[cfg(unix)]
pub(super) fn same_file_snapshot(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
        && left.size() == right.size()
}

#[cfg(not(unix))]
pub(super) fn same_file_snapshot(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

#[cfg(unix)]
fn preserve_executable_mode(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(metadata.mode() & 0o777))
        .map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(not(unix))]
fn preserve_executable_mode(
    _path: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<(), Spec034ReleaseArtifactError> {
    Ok(())
}
