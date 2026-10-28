use crate::spec031_release_runner_fixture as fixture;
use shacs_projection::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
    Spec031ReleaseRunArtifacts, SPEC031_RELEASE_RUNNER_SCHEMA,
};
use std::fs;
use std::path::Path;

const FAILED_WORKSPACE_TRANSCRIPT: &str = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

#[test]
fn rejects_passed_workspace_tests_without_counts_in_v5_v4_and_v3_runs(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut verdicts = Vec::new();
    let mut expected = Vec::new();
    for schema in [
        SPEC031_RELEASE_RUNNER_SCHEMA,
        "spec031.release_runner.v4",
        "spec031.release_runner.v3",
    ] {
        // Given
        let (repo_root, root, mut artifacts) =
            fixture::valid_non_fixture_artifacts_on_disk(&format!("uncounted-{schema}"))?;
        match schema {
            "spec031.release_runner.v4" => {
                fixture::convert_to_legacy_run(&repo_root, &root, &mut artifacts, schema)?;
            }
            "spec031.release_runner.v3" => {
                fixture::convert_to_v3_retired_spec034_run(&repo_root, &root, &mut artifacts)?;
            }
            _ => {}
        }
        let validate = |artifacts: &Spec031ReleaseRunArtifacts| {
            shacs_projection::validate_spec031_release_artifacts_with_repo_root(
                artifacts, &repo_root,
            )
        };
        verdicts.push((schema, "fixture", validate(&artifacts)));

        // When
        forge_workspace_tests(
            &root,
            &mut artifacts,
            Spec031ReleaseCommandStatus::Passed,
            Some(0),
        )?;
        verdicts.push((schema, "passed without counts", validate(&artifacts)));
        fs::write(
            root.join("commands/spec031-test-workspace.stdout"),
            FAILED_WORKSPACE_TRANSCRIPT,
        )?;
        forge_workspace_tests(
            &root,
            &mut artifacts,
            Spec031ReleaseCommandStatus::Passed,
            Some(0),
        )?;
        verdicts.push((
            schema,
            "failed transcript passed without counts",
            validate(&artifacts),
        ));
        forge_workspace_tests(
            &root,
            &mut artifacts,
            Spec031ReleaseCommandStatus::Failed,
            Some(101),
        )?;
        verdicts.push((schema, "failed without counts", validate(&artifacts)));
        forge_workspace_tests(
            &root,
            &mut artifacts,
            Spec031ReleaseCommandStatus::TimedOut,
            None,
        )?;
        verdicts.push((schema, "timed out without counts", validate(&artifacts)));
        expected.extend([
            (schema, "fixture", Ok(())),
            (
                schema,
                "passed without counts",
                Err(Spec031ReleaseArtifactError::ArtifactMismatch),
            ),
            (
                schema,
                "failed transcript passed without counts",
                Err(Spec031ReleaseArtifactError::NonzeroTestsFailed),
            ),
            (
                schema,
                "failed without counts",
                Err(Spec031ReleaseArtifactError::CommandFailed),
            ),
            (
                schema,
                "timed out without counts",
                Err(Spec031ReleaseArtifactError::CommandTimedOut),
            ),
        ]);
    }

    // Then
    assert_eq!(verdicts, expected);
    Ok(())
}

fn forge_workspace_tests(
    root: &Path,
    artifacts: &mut Spec031ReleaseRunArtifacts,
    status: Spec031ReleaseCommandStatus,
    exit_code: Option<i32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let record: &mut Spec031ReleaseCommandRecord = artifacts
        .command_registry
        .iter_mut()
        .find(|record| record.id == "spec031-test-workspace")
        .ok_or("workspace test record")?;
    record.status = status;
    record.exit_code = exit_code;
    record.tests = None;
    fixture::refresh_coverage_hashes(root, artifacts)?;
    fixture::write_artifacts(root, artifacts)?;
    Ok(())
}
