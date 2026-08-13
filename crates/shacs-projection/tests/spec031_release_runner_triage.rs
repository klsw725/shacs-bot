use shacs_projection::{
    run_spec031_release_runner, validate_spec031_release_artifacts, Spec031ExternalAuditStatus,
    Spec031ExternalOwnerId, Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts,
    Spec031ReleaseRunId, Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode,
};
use std::collections::BTreeSet;
use std::fs;
use std::time::Duration;

#[test]
fn spec031_blocked_external_triage_matches_blocked_audit_rows_and_rejects_omission(
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = temp_path("blocked-triage-repo");
    fs::create_dir_all(repo.join("crates"))?;
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
    assert_eq!(error, Spec031ReleaseArtifactError::CommandFailed);

    let artifacts: Spec031ReleaseRunArtifacts =
        serde_json::from_slice(&fs::read(evidence_root.join("manifest.json"))?)?;
    let triage_path = evidence_root.join("triage/blocked-external-evidence.json");
    let mut triage: serde_json::Value = serde_json::from_slice(&fs::read(&triage_path)?)?;
    assert_eq!(
        triage_owner_set(&triage),
        blocked_audit_owner_set(&artifacts)
    );

    triage["blocked_external_audits"]
        .as_array_mut()
        .expect("blocked audits are mutable array")
        .retain(|blocker| blocker["owner"] != "spec033");
    fs::write(&triage_path, serde_json::to_vec_pretty(&triage)?)?;

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("omitted blocked audit owner fails triage validation");
    assert_eq!(error, Spec031ReleaseArtifactError::CommandFailed);
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
