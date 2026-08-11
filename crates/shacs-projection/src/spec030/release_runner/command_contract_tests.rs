use super::catalog::coverage;
use super::command_contract::{
    lifecycle_record_matches, required_ids, validate_exact_ids, CommandEvidenceMode,
    LifecycleCwdRoots,
};
use super::semantic::fixture_surface_assertions;
use super::{model::*, runner::run_with_command_evidence_mode};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn command_sets_require_owner_lifecycle_only_for_linux_current_worktree() {
    // Given / When
    let fixture = required_ids(CommandEvidenceMode::SuccessFixture);
    let external = required_ids(CommandEvidenceMode::ExternalRecord);
    let linux = required_ids(CommandEvidenceMode::LinuxCurrentWorktree);

    // Then
    assert_eq!(fixture, external);
    assert!(fixture.contains(&"surface-tui-no-session"));
    assert!(fixture.contains(&"surface-tui-runtime"));
    assert!(!fixture.contains(&"spec030-bwrap-owner-lifecycle"));
    assert!(linux.contains(&"spec030-bwrap-owner-lifecycle"));
    assert_eq!(linux.len(), fixture.len() + 1);
}

#[test]
fn linux_command_set_rejects_missing_and_extra_owner_lifecycle() {
    // Given
    let expected = required_ids(CommandEvidenceMode::LinuxCurrentWorktree);
    let missing = expected
        .iter()
        .copied()
        .filter(|id| *id != "spec030-bwrap-owner-lifecycle")
        .collect::<Vec<_>>();
    let mut extra = expected.clone();
    extra.push("spec030-bwrap-owner-lifecycle");
    let mut external_with_lifecycle = required_ids(CommandEvidenceMode::ExternalRecord);
    external_with_lifecycle.push("spec030-bwrap-owner-lifecycle");

    // When / Then
    assert!(!validate_exact_ids(
        CommandEvidenceMode::LinuxCurrentWorktree,
        missing
    ));
    assert!(!validate_exact_ids(
        CommandEvidenceMode::LinuxCurrentWorktree,
        extra
    ));
    assert!(validate_exact_ids(
        CommandEvidenceMode::LinuxCurrentWorktree,
        expected
    ));
    assert!(!validate_exact_ids(
        CommandEvidenceMode::ExternalRecord,
        external_with_lifecycle
    ));
}

#[test]
fn prd006_includes_owner_lifecycle_only_for_linux_current_worktree() {
    // Given
    let surfaces = fixture_surface_assertions();

    // When
    let fixture = coverage(&[], &surfaces, CommandEvidenceMode::SuccessFixture);
    let external = coverage(&[], &surfaces, CommandEvidenceMode::ExternalRecord);
    let linux = coverage(&[], &surfaces, CommandEvidenceMode::LinuxCurrentWorktree);
    let fixture_prd006 = fixture.iter().find(|row| row.prd == "006");
    let external_prd006 = external.iter().find(|row| row.prd == "006");
    let linux_prd006 = linux.iter().find(|row| row.prd == "006");

    // Then
    assert!(fixture_prd006.is_some_and(|row| !row
        .command_ids
        .iter()
        .any(|id| id == "spec030-bwrap-owner-lifecycle")));
    assert!(external_prd006.is_some_and(|row| !row
        .command_ids
        .iter()
        .any(|id| id == "spec030-bwrap-owner-lifecycle")));
    assert!(linux_prd006.is_some_and(|row| row
        .command_ids
        .iter()
        .any(|id| id == "spec030-bwrap-owner-lifecycle")));
}

