use super::super::artifacts::write_json;
use super::super::catalog;
use super::super::model::*;
use shacs_projection::Spec031ReleaseCommandStatus;
use std::path::Path;
#[cfg(not(test))]
use super::super::source::MaterializedSource;

#[cfg(not(test))]
pub fn run_results(
    config: &Spec034ReleaseConfig,
    output: &Path,
    execution: &MaterializedSource,
    source_digest: &str,
    toolchain: &super::super::tools::ResolvedToolchain,
) -> Result<ResultsDocument, Spec034ReleaseArtifactError> {
    let commands =
        super::command_execution::run(config, output, execution, source_digest, toolchain)?;
    if commands.iter().any(|command| !command_passed(command)) {
        return Err(Spec034ReleaseArtifactError::CommandFailed);
    }
    Ok(ResultsDocument {
        schema: "spec034.results.v2".to_owned(),
        run_id: config.run_id.clone(),
        mode: config.mode,
        runner_passed: true,
        closure_eligible: false,
        execution_attested: false,
        structural_only: true,
        commands,
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
        "summary.json",
        &SummaryDocument {
            schema: "spec034.summary.v1".to_owned(),
            run_id: config.run_id.clone(),
            label: "runner-mechanics-only".to_owned(),
            runner_passed: true,
            closure_eligible: false,
            execution_attested: false,
            structural_only: true,
            non_guarantees: catalog::non_guarantees(),
        },
    )
}

pub(super) fn write_cleanup_receipt(
    config: &Spec034ReleaseConfig,
    root: &Path,
    cleanup: &super::isolation::CompletedIsolationCleanup,
) -> Result<(), Spec034ReleaseArtifactError> {
    write_json(root, "cleanup-receipt.json", &cleanup.receipt(&config.run_id))
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

pub(super) fn write_summary(
    output: &Path,
    locator: &str,
    raw: &[u8],
) -> Result<String, Spec034ReleaseArtifactError> {
    let summary = CommandStreamSummary {
        schema: "spec034.command_stream_summary.v1".to_owned(),
        byte_count: raw.len() as u64,
        digest: super::super::artifacts::digest_bytes(raw),
    };
    let bytes = serde_json::to_vec_pretty(&summary).map_err(Spec034ReleaseArtifactError::Json)?;
    super::super::artifacts::durable_write(&output.join(locator), &bytes)?;
    Ok(super::super::artifacts::digest_bytes(&bytes))
}

#[cfg(test)]
#[path = "generation_fixture.rs"]
mod fixture_results;
#[cfg(test)]
pub(super) use fixture_results::fixture_results;

#[cfg(test)]
#[path = "generation_test.rs"]
mod tests;
