use super::bwrap_provenance::{build_spec030_bwrap_record, TrustedBwrapExecution};
use super::bwrap_record::{materialize_external, write_provenance};
use super::command_runner::{execute, execute_lifecycle, Invocation};
use super::model::{
    Spec030ExternalEvidence, Spec030ReleaseArtifactError, Spec030ReleaseBlocker,
    Spec030ReleaseRunnerConfig,
};
use super::source_manifest::sha256_bytes;
use crate::release_evidence::EvidenceWriter;
use crate::{parse_cargo_test_counts, Spec031ReleaseCommandRecord, Spec031ReleaseGateKind};

pub(super) fn collect_bwrap(
    config: &Spec030ReleaseRunnerConfig,
    source_digest: &str,
    writer: &EvidenceWriter,
    blockers: &mut Vec<Spec030ReleaseBlocker>,
    records: &mut Vec<Spec031ReleaseCommandRecord>,
) -> Result<Vec<Spec030ExternalEvidence>, Spec030ReleaseArtifactError> {
    if cfg!(target_os = "linux") {
        records.push(execute_lifecycle(config, writer, &config.repo_root)?);
        let mut record = execute(
            config,
            writer,
            Invocation {
                id: "spec030-bwrap-active",
                package: Some("shacs-core"),
                gate: Spec031ReleaseGateKind::FocusedCargoTest,
                cwd: config.repo_root.clone(),
                argv: strings(&[
                    "env",
                    "SHACS_REQUIRE_BWRAP=1",
                    "cargo",
                    "test",
                    "--manifest-path",
                    "crates/Cargo.toml",
                    "-p",
                    "shacs-core",
                    "--test",
                    "spec030_sandbox_adapter",
                    "real_bwrap_lane_runs_only_when_required",
                ]),
                filter: Some("real_bwrap_lane_runs_only_when_required"),
            },
        )?;
        let stdout = std::fs::read(config.evidence_root.join(&record.stdout_path))
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        let stderr = std::fs::read(config.evidence_root.join(&record.stderr_path))
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        record.tests = parse_cargo_test_counts(
            std::str::from_utf8(&stdout).map_err(|_| Spec030ReleaseArtifactError::Io)?,
        );
        let provenance = build_spec030_bwrap_record(TrustedBwrapExecution {
            source_digest,
            image_digest: &bwrap_image_digest()?,
            command: &record,
            stdout: &stdout,
            stderr: &stderr,
        })
        .map_err(|_| Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
        let evidence = write_provenance(writer, &provenance, &stdout, &stderr)?;
        records.push(record);
        return Ok(vec![evidence]);
    }
    let Some((command, evidence)) = materialize_external(
        config.bwrap_record.as_deref(),
        source_digest,
        writer,
        blockers,
    )?
    else {
        return Ok(Vec::new());
    };
    records.push(command);
    Ok(vec![evidence])
}

fn bwrap_image_digest() -> Result<String, Spec030ReleaseArtifactError> {
    let path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join("bwrap"))
        .find(|candidate| candidate.is_file())
        .ok_or(Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
    let bytes = std::fs::read(path).map_err(|_| Spec030ReleaseArtifactError::Io)?;
    Ok(sha256_bytes(&bytes))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
