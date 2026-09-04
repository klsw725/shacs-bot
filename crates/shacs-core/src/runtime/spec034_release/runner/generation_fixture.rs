use super::*;
use shacs_projection::{
    Spec031ReleaseCommandStatus, Spec031ReleaseGateKind, Spec031ReleaseTestCounts,
};

pub(crate) fn fixture_results(
    config: &Spec034ReleaseConfig,
    output: &Path,
    source_digest: &str,
    toolchain: &super::super::super::tools::ResolvedToolchain,
) -> Result<ResultsDocument, Spec034ReleaseArtifactError> {
    let commands = super::super::command_specs::COMMAND_SPECS.iter().map(|spec| {
        command(
            output,
            source_digest,
            toolchain,
            spec,
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
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

fn command(
    output: &Path,
    source_digest: &str,
    toolchain: &super::super::super::tools::ResolvedToolchain,
    spec: &super::super::command_specs::CommandSpec,
) -> Result<CommandEvidence, Spec034ReleaseArtifactError> {
    let id = format!("spec034-{}", spec.kind);
    let stdout_path = format!("{id}.stdout");
    let stderr_path = format!("{id}.stderr");
    let stdout_digest = super::write_summary(output, &stdout_path, b"")?;
    let stderr_digest = super::write_summary(output, &stderr_path, b"")?;
    Ok(CommandEvidence {
        kind: spec.kind.to_owned(),
        source_digest: source_digest.to_owned(),
        tool: toolchain.cargo_identity().clone(),
        rustc: toolchain.rustc_identity().clone(),
        environment_policy: "spec034.controlled-toolchain.v1".to_owned(),
        command: PortableCommandRecord {
            id,
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: Some(spec.package.to_owned()),
            filter: None,
            argv: spec.argv(),
            cwd: ".".to_owned(),
            status: Spec031ReleaseCommandStatus::Passed,
            exit_code: Some(0),
            stdout_path,
            stderr_path,
            tests: Some(Spec031ReleaseTestCounts {
                tests_run: spec.tests_run,
                tests_failed: 0,
            }),
        },
        portable_process_receipt: PortableProcessReceipt {
            reaped: true,
            temp_paths_published: true,
        },
        stdout_digest,
        stderr_digest,
    })
}
