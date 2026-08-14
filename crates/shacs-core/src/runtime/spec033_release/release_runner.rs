use super::release_runner_model::*;
use super::{collect_spec033_replay_evidence, redact_spec033_artifact, Spec033ReleaseCheck};
use shacs_projection::{
    execute_spec031_release_command, Spec031ReleaseCommandSpec, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind,
};
use std::fs;
use std::path::Path;

mod coverage_catalog;
mod release_artifacts;
mod source_manifest;
use coverage_catalog::{blocker_coverage, coverage, required_blockers};
use release_artifacts::{
    blocked_non_guarantees, collect_digests, reject_symlink, sync_dir, validate_tree, write_json,
    write_summary,
};

pub fn run_spec033_release_runner(
    config: &Spec033ReleaseConfig,
) -> Result<Spec033ReleaseManifest, Spec033ReleaseArtifactError> {
    validate_config(config)?;
    let parent = config
        .evidence_root
        .parent()
        .ok_or(Spec033ReleaseArtifactError::InvalidConfig)?;
    let trajectory = trajectory_provenance(config)?;
    let source_manifest = source_manifest::collect(&config.repo_root)?;
    reject_symlink(parent)?;
    fs::create_dir_all(parent).map_err(Spec033ReleaseArtifactError::Io)?;
    if config.evidence_root.exists() {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    let staging = tempfile::Builder::new()
        .prefix(".spec033-release-")
        .tempdir_in(parent)
        .map_err(Spec033ReleaseArtifactError::Io)?;
    let raw = RawEvidence::create(&config.run_id)?;
    let mut commands = Vec::new();
    for kind in Spec033ReleaseCheck::required() {
        commands.push(run_check(config, kind, raw.path(), staging.path())?);
    }
    let edge_commands = run_edge_checks(config, raw.path(), staging.path())?;
    let replay = collect_spec033_replay_evidence(
        &config.trajectory_root,
        &super::super::spec033_projection::replay_receipt_root(&config.data_dir),
        &config.trajectory_id,
        &config.run_id,
    )
    .map_err(Spec033ReleaseArtifactError::Replay)?;
    write_json(staging.path().join("replay/receipt.json"), &replay)?;
    let coverage = coverage(staging.path(), &commands)?;
    validate_spec033_release_coverage(&coverage)?;
    let blocker_coverage = blocker_coverage(&edge_commands)?;
    write_json(staging.path().join("coverage/spec033.json"), &coverage)?;
    let blocked_non_guarantees = blocked_non_guarantees();
    let mut manifest = Spec033ReleaseManifest {
        schema: SPEC033_RELEASE_SCHEMA.to_owned(),
        run_id: config.run_id.clone(),
        trajectory_id: config.trajectory_id.clone(),
        mode: config.mode,
        trajectory,
        source_manifest,
        commands,
        edge_commands,
        replay,
        coverage,
        blocker_coverage,
        artifact_digests: Vec::new(),
        blocked_non_guarantees,
    };
    write_summary(staging.path(), &manifest)?;
    manifest.artifact_digests = collect_digests(staging.path())?;
    write_json(staging.path().join("manifest.json"), &manifest)?;
    validate_tree(staging.path(), &manifest)?;
    let staged = staging.keep();
    fs::rename(&staged, &config.evidence_root).map_err(Spec033ReleaseArtifactError::Io)?;
    sync_dir(parent)?;
    validate_spec033_release_artifacts_against(&config.evidence_root, &config.repo_root)
}

fn run_edge_checks(
    config: &Spec033ReleaseConfig,
    raw: &Path,
    output: &Path,
) -> Result<Vec<Spec033EdgeCommandEvidence>, Spec033ReleaseArtifactError> {
    required_blockers()
        .iter()
        .map(|edge| {
            let mut record = execute_spec031_release_command(
                &Spec031ReleaseCommandSpec {
                    id: format!("spec033-edge-{}", edge.blocker.to_ascii_lowercase()),
                    gate: Spec031ReleaseGateKind::FocusedCargoTest,
                    package: Some(edge.package.to_owned()),
                    filter: Some(edge.test_id.to_owned()),
                    argv: edge.command(),
                    cwd: config.repo_root.clone(),
                    timeout: config.command_timeout,
                },
                raw,
            )
            .map_err(Spec033ReleaseArtifactError::Command)?;
            if record.status != Spec031ReleaseCommandStatus::Passed
                || record.tests.as_ref().map(|tests| tests.tests_run) != Some(1)
            {
                return Err(Spec033ReleaseArtifactError::CommandFailed);
            }
            let blocker = edge.blocker.to_ascii_lowercase();
            let artifact = format!("edges/{blocker}/stdout.log");
            fs::create_dir_all(output.join(format!("edges/{blocker}")))
                .map_err(Spec033ReleaseArtifactError::Io)?;
            let transform = redact_spec033_artifact(
                &raw.join(&record.stdout_path),
                &output.join(&artifact),
                8 * 1024 * 1024,
            )
            .map_err(Spec033ReleaseArtifactError::Replay)?;
            record.cwd = ".".to_owned();
            record.duration_ms = 0;
            record.process_receipt = None;
            record.stdout_path.clone_from(&artifact);
            record.stderr_path.clear();
            Ok(Spec033EdgeCommandEvidence {
                blocker: edge.blocker.to_owned(),
                test_id: edge.test_id.to_owned(),
                command: record,
                artifact,
                artifact_digest: transform.output_digest,
            })
        })
        .collect()
}

struct RawEvidence {
    _directory: tempfile::TempDir,
    canonical_path: std::path::PathBuf,
}

impl RawEvidence {
    fn create(run_id: &str) -> Result<Self, Spec033ReleaseArtifactError> {
        let directory = tempfile::Builder::new()
            .prefix(&format!("spec033-raw-{run_id}-"))
            .tempdir()
            .map_err(Spec033ReleaseArtifactError::Io)?;
        let canonical_path = directory
            .path()
            .canonicalize()
            .map_err(Spec033ReleaseArtifactError::Io)?;
        Ok(Self {
            _directory: directory,
            canonical_path,
        })
    }

    fn path(&self) -> &Path {
        &self.canonical_path
    }
}

pub fn validate_spec033_release_coverage(
    coverage: &[Spec033CoverageRow],
) -> Result<(), Spec033ReleaseArtifactError> {
    coverage_catalog::validate_coverage(coverage)
}

pub fn validate_spec033_release_artifacts(
    root: &Path,
) -> Result<Spec033ReleaseManifest, Spec033ReleaseArtifactError> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    validate_spec033_release_artifacts_against(root, &repo)
}

