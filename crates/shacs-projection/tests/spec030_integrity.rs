use shacs_projection::{
    build_spec030_source_manifest, run_spec030_release_runner, validate_spec030_release_artifacts,
    Spec030ReleaseArtifactError, Spec030ReleaseRunId, Spec030ReleaseRunnerConfig,
    Spec030ReleaseRunnerMode,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock follows epoch")
        .as_nanos();
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!(
            "shacs-spec030-integrity-{label}-{}-{nonce}",
            std::process::id()
        ))
}

fn git(repo: &Path, arguments: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(repo)
        .status()?;
    assert!(status.success(), "git {arguments:?}");
    Ok(())
}

fn clean_repo(label: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = temp_path(label);
    fs::create_dir_all(&repo)?;
    git(&repo, &["init"])?;
    fs::write(repo.join("tracked.rs"), "pub const VALUE: u8 = 1;\n")?;
    git(&repo, &["add", "tracked.rs"])?;
    git(
        &repo,
        &[
            "-c",
            "user.name=Spec030",
            "-c",
            "user.email=spec030@example.invalid",
            "commit",
            "-m",
            "fixture",
        ],
    )?;
    Ok(repo)
}

fn config(label: &str, repo_root: PathBuf) -> Spec030ReleaseRunnerConfig {
    Spec030ReleaseRunnerConfig {
        run_id: Spec030ReleaseRunId::try_new(label).expect("safe id"),
        evidence_root: temp_path(&format!("{label}-evidence")),
        repo_root,
        mode: Spec030ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
        manual_records: Vec::new(),
        bwrap_record: None,
    }
}

#[test]
fn spec030_release_runner_integrity_source_manifest_binds_mutations_and_untracked_files(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let repo = clean_repo("source-mutation")?;
    let first = build_spec030_source_manifest(&repo)?;
    let repeated = build_spec030_source_manifest(&repo)?;

    // When / Then
    assert_eq!(first, repeated);
    fs::write(repo.join("tracked.rs"), "pub const VALUE: u8 = 2;\n")?;
    let mutated = build_spec030_source_manifest(&repo)?;
    assert_ne!(first.source_digest, mutated.source_digest);
    fs::write(repo.join("untracked.rs"), "pub const EXTRA: u8 = 1;\n")?;
    let untracked = build_spec030_source_manifest(&repo)?;
    assert_ne!(mutated.source_digest, untracked.source_digest);
    assert_eq!(first.git_head, untracked.git_head);
    Ok(())
}

#[test]
fn spec030_release_runner_integrity_clean_snapshot_passes() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let config = config("clean-snapshot", clean_repo("clean-snapshot")?);

    // When
    let artifacts = run_spec030_release_runner(&config)?;

    // Then
    validate_spec030_release_artifacts(&artifacts)?;
    assert!(config.evidence_root.join("source-manifest.json").is_file());
    assert!(config
        .evidence_root
        .join("artifact-manifest.json")
        .is_file());
    Ok(())
}

#[test]
fn spec030_release_runner_integrity_rejects_artifact_tamper_and_substitution(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = config("artifact-tamper", clean_repo("artifact-tamper")?);
    let artifacts = run_spec030_release_runner(&config)?;
    fs::write(config.evidence_root.join("facts.json"), b"[]")?;

    // When / Then
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("tamper fails"),
        Spec030ReleaseArtifactError::ArtifactMismatch
    );
    fs::copy(
        config.evidence_root.join("results.json"),
        config.evidence_root.join("facts.json"),
    )?;
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("substitution fails"),
        Spec030ReleaseArtifactError::ArtifactMismatch
    );
    Ok(())
}

#[test]
fn spec030_release_runner_integrity_rejects_manifest_reorder_and_wrong_head(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = config("manifest-tamper", clean_repo("manifest-tamper")?);
    let mut artifacts = run_spec030_release_runner(&config)?;
    let manifest_path = config.evidence_root.join("artifact-manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    manifest["files"]
        .as_array_mut()
        .expect("files array")
        .reverse();
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    // When / Then
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("order is canonical"),
        Spec030ReleaseArtifactError::ManifestMismatch
    );
    artifacts.source_manifest.git_head = "0000000000000000000000000000000000000000".to_owned();
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("HEAD is bound"),
        Spec030ReleaseArtifactError::SourceMismatch
    );
    Ok(())
}

#[test]
fn spec030_release_runner_integrity_rejects_command_and_summary_tamper(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = config("command-tamper", clean_repo("command-tamper")?);
    let artifacts = run_spec030_release_runner(&config)?;
    fs::write(config.evidence_root.join("summary.md"), "PASS")?;

    // When / Then
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("summary tamper fails"),
        Spec030ReleaseArtifactError::ArtifactMismatch
    );
    Ok(())
}
