use super::model::{
    Spec033ArtifactInput, Spec033ArtifactManifest, Spec033ArtifactRef,
    Spec033ArtifactTransformError, Spec033ReviewKind, Spec033ReviewVerdict,
    SPEC033_REVIEW_ARTIFACT_SCHEMA,
};
use crate::release_evidence::EvidenceWriter;
use sha2::{Digest, Sha256};
use shacs_redaction::redact_string;
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub fn build_spec033_review_artifacts(
    input: Spec033ArtifactInput,
) -> Result<Spec033ArtifactManifest, Spec033ArtifactTransformError> {
    let redaction_evidence = input
        .redaction_evidence
        .ok_or(Spec033ArtifactTransformError::MissingRedactionEvidence)?;
    validate_identifier(&input.run_id)?;
    validate_identifier(&input.trajectory_id)?;
    validate_identifier(&input.execution_snapshot_id)?;
    validate_evidence(&input.source_artifact_root, &input.execution_snapshot)?;
    validate_evidence(&input.source_artifact_root, &input.replay_result)?;
    validate_evidence(&input.source_artifact_root, &redaction_evidence)?;
    validate_reviews(&input.source_artifact_root, &input.reviews)?;
    validate_cargo_commands(&input.source_artifact_root, &input.cargo_commands)?;
    validate_coverage(&input.source_artifact_root, &input.coverage)?;

    let mut reviews = input.reviews;
    reviews.sort_by_key(|review| review.kind);
    for review in &mut reviews {
        review.safe_summary = redact_string(&review.safe_summary);
        review
            .evidence
            .sort_by(|left, right| left.locator.cmp(&right.locator));
        review.evidence.dedup();
    }
    let mut coverage = input.coverage;
    coverage
        .artifacts
        .sort_by(|left, right| left.locator.cmp(&right.locator));
    coverage.artifacts.dedup();
    let manifest = Spec033ArtifactManifest {
        schema: SPEC033_REVIEW_ARTIFACT_SCHEMA.to_owned(),
        run_id: input.run_id,
        trajectory_id: input.trajectory_id,
        execution_snapshot_id: input.execution_snapshot_id,
        execution_snapshot: input.execution_snapshot,
        replay_result: input.replay_result,
        redaction_evidence,
        safe_summary: redact_string(&input.safe_summary),
        reviews,
        cargo_commands: input.cargo_commands,
        coverage,
        artifact_paths: vec![
            "manifest.json".to_owned(),
            "coverage/spec033.json".to_owned(),
            "reviews/qa.json".to_owned(),
            "reviews/goal.json".to_owned(),
            "reviews/code.json".to_owned(),
            "reviews/security.json".to_owned(),
            "reviews/docs.json".to_owned(),
        ],
    };
    validate_serialized_strings(&manifest)?;
    Ok(manifest)
}

pub fn write_spec033_review_artifacts(
    output_dir: &Path,
    artifacts: &Spec033ArtifactManifest,
) -> Result<(), Spec033ArtifactTransformError> {
    validate_serialized_strings(artifacts)?;
    let writer =
        EvidenceWriter::open_new_run(output_dir).map_err(|_| Spec033ArtifactTransformError::Io)?;
    write_json(&writer, "manifest.json", artifacts)?;
    write_json(&writer, "coverage/spec033.json", &artifacts.coverage)?;
    for review in &artifacts.reviews {
        write_json(&writer, review.kind.file_name(), review)?;
    }
    Ok(())
}

fn validate_reviews(
    root: &Path,
    reviews: &[super::model::Spec033ReviewRecord],
) -> Result<(), Spec033ArtifactTransformError> {
    let kinds = reviews
        .iter()
        .map(|review| review.kind)
        .collect::<BTreeSet<_>>();
    if kinds != Spec033ReviewKind::required().into_iter().collect() || reviews.len() != 5 {
        return Err(Spec033ArtifactTransformError::MissingReviewEvidence);
    }
    for review in reviews {
        if review.verdict != Spec033ReviewVerdict::Pass || !review.final_review {
            return Err(Spec033ArtifactTransformError::ReviewVerdictFailed);
        }
        if review.evidence.is_empty() {
            return Err(Spec033ArtifactTransformError::MissingReviewEvidence);
        }
        for evidence in &review.evidence {
            validate_evidence(root, evidence)?;
        }
    }
    Ok(())
}