pub fn validate_spec033_release_artifacts_against(
    root: &Path,
    repo_root: &Path,
) -> Result<Spec033ReleaseManifest, Spec033ReleaseArtifactError> {
    let bytes = fs::read(root.join("manifest.json")).map_err(Spec033ReleaseArtifactError::Io)?;
    let manifest = serde_json::from_slice(&bytes).map_err(Spec033ReleaseArtifactError::Json)?;
    validate_tree(root, &manifest)?;
    if source_manifest::collect(repo_root)? != manifest.source_manifest {
        return Err(Spec033ReleaseArtifactError::DigestMismatch);
    }
    Ok(manifest)
}

fn run_check(
    config: &Spec033ReleaseConfig,
    kind: Spec033ReleaseCheck,
    raw: &Path,
    output: &Path,
) -> Result<Spec033ReleaseCommandEvidence, Spec033ReleaseArtifactError> {
    let id = format!("spec033-{kind:?}").to_ascii_lowercase();
    let mut argv = vec!["cargo".to_owned()];
    argv.extend(kind.cargo_args());
    let mut record = execute_spec031_release_command(
        &Spec031ReleaseCommandSpec {
            id,
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: record_package(kind),
            filter: None,
            argv,
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
        raw,
    )
    .map_err(Spec033ReleaseArtifactError::Command)?;
    if record.status != Spec031ReleaseCommandStatus::Passed
        || record
            .tests
            .as_ref()
            .map_or(true, |tests| tests.tests_run == 0)
    {
        return Err(Spec033ReleaseArtifactError::CommandFailed);
    }
    let directory = output.join(format!("gates/{kind:?}").to_ascii_lowercase());
    fs::create_dir_all(&directory).map_err(Spec033ReleaseArtifactError::Io)?;
    let raw_stdout = raw.join(&record.stdout_path);
    let raw_stderr = raw.join(&record.stderr_path);
    let redacted_stdout = format!("gates/{kind:?}/stdout.log").to_ascii_lowercase();
    let redacted_stderr = format!("gates/{kind:?}/stderr.log").to_ascii_lowercase();
    let stdout_transform =
        redact_spec033_artifact(&raw_stdout, &output.join(&redacted_stdout), 8 * 1024 * 1024)
            .map_err(Spec033ReleaseArtifactError::Replay)?;
    let stderr_transform =
        redact_spec033_artifact(&raw_stderr, &output.join(&redacted_stderr), 8 * 1024 * 1024)
            .map_err(Spec033ReleaseArtifactError::Replay)?;
    record.cwd = ".".to_owned();
    record.duration_ms = 0;
    record.process_receipt = None;
    record.stdout_path.clone_from(&redacted_stdout);
    record.stderr_path.clone_from(&redacted_stderr);
    let evidence = Spec033ReleaseCommandEvidence {
        kind,
        command: record,
        stdout_digest: stdout_transform.source_digest.clone(),
        stderr_digest: stderr_transform.source_digest.clone(),
        redacted_stdout,
        redacted_stderr,
        stdout_transform,
        stderr_transform,
    };
    write_json(directory.join("receipt.json"), &evidence)?;
    Ok(evidence)
}

fn record_package(kind: Spec033ReleaseCheck) -> Option<String> {
    match kind {
        Spec033ReleaseCheck::ReviewArtifacts => Some("shacs-projection".to_owned()),
        Spec033ReleaseCheck::AutomationDispatch
        | Spec033ReleaseCheck::GoalAccounting
        | Spec033ReleaseCheck::SnapshotReplay
        | Spec033ReleaseCheck::SelfImprovement => Some("shacs-core".to_owned()),
    }
}

fn validate_config(config: &Spec033ReleaseConfig) -> Result<(), Spec033ReleaseArtifactError> {
    let valid = [&config.run_id, &config.trajectory_id].iter().all(|value| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    });
    valid
        .then_some(())
        .ok_or(Spec033ReleaseArtifactError::InvalidConfig)
}

