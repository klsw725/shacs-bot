use super::super::artifacts::{digest_file, write_json};
use super::super::catalog;
use super::super::model::*;
use super::super::source;
use shacs_projection::{
    execute_spec031_release_command, Spec031ReleaseCommandSpec, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind,
};
use std::path::Path;

struct CommandSpec {
    kind: &'static str,
    package: &'static str,
    target: &'static str,
}

pub fn run_results(
    config: &Spec034ReleaseConfig,
    output: &Path,
) -> Result<ResultsDocument, Spec034ReleaseArtifactError> {
    let output = output
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let specs = [
        CommandSpec {
            kind: "schema-contract",
            package: "shacs-projection",
            target: "spec034_evidence_schema",
        },
        CommandSpec {
            kind: "sequential-integration",
            package: "shacs-core",
            target: "spec034_sequential_integration",
        },
    ];
    let commands = specs
        .into_iter()
        .map(|spec| run_command(config, &output, spec))
        .collect::<Result<Vec<_>, _>>()?;
    if commands.iter().any(|command| !command_passed(command)) {
        return Err(Spec034ReleaseArtifactError::CommandFailed);
    }
    Ok(ResultsDocument {
        schema: "spec034.results.v1".to_owned(),
        run_id: config.run_id.clone(),
        mode: config.mode,
        runner_passed: true,
        closure_eligible: false,
        commands,
    })
}

fn run_command(
    config: &Spec034ReleaseConfig,
    output: &Path,
    spec: CommandSpec,
) -> Result<CommandEvidence, Spec034ReleaseArtifactError> {
    let argv = [
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "--locked",
        "-p",
        spec.package,
        "--test",
        spec.target,
    ]
    .map(str::to_owned)
    .to_vec();
    let command = execute_spec031_release_command(
        &Spec031ReleaseCommandSpec {
            id: format!("spec034-{}", spec.kind),
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: Some(spec.package.to_owned()),
            filter: None,
            argv,
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
        output,
    )
    .map_err(Spec034ReleaseArtifactError::Command)?;
    Ok(CommandEvidence {
        kind: spec.kind.to_owned(),
        stdout_digest: digest_file(&output.join(&command.stdout_path))?,
        stderr_digest: digest_file(&output.join(&command.stderr_path))?,
        command,
    })
}

pub fn coverage(
    config: &Spec034ReleaseConfig,
    commands: &[CommandEvidence],
) -> Result<CoverageDocument, Spec034ReleaseArtifactError> {
    Ok(CoverageDocument {
        schema: "spec034.coverage.v1".to_owned(),
        run_id: config.run_id.clone(),
        requirements: catalog::requirements(&command_ref(commands, "sequential-integration")?),
        blockers: catalog::blockers(&command_ref(commands, "schema-contract")?),
    })
}

pub fn write_documents(
    config: &Spec034ReleaseConfig,
    root: &Path,
    source: &SourceManifest,
    fixtures: &[DigestRow],
    coverage: &CoverageDocument,
    results: &ResultsDocument,
) -> Result<(), Spec034ReleaseArtifactError> {
    let integration = command_ref(&results.commands, "sequential-integration")?;
    let schema = command_ref(&results.commands, "schema-contract")?;
    write_json(root, "results.json", results)?;
    write_json(root, "coverage-matrix.json", coverage)?;
    write_json(
        root,
        "review-records.json",
        &ReviewDocument {
            schema: "spec034.runner_reviews.v1".to_owned(),
            run_id: config.run_id.clone(),
            records: catalog::reviews(&schema, config.mode == Spec034ReleaseMode::SuccessFixture),
        },
    )?;
    write_json(
        root,
        "owner-audits.json",
        &OwnerAuditDocument {
            schema: "spec034.runner_owner_audits.v1".to_owned(),
            run_id: config.run_id.clone(),
            audits: catalog::owner_audits(&integration),
        },
    )?;
    write_json(
        root,
        "failure-triage.json",
        &TriageDocument {
            schema: "spec034.triage.v1".to_owned(),
            run_id: config.run_id.clone(),
            command_failures: Vec::new(),
            open_blockers: Vec::new(),
        },
    )?;
    write_json(
        root,
        "reproducibility-observations.json",
        &ObservationsDocument {
            schema: "spec034.observations.v1".to_owned(),
            run_id: config.run_id.clone(),
            source: source.clone(),
            fixture_digests: fixtures.to_vec(),
            dirty_worktree_recorded: source.worktree_dirty,
        },
    )?;
    write_json(
        root,
        "cleanup-receipt.json",
        &CleanupReceipt {
            schema: "spec034.cleanup.v1".to_owned(),
            run_id: config.run_id.clone(),
            raw_evidence_cleaned: true,
            staging_atomically_published: true,
            leaked_paths: Vec::new(),
        },
    )?;
    write_json(
        root,
        "summary.json",
        &SummaryDocument {
            schema: "spec034.summary.v1".to_owned(),
            run_id: config.run_id.clone(),
            label: "runner-mechanics-only".to_owned(),
            runner_passed: true,
            closure_eligible: false,
            non_guarantees: catalog::non_guarantees(),
        },
    )
}

pub fn fixture_digests(repo: &Path) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    catalog::FIXTURES
        .iter()
        .map(|locator| {
            source::validate_locator(locator)?;
            Ok(DigestRow {
                locator: (*locator).to_owned(),
                digest: digest_file(&repo.join(locator))?,
            })
        })
        .collect()
}

pub fn command_ref(
    commands: &[CommandEvidence],
    kind: &str,
) -> Result<DigestRow, Spec034ReleaseArtifactError> {
    let command = commands
        .iter()
        .find(|command| command.kind == kind)
        .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
    Ok(DigestRow {
        locator: command.command.stdout_path.clone(),
        digest: command.stdout_digest.clone(),
    })
}

pub fn command_passed(command: &CommandEvidence) -> bool {
    command.command.status == Spec031ReleaseCommandStatus::Passed
        && command.command.exit_code == Some(0)
        && command
            .command
            .tests
            .as_ref()
            .is_some_and(|tests| tests.tests_run > 0 && tests.tests_failed == 0)
}
