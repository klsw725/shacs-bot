use super::surface_owner::ephemeral_port;
use super::{
    model::{Spec030ReleaseRunId, Spec030ReleaseRunnerConfig, Spec030ReleaseRunnerMode},
    surface_runner::prepare,
};
use crate::release_evidence::EvidenceWriter;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn ephemeral_owner_port_ignores_hostile_default_listener(
) -> Result<(), super::model::Spec030ReleaseArtifactError> {
    // Given / When / Then
    let hostile = TcpListener::bind("127.0.0.1:8080").ok();
    let port = ephemeral_port()?;
    assert_ne!(port, 0);
    assert_ne!(port, 8080);
    drop(hostile);
    Ok(())
}

#[test]
fn owner_fixture_config_requires_bwrap_auto_approval_and_environment_credential(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let evidence_root = temp_path("config");
    let writer = EvidenceWriter::open_new_run(&evidence_root)?;
    let config = runner_config(evidence_root.clone())?;

    // When
    prepare(&config, &writer, 49_153)?;
    let value = serde_json::from_slice::<serde_json::Value>(&std::fs::read(
        evidence_root.join("surface/config.json"),
    )?)?;

    // Then
    assert_eq!(value["tools"]["exec"]["sandbox"], "bwrap");
    assert_eq!(value["permissions"]["mode"], "auto");
    assert_eq!(
        value["permissions"]["autoApproval"]["allowProcExecVerification"],
        true
    );
    assert_eq!(
        value["permissions"]["autoApproval"]["requireDockerContainmentForExec"],
        false
    );
    assert_eq!(
        value["permissions"]["autoApproval"]["allowWorkspaceEdits"],
        true
    );
    assert_eq!(
        value["plugins"]["trustedWorkspaces"],
        serde_json::json!([evidence_root.join("surface/workspace")])
    );
    assert_eq!(value["trustedRuntime"]["trace"]["enabled"], true);
    assert_eq!(value["trustedRuntime"]["trace"]["destination"], "localOnly");
    assert_eq!(
        value["providers"]["openai"]["credentialSource"]["environment"],
        "SPEC030_OWNER_API_KEY"
    );
    assert_eq!(
        value["providers"]["openai"]["credentialSource"]["localAuth"],
        false
    );
    std::fs::remove_dir_all(evidence_root)?;
    Ok(())
}

pub(super) fn runner_config(
    evidence_root: PathBuf,
) -> Result<Spec030ReleaseRunnerConfig, super::model::Spec030ReleaseArtifactError> {
    Ok(Spec030ReleaseRunnerConfig {
        run_id: Spec030ReleaseRunId::try_new("surface-owner-unit")?,
        evidence_root,
        repo_root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf)
            .ok_or(super::model::Spec030ReleaseArtifactError::Io)?,
        mode: Spec030ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::from_secs(1),
        manual_records: Vec::new(),
        bwrap_record: None,
    })
}

pub(super) fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!(
            "shacs-spec030-owner-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
}
