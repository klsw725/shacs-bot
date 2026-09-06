use super::*;

impl ResolvedTool {
    pub(in crate::runtime::spec034_release) fn resolve_for_test(
        name: &str,
        candidates: Vec<PathBuf>,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::resolve(name, candidates)
    }
}

impl ResolvedToolchain {
    pub(super) fn cargo_home_path(&self) -> &Path {
        &self.cargo_home
    }

    pub(in crate::runtime::spec034_release) fn resolve_tools_for_test(
        home: PathBuf,
        cargo_home: PathBuf,
        target: PathBuf,
        cargo: PathBuf,
        rustc: PathBuf,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let rustdoc = option_env!("CARGO")
            .map(PathBuf::from)
            .and_then(|cargo| cargo.parent().map(|parent| parent.join("rustdoc")))
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        let cargo = ResolvedTool::resolve("cargo", vec![cargo])?;
        let rustc = ResolvedTool::resolve("rustc", vec![rustc])?;
        let rustdoc = ResolvedTool::resolve("rustdoc", vec![rustdoc])?;
        let ledger = ExecutionLedger::arm(&[
            cargo.path.clone(),
            rustc.path.clone(),
            rustdoc.path.clone(),
        ])?;
        let linker_path = linker::fixed_linker()?;
        let linker_seals = vec![PathChainSeal::capture_digest_leaf(&linker_path)?];
        let linker_identity = spawn::capture_process_identity(
            i32::try_from(std::process::id())
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
        )?;
        Self::from_resolved(
            (home, cargo_home, target),
            (cargo, rustc, rustdoc),
            ledger,
            None,
            linker_path,
            linker_seals,
            linker_identity,
        )
    }
}
