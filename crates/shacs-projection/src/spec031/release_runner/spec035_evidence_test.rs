use super::super::model::Spec031ReleaseArtifactError;
use super::super::spec035_evidence::validate_spec035_closure_evidence;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn spec035_generated_evidence_accepts_complete_source_bound_fixture() {
    let repo = valid_fixture("pass");

    validate_spec035_closure_evidence(&repo).expect("complete evidence passes");
}

#[test]
fn spec035_generated_evidence_rejects_blocked_cleanup_gate() {
    let repo = valid_fixture("blocked");
    mutate_json(&evidence(&repo, "manifest.json"), |value| {
        value["gates"][2]["status"] = serde_json::json!("BLOCKED");
    });

    let error = validate_spec035_closure_evidence(&repo).expect_err("blocked gate fails");

    assert_eq!(error, Spec031ReleaseArtifactError::BlockedExternalEvidence);
}

#[test]
fn spec035_generated_evidence_rejects_zero_test_command() {
    let repo = valid_fixture("zero-test");
    mutate_json(&evidence(&repo, "command-registry.json"), |value| {
        value["commands"][0]["passed"] = serde_json::json!(0);
    });
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("zero tests fail");

    assert_eq!(error, Spec031ReleaseArtifactError::ZeroTestsRun);
}

#[test]
fn spec035_generated_evidence_rejects_missing_prd009_artifact() {
    let repo = valid_fixture("missing");
    fs::remove_file(evidence(&repo, "prd009-goal-parity.json")).expect("fixture file removes");

    let error = validate_spec035_closure_evidence(&repo).expect_err("missing artifact fails");

    assert_eq!(error, Spec031ReleaseArtifactError::MissingRequiredArtifact);
}

#[test]
fn spec035_generated_evidence_rejects_stale_source_binding() {
    let repo = valid_fixture("stale");
    fs::write(repo.join("source.txt"), "changed\n").expect("source mutates");

    let error = validate_spec035_closure_evidence(&repo).expect_err("stale source fails");

    assert!(error
        .to_string()
        .contains("source binding digest mismatch for source.txt"));
}

#[test]
fn spec035_generated_evidence_rejects_tampered_hashed_artifact() {
    let repo = valid_fixture("tampered");
    fs::write(
        evidence(&repo, "prd008-capability-transcript.json"),
        "tampered\n",
    )
    .expect("artifact mutates");

    let error = validate_spec035_closure_evidence(&repo).expect_err("tamper fails");

    assert!(error
        .to_string()
        .contains("SHA-256 inventory digest mismatch for prd008-capability-transcript.json"));
}