#[test]
fn linux_current_worktree_command_evidence_passes_final_validator_exactly(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = Spec030ReleaseRunnerConfig {
        run_id: Spec030ReleaseRunId::try_new("linux-command-equivalent")?,
        evidence_root: std::env::temp_dir().canonicalize()?.join(format!(
            "shacs-spec030-linux-command-equivalent-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        )),
        repo_root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .ok_or("repository root missing")?
            .to_path_buf(),
        mode: Spec030ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
        manual_records: Vec::new(),
        bwrap_record: None,
    };
    let artifacts =
        run_with_command_evidence_mode(&config, CommandEvidenceMode::LinuxCurrentWorktree)?;
    let lifecycle = artifacts
        .commands
        .iter()
        .find(|command| command.id == "spec030-bwrap-owner-lifecycle")
        .ok_or("lifecycle command missing")?;
    let fixture_roots = LifecycleCwdRoots {
        runner_mode: Spec030ReleaseRunnerMode::SuccessFixture,
        evidence_root: std::path::Path::new(&artifacts.evidence_root),
        repo_root: std::path::Path::new(&artifacts.repo_root),
    };
    let mut tampered = Vec::new();
    let mut gate = lifecycle.clone();
    gate.gate = crate::Spec031ReleaseGateKind::FullCargoGate;
    tampered.push(gate);
    let mut package = lifecycle.clone();
    package.package = Some("renamed-package".to_owned());
    tampered.push(package);
    let mut filter = lifecycle.clone();
    filter.filter = Some("renamed_filter".to_owned());
    tampered.push(filter);
    let mut manifest = lifecycle.clone();
    manifest.argv[6] = "other/Cargo.toml".to_owned();
    tampered.push(manifest);
    let mut argv = lifecycle.clone();
    argv.argv.push("--ignored".to_owned());
    tampered.push(argv);
    let mut environment = lifecycle.clone();
    environment.argv[1] = "SHACS_REQUIRE_BWRAP=0".to_owned();
    tampered.push(environment);
    let mut renamed = lifecycle.clone();
    renamed.id = "spec030-fixture-owner-lifecycle".to_owned();
    tampered.push(renamed);
    let mut other_workspace = lifecycle.clone();
    other_workspace.cwd = config.repo_root.canonicalize()?.display().to_string();
    let mut noncanonical = lifecycle.clone();
    noncanonical.cwd = config
        .evidence_root
        .join("fixtures/success/../success")
        .display()
        .to_string();
    let mut escaped = lifecycle.clone();
    escaped.cwd = config.evidence_root.display().to_string();
    let mut cwd_tampered_artifacts = artifacts.clone();
    cwd_tampered_artifacts
        .commands
        .iter_mut()
        .find(|command| command.id == "spec030-bwrap-owner-lifecycle")
        .ok_or("lifecycle command missing")?
        .cwd = other_workspace.cwd.clone();
    let mut production = lifecycle.clone();
    production.cwd = config.repo_root.canonicalize()?.display().to_string();
    let production_roots = LifecycleCwdRoots {
        runner_mode: Spec030ReleaseRunnerMode::CurrentWorktree,
        evidence_root: std::path::Path::new(&artifacts.evidence_root),
        repo_root: &config.repo_root,
    };
    let serialized = serde_json::to_value(&artifacts)?;
    let mut mode_mismatch = artifacts.clone();
    mode_mismatch.command_evidence_mode = CommandEvidenceMode::ExternalRecord;
    let mut missing = artifacts.clone();
    missing
        .commands
        .retain(|command| command.id != "spec030-bwrap-owner-lifecycle");
    let mut extra = artifacts.clone();
    let mut extra_command = extra.commands[0].clone();
    extra_command.id = "spec030-bwrap-owner-lifecycle-extra".to_owned();
    extra.commands.push(extra_command);

    // When / Then
    assert_eq!(
        super::validate::validate_with_command_evidence_mode(
            &missing,
            CommandEvidenceMode::LinuxCurrentWorktree,
        )
        .expect_err("missing lifecycle command must fail"),
        Spec030ReleaseArtifactError::CommandFailed
    );
    assert!(lifecycle_record_matches(lifecycle, fixture_roots));
    assert!(tampered
        .iter()
        .all(|record| !lifecycle_record_matches(record, fixture_roots)));
    assert!(!lifecycle_record_matches(&other_workspace, fixture_roots));
    assert!(!lifecycle_record_matches(&noncanonical, fixture_roots));
    assert!(!lifecycle_record_matches(&escaped, fixture_roots));
    assert!(lifecycle_record_matches(&production, production_roots));
    assert_eq!(
        super::validate::validate_with_command_evidence_mode(
            &cwd_tampered_artifacts,
            CommandEvidenceMode::LinuxCurrentWorktree,
        )
        .expect_err("lifecycle cwd must remain bound to the fixture workspace"),
        Spec030ReleaseArtifactError::CommandFailed
    );
    assert_eq!(
        serialized["command_evidence_mode"],
        "linux_current_worktree"
    );
    assert_eq!(serialized["schema"], "spec030.release_runner.v4");
    assert_eq!(
        super::validate::validate_spec030_release_artifacts(&mode_mismatch)
            .expect_err("persisted evidence mode must control validation"),
        Spec030ReleaseArtifactError::CommandFailed
    );
    assert_eq!(
        super::validate::validate_with_command_evidence_mode(
            &extra,
            CommandEvidenceMode::LinuxCurrentWorktree,
        )
        .expect_err("extra lifecycle command must fail"),
        Spec030ReleaseArtifactError::CommandFailed
    );
    super::validate::validate_with_command_evidence_mode(
        &artifacts,
        CommandEvidenceMode::LinuxCurrentWorktree,
    )?;
    Ok(())
}
