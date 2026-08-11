use super::model::{
    Spec030ReleaseArtifactError, Spec030ReleaseRunnerConfig, Spec030ReleaseRunnerMode,
    Spec030SurfaceOwnerEvidence, Spec030SurfaceOwnerReadiness, Spec030SurfaceOwnerShutdown,
};
use super::surface_owner_evidence::{signed, validate, SURFACE_OWNER_SCHEMA};
use super::surface_owner_spawn::spawn_spec;
use super::surface_owner_tests::{runner_config, temp_path};
use super::surface_runner::prepare;
use crate::release_evidence::EvidenceWriter;

#[test]
fn owner_evidence_rejects_mutated_executable() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    assert_tamper_rejected("executable", |config, evidence| {
        evidence.spawn.executable = config
            .repo_root
            .join("other-shacs-bot")
            .display()
            .to_string();
        evidence.argv = evidence.spawn.argv();
    })
}

#[test]
fn owner_evidence_rejects_mutated_config_path() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    assert_tamper_rejected("config-path", |_, evidence| {
        evidence.spawn.config_path = "surface/config.json".to_owned();
        evidence.argv = evidence.spawn.argv();
    })
}

#[test]
fn owner_evidence_rejects_mutated_workspace_path() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    assert_tamper_rejected("workspace-path", |_, evidence| {
        evidence.spawn.workspace_path = "surface/workspace".to_owned();
        evidence.argv = evidence.spawn.argv();
    })
}

#[test]
fn owner_evidence_rejects_mutated_bind() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    assert_tamper_rejected("bind", |_, evidence| {
        evidence.spawn.bind = "127.0.0.1:49154".to_owned();
        evidence.argv = evidence.spawn.argv();
    })
}

#[test]
fn owner_evidence_rejects_separately_reconstructed_argv() -> Result<(), Box<dyn std::error::Error>>
{
    // Given / When / Then
    assert_tamper_rejected("argv", |_, evidence| {
        evidence.argv[3] = "surface/config.json".to_owned();
    })
}

#[test]
fn owner_evidence_rejects_config_without_bwrap() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    assert_config_tamper_rejected("missing-bwrap", |value| {
        value["tools"]["exec"]["sandbox"] = serde_json::Value::Null;
    })
}

#[test]
fn owner_evidence_rejects_config_without_credential_source(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    assert_config_tamper_rejected("missing-credential", |value| {
        value["providers"]["openai"]["credentialSource"] = serde_json::Value::Null;
    })
}

#[test]
fn owner_evidence_accepts_exact_spawn_and_fixture_config() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let (config, evidence) = owner_evidence("valid")?;
    // When
    let result = validate(
        &evidence,
        Spec030ReleaseRunnerMode::CurrentWorktree,
        &config.evidence_root,
        &config.repo_root,
    );
    // Then
    assert_eq!(result, Ok(()));
    std::fs::remove_dir_all(config.evidence_root)?;
    Ok(())
}

fn assert_tamper_rejected(
    label: &str,
    tamper: impl FnOnce(&Spec030ReleaseRunnerConfig, &mut Spec030SurfaceOwnerEvidence),
) -> Result<(), Box<dyn std::error::Error>> {
    let (config, mut evidence) = owner_evidence(label)?;
    tamper(&config, &mut evidence);
    let evidence = signed(evidence)?;
    let result = validate(
        &evidence,
        Spec030ReleaseRunnerMode::CurrentWorktree,
        &config.evidence_root,
        &config.repo_root,
    );
    assert_eq!(
        result,
        Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)
    );
    std::fs::remove_dir_all(config.evidence_root)?;
    Ok(())
}

fn assert_config_tamper_rejected(
    label: &str,
    tamper: impl FnOnce(&mut serde_json::Value),
) -> Result<(), Box<dyn std::error::Error>> {
    let (config, evidence) = owner_evidence(label)?;
    let path = config.evidence_root.join("surface/config.json");
    let mut value = serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path)?)?;
    tamper(&mut value);
    std::fs::write(&path, serde_json::to_vec_pretty(&value)?)?;
    let result = validate(
        &evidence,
        Spec030ReleaseRunnerMode::CurrentWorktree,
        &config.evidence_root,
        &config.repo_root,
    );
    assert_eq!(
        result,
        Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)
    );
    std::fs::remove_dir_all(config.evidence_root)?;
    Ok(())
}

fn owner_evidence(
    label: &str,
) -> Result<(Spec030ReleaseRunnerConfig, Spec030SurfaceOwnerEvidence), Box<dyn std::error::Error>> {
    let evidence_root = temp_path(label);
    let writer = EvidenceWriter::open_new_run(&evidence_root)?;
    let config = runner_config(evidence_root)?;
    prepare(&config, &writer, 49_153)?;
    writer.write_new("surface/owner.stdout", b"ready")?;
    writer.write_new("surface/owner.stderr", b"stopped")?;
    let spawn = spawn_spec(&config, 49_153);
    let argv = spawn.argv();
    let evidence = signed(Spec030SurfaceOwnerEvidence {
        schema: SURFACE_OWNER_SCHEMA.to_owned(),
        production_owner: true,
        owner_pid: u32::MAX,
        spawn,
        argv,
        bind_host: "127.0.0.1".to_owned(),
        requested_port: 0,
        bound_port: 49_153,
        readiness: Spec030SurfaceOwnerReadiness::Observed,
        shutdown: Spec030SurfaceOwnerShutdown::Reaped,
        temp_root: "surface/owner-tmp".to_owned(),
        temp_root_removed: true,
        stdout_path: "surface/owner.stdout".to_owned(),
        stderr_path: "surface/owner.stderr".to_owned(),
        stdout_sha256: super::source_manifest::sha256_bytes(b"ready"),
        stderr_sha256: super::source_manifest::sha256_bytes(b"stopped"),
        receipt_sha256: String::new(),
    })?;
    Ok((config, evidence))
}