pub(super) fn valid_fixture(label: &str) -> PathBuf {
    let repo = temp_path(label);
    let root = evidence_root(&repo);
    fs::create_dir_all(root.join("source-audits")).expect("fixture directories write");
    fs::write(repo.join("source.txt"), "bound\n").expect("source writes");
    for file in [
        "prd008-capability-transcript.json",
        "prd008-snapshot-failure-injection.json",
        "prd009-goal-parity.json",
        "prd009-owner-locator-audit.json",
        "visual-review.json",
        "correction-command-registry.json",
        "closure-mapping-corrected.json",
    ] {
        fs::write(root.join(file), "{}\n").expect("evidence file writes");
    }
    write_json(
        &root.join("external-owner-audits.json"),
        &serde_json::json!({
            "schema": "spec035.external_owner_audits.v1",
            "sourceBinding": "source-audits/final-source-files.sha256",
            "owners": [{"spec": "029", "status": "PASS", "tests": ["commands/focused.txt"], "passed": 1, "failed": 0}],
            "result": "PASS"
        }),
    );
    write_json(
        &root.join("workspace-failure-triage.json"),
        &serde_json::json!({"workspaceResult": "PASS"}),
    );
    write_json(
        &root.join("command-registry.json"),
        &serde_json::json!({
            "schema": "spec035.command_registry.v1",
            "commands": [{"id": "focused", "transcript": "commands/focused.txt", "exitCode": 0, "passed": 1, "failed": 0}],
            "testSummary": {"passed": 1, "failed": 0},
            "workspaceGate": {"exitCode": 0, "status": "PASS", "failed": 0},
            "result": "PASS"
        }),
    );
    write_json(
        &root.join("cleanup-registry.json"),
        &serde_json::json!({
            "schema": "spec035.cleanup_registry.v1",
            "items": [{"kind": "path", "id": "fixture", "disposition": "removed", "absenceProof": "cleanup-proof.txt"}],
            "excluded": [],
            "result": "PASS"
        }),
    );
    write_json(
        &root.join("incident-registry.json"),
        &serde_json::json!({"schema": "spec035.todo10_incidents.v1", "incidents": [{"id": "resolved", "status": "RESOLVED"}], "result": "PASS"}),
    );
    fs::write(root.join("evidence.json"), "{}\n").expect("shared evidence writes");
    fs::create_dir_all(root.join("commands")).expect("command directory writes");
    fs::write(
        root.join("commands/focused.txt"),
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    )
    .expect("command transcript writes");
    fs::write(root.join("cleanup-proof.txt"), "absent\n").expect("cleanup proof writes");
    let source_hash = sha256(&fs::read(repo.join("source.txt")).expect("source reads"));
    let source_list = format!("{source_hash}  source.txt\n");
    fs::write(
        root.join("source-audits/final-source-files.sha256"),
        &source_list,
    )
    .expect("source list writes");
    let source_list_hash = sha256(source_list.as_bytes());
    write_json(
        &root.join("source-audits/source-binding.json"),
        &serde_json::json!({
            "schema": "spec035.source_binding.v1",
            "files": 1,
            "finalSourceList": "final-source-files.sha256",
            "preSha256": source_list_hash,
            "finalSha256": source_list_hash,
            "drift": false
        }),
    );
    let requirements = (0..10)
        .flat_map(|prd| (1..=4).map(move |row| (prd, row)))
        .map(|(prd, row)| {
            serde_json::json!({
                "id": format!("PRD{prd:03}-{row}"),
                "status": "PASS",
                "evidence": ["evidence.json"]
            })
        })
        .collect::<Vec<_>>();
    write_json(
        &root.join("manifest.json"),
        &serde_json::json!({
            "schema": "spec035.prd000_009_closure_manifest.v1",
            "verdict": "PASS",
            "sourceBinding": {
                "finalSourceFiles": "source-audits/final-source-files.sha256",
                "drift": false
            },
            "registries": {
                "commands": "command-registry.json",
                "externalOwners": "external-owner-audits.json",
                "visualReview": "visual-review.json",
                "cleanup": "cleanup-registry.json",
                "artifactHashes": "artifact-hashes.sha256",
                "workspaceFailureTriage": "workspace-failure-triage.json",
                "correctionCommands": "correction-command-registry.json",
                "incidents": "incident-registry.json",
                "correctionSourceBinding": "source-audits/source-binding.json",
                "correctedClosureMapping": "closure-mapping-corrected.json"
            },
            "summary": {"requirements": 40, "passed": 40, "blocked": 0, "focusedTestsFailed": 0, "workspaceTestsFailed": 0},
            "requirements": requirements,
            "gates": [
                {"id": "external-owner-exact", "status": "PASS"},
                {"id": "workspace-suite", "status": "PASS"},
                {"id": "cleanup", "status": "PASS"},
                {"id": "default-config-incident", "status": "PASS"}
            ]
        }),
    );
    refresh_inventory(&repo);
    repo
}

pub(super) fn refresh_inventory(repo: &Path) {
    let root = evidence_root(repo);
    let files = [
        "cleanup-registry.json",
        "cleanup-proof.txt",
        "command-registry.json",
        "commands/focused.txt",
        "closure-mapping-corrected.json",
        "correction-command-registry.json",
        "evidence.json",
        "external-owner-audits.json",
        "incident-registry.json",
        "prd008-capability-transcript.json",
        "prd008-snapshot-failure-injection.json",
        "prd009-goal-parity.json",
        "prd009-owner-locator-audit.json",
        "source-audits/final-source-files.sha256",
        "source-audits/source-binding.json",
        "workspace-failure-triage.json",
        "visual-review.json",
    ];
    let inventory = files
        .iter()
        .map(|file| {
            let bytes = fs::read(root.join(file)).expect("inventory file reads");
            format!(
                "{}  .omo/evidence/spec035/prd000-009/{file}\n",
                sha256(&bytes)
            )
        })
        .collect::<String>();
    fs::write(root.join("artifact-hashes.sha256"), inventory).expect("inventory writes");
}

pub(super) fn mutate_json(path: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let mut value =
        serde_json::from_slice(&fs::read(path).expect("json reads")).expect("fixture json parses");
    mutate(&mut value);
    write_json(path, &value);
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("json serializes"),
    )
    .expect("json writes");
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn evidence(repo: &Path, file: &str) -> PathBuf {
    evidence_root(repo).join(file)
}

fn evidence_root(repo: &Path) -> PathBuf {
    repo.join(".omo/evidence/spec035/prd000-009")
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "shacs-spec035-release-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ))
}