fn trajectory_provenance(
    config: &Spec033ReleaseConfig,
) -> Result<Spec033TrajectoryProvenance, Spec033ReleaseArtifactError> {
    let store = super::RecordedTrajectoryStore::open(&config.trajectory_root).map_err(|error| {
        Spec033ReleaseArtifactError::Replay(super::Spec033ReleaseEvidenceError::Store(error))
    })?;
    let record = store.read(&config.trajectory_id).map_err(|error| {
        Spec033ReleaseArtifactError::Replay(super::Spec033ReleaseEvidenceError::Store(error))
    })?;
    let source = record
        .sources
        .iter()
        .find(|source| source.source_id == "automation-instruction");
    if config.mode == Spec033ReleaseMode::CurrentWorktree
        && (record.origin != crate::runtime::RecordedTrajectoryOrigin::AutomationOwnerReceipt
            || config.trajectory_id.starts_with("trajectory-")
            || record.sources.len() != 1
            || source.is_none()
            || record.owner_outcome.diagnostics_refs.is_empty()
            || record.owner_outcome.actual_verdict.is_none()
            || record.owner_outcome.actual_outcome.is_none()
            || record.owner_outcome.actual_projection_status.is_none())
    {
        return Err(Spec033ReleaseArtifactError::InvalidConfig);
    }
    Ok(Spec033TrajectoryProvenance {
        record_path: format!("trajectories/{}/record.json", config.trajectory_id),
        record_digest: record.record_digest,
        source_id: source
            .map(|source| source.source_id.clone())
            .unwrap_or_default(),
        origin: record.origin,
    })
}
