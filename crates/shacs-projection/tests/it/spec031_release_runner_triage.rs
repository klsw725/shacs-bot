use shacs_projection::{
    run_spec031_release_runner, Spec031ExternalAuditStatus, Spec031ExternalOwnerId,
    Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts, Spec031ReleaseRunId,
    Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode,
};
use std::collections::BTreeSet;
use std::fs;
use std::time::Duration;

#[test]
fn spec031_blocked_external_triage_matches_blocked_audit_rows_before_commands(
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = temp_path("blocked-triage-repo");
    fs::create_dir_all(repo.join("crates"))?;
    copy_spec035_archive(&repo)?;
    fs::write(
        repo.join("crates/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = []\n",
    )?;
    let evidence_root = temp_path("blocked-triage-evidence");

    let error = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("blocked-triage-run")?,
        evidence_root: evidence_root.clone(),
        repo_root: repo,
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::from_secs(1),
    })
    .expect_err("external blockers fail release runner");
    assert_eq!(error, Spec031ReleaseArtifactError::BlockedExternalEvidence);

    let artifacts: Spec031ReleaseRunArtifacts =
        serde_json::from_slice(&fs::read(evidence_root.join("manifest.json"))?)?;
    assert!(!artifacts.command_registry.is_empty());
    let triage_path = evidence_root.join("triage/blocked-external-evidence.json");
    let triage: serde_json::Value = serde_json::from_slice(&fs::read(&triage_path)?)?;
    assert_eq!(
        triage_owner_set(&triage),
        blocked_audit_owner_set(&artifacts)
    );

    Ok(())
}

fn copy_spec035_archive(repo: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for relative in [
        "docs/archive/spec035/README.md",
        "docs/archive/spec035/current/SHA256SUMS",
        "docs/archive/spec035/current/SPEC.md",
        "docs/archive/spec035/current/CLOSURE.md",
        "docs/archive/spec035/current/prds/000-shared-projection-model-and-vocabulary.md",
        "docs/archive/spec035/current/prds/001-surface-adapter-parity-cli-api-channel.md",
        "docs/archive/spec035/current/prds/002-approval-progress-and-recovery-parity.md",
        "docs/archive/spec035/current/prds/003-readiness-degraded-health-and-diagnostics.md",
        "docs/archive/spec035/current/prds/004-context-extension-app-and-media-projection.md",
        "docs/archive/spec035/current/prds/005-interactive-tui-repl-and-onboard-flows.md",
        "docs/archive/spec035/current/prds/006-reconnect-backpressure-and-drop-accounting.md",
        "docs/archive/spec035/current/prds/007-release-runner-and-spec035-closure.md",
        "docs/archive/spec035/current/prds/008-transport-capability-and-snapshot-first-reconnect.md",
        "docs/archive/spec035/current/prds/009-goal-and-read-only-tasks-projection.md",
        "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md",
        "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md",
        "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md",
        "docs/specs/033-evaluation-automation-live-integration/SPEC.md",
        "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md",
    ] {
        let target = repo.join(relative);
        fs::create_dir_all(target.parent().ok_or("archive parent")?)?;
        fs::copy(source_root.join(relative), target)?;
    }
    Ok(())
}

fn blocked_audit_owner_set(artifacts: &Spec031ReleaseRunArtifacts) -> BTreeSet<String> {
    artifacts
        .external_audits
        .iter()
        .filter(|audit| audit.status == Spec031ExternalAuditStatus::Blocked)
        .map(|audit| owner_slug(audit.owner).to_owned())
        .collect()
}

fn triage_owner_set(triage: &serde_json::Value) -> BTreeSet<String> {
    triage["blocked_external_audits"]
        .as_array()
        .expect("blocked audits are an array")
        .iter()
        .map(|blocker| {
            blocker["owner"]
                .as_str()
                .expect("blocker owner is string")
                .to_owned()
        })
        .collect()
}

fn owner_slug(owner: Spec031ExternalOwnerId) -> &'static str {
    match owner {
        Spec031ExternalOwnerId::Spec029 => "spec029",
        Spec031ExternalOwnerId::Spec030 => "spec030",
        Spec031ExternalOwnerId::Spec032 => "spec032",
        Spec031ExternalOwnerId::Spec033 => "spec033",
        Spec031ExternalOwnerId::Spec034 => "spec034",
        Spec031ExternalOwnerId::Spec035 => "spec035",
    }
}

fn temp_path(label: &str) -> std::path::PathBuf {
    temp_base().join(format!(
        "shacs-spec031-release-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ))
}

fn temp_base() -> std::path::PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
}
