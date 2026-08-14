use super::*;
use sha2::{Digest, Sha256};
use std::path::Component;
use std::path::PathBuf;

pub(super) fn validate_tree(
    root: &Path,
    manifest: &Spec033ReleaseManifest,
) -> Result<(), Spec033ReleaseArtifactError> {
    let root_metadata = fs::symlink_metadata(root).map_err(Spec033ReleaseArtifactError::Io)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    let canonical_root = root
        .canonicalize()
        .map_err(Spec033ReleaseArtifactError::Io)?;
    if manifest.schema != SPEC033_RELEASE_SCHEMA
        || manifest.commands.len() != 5
        || manifest.edge_commands.len() != 17
        || manifest.replay.result.status != shacs_eval::evaluator::ReplayRunStatus::Passed
        || manifest.replay.compared_recorded_outcomes == 0
        || manifest.replay.trajectory_id != manifest.trajectory_id
        || manifest.replay.result.diagnostics_ref.digest != manifest.trajectory.record_digest
        || manifest.trajectory.record_path.is_empty()
        || Path::new(&manifest.trajectory.record_path).is_absolute()
        || Path::new(&manifest.trajectory.record_path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || (manifest.mode == Spec033ReleaseMode::CurrentWorktree
            && (manifest.trajectory.origin
                != crate::runtime::RecordedTrajectoryOrigin::AutomationOwnerReceipt
                || manifest.trajectory_id.starts_with("trajectory-")
                || manifest.trajectory.source_id != "automation-instruction"
                || manifest
                    .replay
                    .result
                    .case_results
                    .iter()
                    .any(|result| result.diagnostics_refs.is_empty())))
        || (manifest.trajectory.origin
            == crate::runtime::RecordedTrajectoryOrigin::AutomationOwnerReceipt
            && (manifest.trajectory_id.starts_with("trajectory-")
                || manifest.trajectory.source_id != "automation-instruction"
                || manifest
                    .replay
                    .result
                    .case_results
                    .iter()
                    .any(|result| result.diagnostics_refs.is_empty())))
        || super::coverage_catalog::validate_coverage(&manifest.coverage).is_err()
        || super::coverage_catalog::validate_blocker_coverage(&manifest.blocker_coverage).is_err()
        || manifest.blocked_non_guarantees != blocked_non_guarantees()
    {
        return Err(Spec033ReleaseArtifactError::MissingGuarantee);
    }
    for edge in &manifest.edge_commands {
        let artifact = validated_artifact(root, &canonical_root, &edge.artifact)?;
        let expected = super::coverage_catalog::required_blockers()
            .into_iter()
            .find(|expected| expected.blocker == edge.blocker)
            .ok_or(Spec033ReleaseArtifactError::MissingGuarantee)?;
        if edge.test_id != expected.test_id
            || edge.command.argv != expected.command()
            || edge.command.status != Spec031ReleaseCommandStatus::Passed
            || edge.command.tests.as_ref().map(|tests| tests.tests_run) != Some(1)
            || digest_file(&artifact)? != edge.artifact_digest
        {
            return Err(Spec033ReleaseArtifactError::DigestMismatch);
        }
    }
    let actual_digests = collect_digests(root)?;
    if actual_digests != manifest.artifact_digests {
        return Err(Spec033ReleaseArtifactError::DigestMismatch);
    }
    for command in &manifest.commands {
        let stdout = validated_artifact(root, &canonical_root, &command.redacted_stdout)?;
        let stderr = validated_artifact(root, &canonical_root, &command.redacted_stderr)?;
        let expected_argv = std::iter::once("cargo".to_owned())
            .chain(command.kind.cargo_args())
            .collect::<Vec<_>>();
        if command.command.status != Spec031ReleaseCommandStatus::Passed
            || command.command.argv != expected_argv
            || command.command.id != format!("spec033-{:?}", command.kind).to_ascii_lowercase()
            || command.stdout_transform.schema != super::super::SPEC033_REDACTION_TRANSFORM_SCHEMA
            || command.stderr_transform.schema != super::super::SPEC033_REDACTION_TRANSFORM_SCHEMA
            || command.stdout_digest != command.stdout_transform.source_digest
            || command.stderr_digest != command.stderr_transform.source_digest
            || digest_file(&stdout)? != command.stdout_transform.output_digest
            || digest_file(&stderr)? != command.stderr_transform.output_digest
        {
            return Err(Spec033ReleaseArtifactError::DigestMismatch);
        }
    }
    for row in &manifest.artifact_digests {
        let artifact = validated_artifact(root, &canonical_root, &row.locator)?;
        if digest_file(&artifact)? != row.digest {
            return Err(Spec033ReleaseArtifactError::DigestMismatch);
        }
    }
    Ok(())
}

pub(super) fn collect_digests(
    root: &Path,
) -> Result<Vec<Spec033DigestRow>, Spec033ReleaseArtifactError> {
    let mut rows = Vec::new();
    collect_dir(root, root, &mut rows)?;
    rows.sort_by(|left, right| left.locator.cmp(&right.locator));
    Ok(rows)
}

fn collect_dir(
    root: &Path,
    dir: &Path,
    rows: &mut Vec<Spec033DigestRow>,
) -> Result<(), Spec033ReleaseArtifactError> {
    for entry in fs::read_dir(dir).map_err(Spec033ReleaseArtifactError::Io)? {
        let entry = entry.map_err(Spec033ReleaseArtifactError::Io)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(Spec033ReleaseArtifactError::Io)?;
        if file_type.is_symlink() {
            return Err(Spec033ReleaseArtifactError::InvalidConfig);
        }
        if file_type.is_dir() {
            collect_dir(root, &path, rows)?;
        } else if file_type.is_file()
            && path.file_name().is_some_and(|name| name != "manifest.json")
        {
            let locator = path
                .strip_prefix(root)
                .map_err(|_| Spec033ReleaseArtifactError::InvalidConfig)?;
            rows.push(Spec033DigestRow {
                locator: locator.to_string_lossy().into_owned(),
                digest: digest_file(&path)?,
            });
        } else if !file_type.is_file() {
            return Err(Spec033ReleaseArtifactError::InvalidConfig);
        }
    }
    Ok(())
}

fn validated_artifact(
    root: &Path,
    canonical_root: &Path,
    locator: &str,
) -> Result<PathBuf, Spec033ReleaseArtifactError> {
    let relative = Path::new(locator);
    if locator.is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    let artifact = root.join(relative);
    let metadata = fs::symlink_metadata(&artifact).map_err(Spec033ReleaseArtifactError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    let canonical_artifact = artifact
        .canonicalize()
        .map_err(Spec033ReleaseArtifactError::Io)?;
    if !canonical_artifact.starts_with(canonical_root) {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    Ok(canonical_artifact)
}

pub(super) fn write_json(
    path: PathBuf,
    value: &impl serde::Serialize,
) -> Result<(), Spec033ReleaseArtifactError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(Spec033ReleaseArtifactError::Io)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(Spec033ReleaseArtifactError::Json)?;
    fs::write(path, bytes).map_err(Spec033ReleaseArtifactError::Io)
}

pub(super) fn write_summary(
    root: &Path,
    manifest: &Spec033ReleaseManifest,
) -> Result<(), Spec033ReleaseArtifactError> {
    let commands = manifest
        .commands
        .iter()
        .map(|command| format!("- `{}`: passed", command.command.argv.join(" ")))
        .chain(manifest.edge_commands.iter().map(|edge| {
            format!(
                "- `{}`: passed ({})",
                edge.command.argv.join(" "),
                edge.blocker
            )
        }))
        .collect::<Vec<_>>()
        .join("\n");
    let artifacts = manifest
        .commands
        .iter()
        .flat_map(|command| {
            [
                (
                    &command.redacted_stdout,
                    &command.stdout_transform.output_digest,
                ),
                (
                    &command.redacted_stderr,
                    &command.stderr_transform.output_digest,
                ),
            ]
        })
        .chain(
            manifest
                .edge_commands
                .iter()
                .map(|edge| (&edge.artifact, &edge.artifact_digest)),
        )
        .map(|(locator, digest)| format!("- `{locator}`: `{digest}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let non_guarantees = manifest
        .blocked_non_guarantees
        .iter()
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = format!(
        "# Spec033 Release Closure\n\nrun: `{}`\ntrajectory: `{}`\n\n## Commands\n{}\n\n## Artifacts\n{}\n\n## Failures\n- none\n\n## Disclosure\n- persisted command output is redacted; raw temporary evidence is not retained\n\n## Cleanup\n- raw evidence is RAII-cleaned and staging was atomically published\n\n## Non-guarantees\n{}\n",
        manifest.run_id, manifest.trajectory_id, commands, artifacts, non_guarantees
    );
    fs::write(root.join("summary.md"), summary).map_err(Spec033ReleaseArtifactError::Io)
}

pub(super) fn blocked_non_guarantees() -> Vec<String> {
    vec![
        "recorded replay is not current authorization truth".to_owned(),
        "local owner receipt origin is not cryptographic authentication".to_owned(),
        "bounded release transcripts do not prove complete runtime redaction".to_owned(),
        "local Cargo evidence does not prove external delivery guarantees".to_owned(),
    ]
}

pub(super) fn digest_file(path: &Path) -> Result<String, Spec033ReleaseArtifactError> {
    let bytes = fs::read(path).map_err(Spec033ReleaseArtifactError::Io)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), Spec033ReleaseArtifactError> {
    if path.exists()
        && fs::symlink_metadata(path)
            .map_err(Spec033ReleaseArtifactError::Io)?
            .file_type()
            .is_symlink()
    {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    Ok(())
}

pub(super) fn sync_dir(path: &Path) -> Result<(), Spec033ReleaseArtifactError> {
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(Spec033ReleaseArtifactError::Io)
}
