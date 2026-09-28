use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_projection::{Spec031ExternalOwnerId, Spec031ReleaseRunArtifacts};
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn current_worktree_stderr_matches_generated_spec035_cause(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let evidence_root = temp.path().canonicalize()?.join("current-evidence");
    let repo_root = temp.path().canonicalize()?.join("repo");
    let fixture_root = repo_root.join(".omo/evidence/spec035/prd000-009");
    fs::create_dir_all(fixture_root.join("source-audits"))?;
    fs::create_dir_all(fixture_root.join("commands"))?;
    assert!(Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&repo_root)
        .status()?
        .success());
    fs::write(repo_root.join("source.txt"), "bound\n")?;
    let source_list = format!("{:x}  source.txt\n", Sha256::digest(b"bound\n"));
    let source_list_hash = format!("{:x}", Sha256::digest(source_list.as_bytes()));
    let mut inventory = String::new();
    for (file, contents) in [
        ("source-audits/final-source-files.sha256", source_list),
        (
            "source-audits/source-binding.json",
            json!({
                "schema": "spec035.source_binding.v1", "files": 1,
                "finalSourceList": "final-source-files.sha256",
                "preSha256": source_list_hash, "finalSha256": source_list_hash,
                "drift": false
            })
            .to_string(),
        ),
        (
            "command-registry.json",
            json!({
                "schema": "spec035.command_registry.v1",
                "commands": [{"id": "focused", "transcript": "commands/focused.txt", "exitCode": 0, "passed": 1, "failed": 0}],
                "testSummary": {"passed": 1, "failed": 0},
                "workspaceGate": {"exitCode": 0, "status": "PASS", "failed": 0},
                "result": "PASS"
            })
            .to_string(),
        ),
        (
            "external-owner-audits.json",
            json!({
                "schema": "spec035.external_owner_audits.v1",
                "owners": [{"spec": "029", "status": "PASS", "tests": ["commands/focused.txt"], "passed": 1, "failed": 0}],
                "result": "PASS"
            })
            .to_string(),
        ),
        (
            "cleanup-registry.json",
            json!({
                "schema": "spec035.cleanup_registry.v1",
                "items": [{"id": "fixture", "disposition": "removed", "absenceProof": "cleanup-proof.txt"}],
                "result": "PASS"
            })
            .to_string(),
        ),
        (
            "incident-registry.json",
            json!({"schema": "spec035.todo10_incidents.v1", "incidents": [{"id": "resolved", "status": "RESOLVED"}], "result": "PASS"}).to_string(),
        ),
        ("workspace-failure-triage.json", json!({"workspaceResult": "PASS"}).to_string()),
        ("commands/focused.txt", "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n".to_owned()),
        ("cleanup-proof.txt", "absent\n".to_owned()),
        ("evidence.json", "{}\n".to_owned()),
        ("visual-review.json", "{}\n".to_owned()),
        ("correction-command-registry.json", "{}\n".to_owned()),
        ("closure-mapping-corrected.json", "{}\n".to_owned()),
    ] {
        fs::write(fixture_root.join(file), &contents)?;
        inventory.push_str(&format!(
            "{:x}  .omo/evidence/spec035/prd000-009/{file}\n",
            Sha256::digest(contents.as_bytes())
        ));
    }
    fs::write(fixture_root.join("artifact-hashes.sha256"), inventory)?;
    let requirements: Vec<_> = (0..10)
        .flat_map(|prd| (1..=4).map(move |row| (prd, row)))
        .map(|(prd, row)| {
            json!({"id": format!("PRD{prd:03}-{row}"), "status": "PASS", "evidence": ["evidence.json"]})
        })
        .collect();
    fs::write(
        fixture_root.join("manifest.json"),
        serde_json::to_vec(&json!({
            "schema": "spec035.prd000_009_closure_manifest.v1", "verdict": "PASS",
            "sourceBinding": {"finalSourceFiles": "source-audits/final-source-files.sha256", "drift": false},
            "registries": {
                "commands": "command-registry.json", "externalOwners": "external-owner-audits.json",
                "visualReview": "visual-review.json", "cleanup": "cleanup-registry.json",
                "artifactHashes": "artifact-hashes.sha256", "workspaceFailureTriage": "workspace-failure-triage.json",
                "correctionCommands": "correction-command-registry.json", "incidents": "incident-registry.json",
                "correctionSourceBinding": "source-audits/source-binding.json", "correctedClosureMapping": "closure-mapping-corrected.json"
            },
            "summary": {"requirements": 40, "passed": 40, "blocked": 0, "focusedTestsFailed": 0, "workspaceTestsFailed": 0},
            "requirements": requirements,
            "gates": [
                {"id": "external-owner-exact", "status": "PASS"},
                {"id": "workspace-suite", "status": "PASS"},
                {"id": "cleanup", "status": "PASS"},
                {"id": "default-config-incident", "status": "PASS"}
            ]
        }))?,
    )?;
    fs::write(repo_root.join("source.txt"), "changed\n")?;

    let output = Command::new(env!("CARGO_BIN_EXE_spec031-release-runner"))
        .args([
            "--run-id",
            "spec035-cli-current",
            "--evidence-root",
            evidence_root.to_str().ok_or("non-UTF-8 evidence path")?,
            "--repo-root",
            repo_root.to_str().ok_or("non-UTF-8 repository path")?,
            "--mode",
            "current-worktree",
        ])
        .output()?;

    assert_eq!(output.status.code(), Some(1));
    let artifacts: Spec031ReleaseRunArtifacts =
        serde_json::from_slice(&fs::read(evidence_root.join("manifest.json"))?)?;
    let spec035_reason = &artifacts
        .external_audits
        .iter()
        .find(|audit| audit.owner == Spec031ExternalOwnerId::Spec035)
        .ok_or("missing Spec035 audit")?
        .reason;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(spec035_reason.contains("source binding digest mismatch for "));
    assert_eq!(
        spec035_reason,
        "Spec035 evidence rejected: Spec035Evidence(\"source binding digest mismatch for source.txt\")"
    );
    assert_eq!(
        stderr,
        format!("spec031 release runner failed: BlockedExternalEvidence: {spec035_reason}\n")
    );

    Ok(())
}

#[test]
fn success_fixture_writes_no_stderr() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let evidence_root = temp.path().canonicalize()?.join("success-evidence");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;

    let output = Command::new(env!("CARGO_BIN_EXE_spec031-release-runner"))
        .args([
            "--run-id",
            "spec035-cli-success",
            "--evidence-root",
            evidence_root.to_str().ok_or("non-UTF-8 evidence path")?,
            "--repo-root",
            repo_root.to_str().ok_or("non-UTF-8 repository path")?,
            "--mode",
            "success-fixture",
        ])
        .output()?;

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    Ok(())
}