fn validate_cargo_commands(
    root: &Path,
    commands: &[super::model::Spec033CargoCommandResult],
) -> Result<(), Spec033ArtifactTransformError> {
    if commands.is_empty() {
        return Err(Spec033ArtifactTransformError::InvalidReviewCommand);
    }
    for command in commands {
        if !command.extra_arguments.is_empty() {
            return Err(Spec033ArtifactTransformError::InvalidReviewCommand);
        }
        if !command.passed || command.exit_code != 0 {
            return Err(Spec033ArtifactTransformError::ReviewCommandFailed);
        }
        validate_evidence(root, &command.evidence)?;
    }
    Ok(())
}

fn validate_coverage(
    root: &Path,
    coverage: &super::model::Spec033CoverageEntry,
) -> Result<(), Spec033ArtifactTransformError> {
    if coverage.spec_id != "033" || coverage.artifacts.is_empty() {
        return Err(Spec033ArtifactTransformError::InvalidCoverageEntry);
    }
    if !coverage.waivers.is_empty() {
        return Err(Spec033ArtifactTransformError::ForbiddenWaiver);
    }
    if !coverage.blockers.is_empty() {
        return Err(Spec033ArtifactTransformError::ForbiddenBlocker);
    }
    for artifact in &coverage.artifacts {
        validate_evidence(root, artifact)?;
    }
    Ok(())
}

fn validate_evidence(
    root: &Path,
    evidence: &Spec033ArtifactRef,
) -> Result<(), Spec033ArtifactTransformError> {
    validate_locator(&evidence.locator)?;
    validate_digest(&evidence.digest)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| Spec033ArtifactTransformError::Io)?;
    let artifact_path = root.join(&evidence.locator);
    let canonical_artifact = artifact_path.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Spec033ArtifactTransformError::MissingEvidenceArtifact
        } else {
            Spec033ArtifactTransformError::Io
        }
    })?;
    if !canonical_artifact.starts_with(&canonical_root) || !canonical_artifact.is_file() {
        return Err(Spec033ArtifactTransformError::UnsafePersistedString);
    }
    let bytes = std::fs::read(canonical_artifact).map_err(|_| Spec033ArtifactTransformError::Io)?;
    if digest(&bytes) != evidence.digest {
        return Err(Spec033ArtifactTransformError::EvidenceDigestMismatch);
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), Spec033ArtifactTransformError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(Spec033ArtifactTransformError::UnsafePersistedString);
    }
    Ok(())
}

fn validate_locator(locator: &str) -> Result<(), Spec033ArtifactTransformError> {
    let path = Path::new(locator);
    if locator.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Spec033ArtifactTransformError::UnsafePersistedString);
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), Spec033ArtifactTransformError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Spec033ArtifactTransformError::UnsafePersistedString);
    }
    Ok(())
}

fn validate_serialized_strings(
    value: &impl serde::Serialize,
) -> Result<(), Spec033ArtifactTransformError> {
    let encoded = serde_json::to_string(value).map_err(|_| Spec033ArtifactTransformError::Json)?;
    if encoded.contains("/Users/") || encoded.contains("/home/") || encoded.contains("sk-secret") {
        return Err(Spec033ArtifactTransformError::UnsafePersistedString);
    }
    Ok(())
}

fn write_json(
    writer: &EvidenceWriter,
    path: &str,
    value: &impl serde::Serialize,
) -> Result<(), Spec033ArtifactTransformError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| Spec033ArtifactTransformError::Json)?;
    writer
        .write_new(path, &bytes)
        .map_err(|_| Spec033ArtifactTransformError::Io)
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
