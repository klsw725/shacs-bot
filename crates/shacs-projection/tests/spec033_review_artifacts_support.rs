use shacs_projection::{
    Spec033ArtifactInput, Spec033ArtifactRef, Spec033CargoCommand, Spec033CargoCommandResult,
    Spec033CargoPackage, Spec033CoverageEntry, Spec033ReviewKind, Spec033ReviewRecord,
    Spec033ReviewVerdict, Spec033TestTarget,
};
use std::error::Error;
use std::path::Path;

const REPLAY: &[u8] = b"recorded replay result";
const REDACTION: &[u8] = b"recorded redaction evidence";
const SNAPSHOT: &[u8] = b"recorded snapshot";

pub fn write_source_artifacts(root: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(root.join("evidence"))?;
    std::fs::write(root.join("evidence/replay.json"), REPLAY)?;
    std::fs::write(root.join("evidence/redaction.json"), REDACTION)?;
    std::fs::write(root.join("evidence/snapshot.json"), SNAPSHOT)?;
    Ok(())
}

pub fn artifact_input(root: &Path) -> Spec033ArtifactInput {
    Spec033ArtifactInput {
        source_artifact_root: root.to_path_buf(),
        run_id: "spec033-prd004".to_owned(),
        trajectory_id: "trajectory-004".to_owned(),
        execution_snapshot_id: "execution-004".to_owned(),
        execution_snapshot: artifact_ref("evidence/snapshot.json", SNAPSHOT),
        replay_result: artifact_ref("evidence/replay.json", REPLAY),
        redaction_evidence: Some(artifact_ref("evidence/redaction.json", REDACTION)),
        safe_summary: "reviewed token=sk-secret".to_owned(),
        reviews: Spec033ReviewKind::required()
            .into_iter()
            .map(review)
            .collect(),
        cargo_commands: vec![cargo_command_result()],
        coverage: Spec033CoverageEntry {
            spec_id: "033".to_owned(),
            artifacts: vec![artifact_ref("evidence/replay.json", REPLAY)],
            waivers: Vec::new(),
            blockers: Vec::new(),
        },
    }
}

fn review(kind: Spec033ReviewKind) -> Spec033ReviewRecord {
    Spec033ReviewRecord {
        kind,
        verdict: Spec033ReviewVerdict::Pass,
        final_review: true,
        evidence: vec![artifact_ref("evidence/replay.json", REPLAY)],
        safe_summary: "reviewed token=sk-secret".to_owned(),
    }
}

fn cargo_command_result() -> Spec033CargoCommandResult {
    Spec033CargoCommandResult {
        command: Spec033CargoCommand {
            package: Spec033CargoPackage::Projection,
            test_target: Spec033TestTarget::ReviewArtifacts,
        },
        extra_arguments: Vec::new(),
        exit_code: 0,
        passed: true,
        evidence: artifact_ref("evidence/replay.json", REPLAY),
    }
}

fn artifact_ref(locator: &str, bytes: &[u8]) -> Spec033ArtifactRef {
    Spec033ArtifactRef {
        locator: locator.to_owned(),
        digest: digest(bytes),
    }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
