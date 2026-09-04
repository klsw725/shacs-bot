use super::*;
use std::time::Duration;
use std::sync::Mutex;

static RUNNER_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn oversized_command_timeout_is_invalid_config() {
    let config = Spec034ReleaseConfig {
        run_id: "oversized-timeout".to_owned(),
        repo_root: Path::new(".").to_path_buf(),
        evidence_root: Path::new("evidence").to_path_buf(),
        cache_root: None,
        mode: Spec034ReleaseMode::CurrentWorktree,
        command_timeout: Duration::from_secs(7_201),
    };

    assert!(matches!(
        config::validate(&config),
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    ));
}

#[test]
fn invalid_config_creates_no_run_root_residue() -> Result<(), Box<dyn std::error::Error>> {
    let parent = tempfile::tempdir()?;
    let repo = parent.path().join("repo");
    std::fs::create_dir(&repo)?;
    let config = Spec034ReleaseConfig {
        run_id: "invalid-no-residue".to_owned(),
        repo_root: repo,
        evidence_root: parent.path().join("evidence"),
        cache_root: Some(parent.path().join("cache")),
        mode: Spec034ReleaseMode::CurrentWorktree,
        command_timeout: Duration::from_secs(7_201),
    };

    assert!(matches!(
        run_spec034_release_runner(&config),
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    ));
    assert!(std::fs::read_dir(parent.path())?.all(|entry| {
        entry
            .map(|entry| !entry.file_name().to_string_lossy().starts_with(".shacs-spec034-run-"))
            .unwrap_or(false)
    }));
    Ok(())
}

#[test]
fn final_semantic_failure_leaves_commit_status_unknown() -> Result<(), Box<dyn std::error::Error>> {
    let _runner = RUNNER_LOCK.lock().map_err(|_| "runner lock poisoned")?;
    // Given
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-final-validation-failure".to_owned(),
        repo_root: repo.clone(),
        evidence_root: evidence.clone(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    // When
    let result = run_with_final_validator(&config, |context| {
        let root = context.root();
        assert!(!root.join("publication-status.json").exists());
        Err(Spec034ReleaseArtifactError::InvalidEvidence)
    });

    // Then
    let error = result.expect_err("injected final validation must fail");
    assert!(!evidence.exists(), "failed evidence became visible after {error:?}");
    Ok(())
}

#[test]
fn final_semantic_panic_never_publishes_validated_marker() -> Result<(), Box<dyn std::error::Error>> {
    let _runner = RUNNER_LOCK.lock().map_err(|_| "runner lock poisoned")?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-final-validation-panic".to_owned(),
        repo_root: repo,
        evidence_root: evidence.clone(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    let result = run_with_final_validator(&config, |context| {
        let root = context.root();
        assert!(!root.join("publication-status.json").exists());
        panic!("injected final validator panic");
    });

    assert!(
        matches!(result, Err(Spec034ReleaseArtifactError::InvalidEvidence)),
        "{result:?}"
    );
    assert!(!evidence.join("publication-status.json").exists());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn artifact_inserted_after_final_validation_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    let _runner = RUNNER_LOCK.lock().map_err(|_| "runner lock poisoned")?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-post-final-insertion".to_owned(),
        repo_root: repo.clone(),
        evidence_root: evidence.clone(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    let result = run_with_final_validator(&config, |context| {
        let staging = context.root();
        let manifest = validation::validate_pending_with_git(context)?;
        std::fs::write(staging.join("extra.json"), b"{}")
            .map_err(Spec034ReleaseArtifactError::Io)?;
        Ok(manifest)
    });

    assert!(result.is_err());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn semantically_equivalent_manifest_rewrite_after_validation_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    let _runner = RUNNER_LOCK.lock().map_err(|_| "runner lock poisoned")?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-post-validation-manifest-rewrite".to_owned(),
        repo_root: repo.clone(),
        evidence_root: evidence.clone(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    let result = run_with_final_validator(&config, |context| {
        let staging = context.root();
        let manifest = validation::validate_pending_with_git(context)?;
        let mut bytes = std::fs::read(staging.join("manifest.json"))
            .map_err(Spec034ReleaseArtifactError::Io)?;
        bytes.push(b'\n');
        std::fs::write(staging.join("manifest.json"), bytes)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        Ok(manifest)
    });

    assert!(result.is_err());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn marker_overwrite_before_publication_verification_blocks_runner(
) -> Result<(), Box<dyn std::error::Error>> {
    let _runner = RUNNER_LOCK.lock().map_err(|_| "runner lock poisoned")?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-marker-overwrite".to_owned(),
        repo_root: repo.clone(),
        evidence_root: evidence.clone(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    let result = run_with_publication_hooks(
        &config,
        validation::validate_pending_with_git,
        |staging| {
            std::fs::write(staging.join("publication-status.json"), b"overwritten marker")
                .expect("overwrite marker");
        },
        |_| {},
    );

    assert!(result.is_err(), "{result:?}");
    assert!(!evidence.exists());
    assert!(audit_spec034_release_artifacts_against(&evidence, &repo).is_err());
    Ok(())
}

#[test]
fn fresh_attestation_rejects_consistent_stream_summary_forgery(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let output = root.path().canonicalize()?;
    let toolchain = super::super::tools::ResolvedToolchain::resolve()?;
    let config = Spec034ReleaseConfig {
        run_id: "consistent-forgery".to_owned(),
        repo_root: Path::new(".").to_path_buf(),
        evidence_root: output.join("evidence"),
        cache_root: Some(output.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(60),
    };
    let mut results = generation::fixture_results(&config, &output, "sha256:source", &toolchain)?;
    let attestation = FreshExecutionAttestation::from_commands(
        "sha256:source",
        &results.commands,
        toolchain.linker_attestation_digest()?,
    )?;
    let locator = results.commands[0].command.stdout_path.clone();
    let mut summary: CommandStreamSummary =
        serde_json::from_slice(&std::fs::read(output.join(&locator))?)?;
    summary.digest = format!("sha256:{}", "0".repeat(64));
    let bytes = serde_json::to_vec_pretty(&summary)?;
    std::fs::write(output.join(&locator), &bytes)?;
    results.commands[0].stdout_digest = super::super::artifacts::digest_bytes(&bytes);
    write_json(&output, "results.json", &results)?;
    let snapshot = super::super::artifacts::ArtifactSnapshot::capture(&output)?;

    let result = attestation.verify_snapshot("sha256:source", &snapshot, &toolchain);

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::InvalidEvidence)));
    Ok(())
}

#[test]
fn manifest_overwrite_after_publication_verification_blocks_runner(
) -> Result<(), Box<dyn std::error::Error>> {
    let _runner = RUNNER_LOCK.lock().map_err(|_| "runner lock poisoned")?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-manifest-overwrite".to_owned(),
        repo_root: repo.clone(),
        evidence_root: evidence.clone(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    let result = run_with_publication_hooks(
        &config,
        validation::validate_pending_with_git,
        |_| {},
        |staging| {
            std::fs::write(staging.join("manifest.json"), b"overwritten manifest")
                .expect("overwrite manifest");
        },
    );

    assert!(result.is_err(), "{result:?}");
    assert!(!evidence.exists());
    assert!(audit_spec034_release_artifacts_against(&evidence, &repo).is_err());
    Ok(())
}
