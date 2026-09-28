use super::spec035_evidence_io::sha256;
use super::spec035_execution_io::{decode, Evidence};
use super::spec035_execution_model::{Execution, FileRef};
use serde_json::{json, Value};
use std::fs;

#[path = "spec035_accounting_remediation_test.rs"]
mod remediation;

const STDOUT: &str = "running 2 tests\nrunning 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

struct CommandFixture {
    repo: tempfile::TempDir,
    evidence: tempfile::TempDir,
    command: Value,
    accounting: Value,
    source: FileRef,
    stdout: FileRef,
    stderr: FileRef,
    incident: Option<Value>,
    incident_receipt: Value,
}

impl CommandFixture {
    fn new() -> Self {
        let repo = tempfile::tempdir().expect("owned repo");
        assert!(std::process::Command::new("git")
            .env("GIT_MASTER", "1")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .status()
            .expect("git init")
            .success());
        fs::create_dir(repo.path().join("crates")).expect("crates");
        let source_files: Vec<_> = ["crates/Cargo.toml", "crates/Cargo.lock"]
            .into_iter()
            .map(|path| {
                fs::write(repo.path().join(path), "synthetic source, never executed")
                    .expect("source");
                FileRef {
                    path: path.to_owned(),
                    sha256: sha256(b"synthetic source, never executed"),
                }
            })
            .collect();
        let evidence = tempfile::tempdir().expect("owned evidence");
        let source = write(
            evidence.path(),
            "source-before.json",
            &serde_json::to_vec(&source_files).expect("source"),
        );
        write(
            evidence.path(),
            "source-after.json",
            &serde_json::to_vec(&source_files).expect("source"),
        );
        let stdout = write(evidence.path(), "workspace.stdout", STDOUT.as_bytes());
        let stderr = write(
            evidence.path(),
            "workspace.stderr",
            b"Running fixture-target\n",
        );
        let accounting = json!({"schema":"spec035.workspace_test_accounting.v1","run_id":"fixture-run","source_sha256":source.sha256,"stdout":stdout,"stderr":stderr,"targets":[{"header":"Running fixture-target","summary_line":4}]});
        let command = json!({"id":"workspace","kind":"workspace_test","run_id":"fixture-run","source_sha256":source.sha256,"argv":["cargo","test","--manifest-path","crates/Cargo.toml","--locked","--workspace","--no-fail-fast"],"package":null,"filter":null,"tests":{"tests_run":2,"tests_failed":0},"exit_code":0,"stdout":stdout,"stderr":stderr});
        let incident_receipt = json!({"schema":"spec035.incident_disposition.v1","run_id":"fixture-run","source_sha256":source.sha256,"subject":"default-config-rewrite"});
        Self {
            repo,
            evidence,
            command,
            accounting,
            source,
            stdout,
            stderr,
            incident: None,
            incident_receipt,
        }
    }

    fn validate(&self) -> Result<(), super::model::Spec031ReleaseArtifactError> {
        let accounting = write(
            self.evidence.path(),
            "accounting.json",
            &serde_json::to_vec(&self.accounting).expect("accounting"),
        );
        let after = FileRef {
            path: "source-after.json".to_owned(),
            sha256: self.source.sha256.clone(),
        };
        let acceptance = write(
            self.evidence.path(),
            "acceptance.json",
            &serde_json::to_vec(&super::spec035_admission_boundary_test::acceptance())
                .expect("acceptance"),
        );
        let mut incident = self.incident_receipt.clone();
        incident["acceptance"] = json!(acceptance);
        let incident_receipt = write(
            self.evidence.path(),
            "incident.json",
            &serde_json::to_vec(&incident).expect("incident"),
        );
        let inventory = write(
            self.evidence.path(),
            "inventory.json",
            &serde_json::to_vec(&[
                &self.source,
                &after,
                &self.stdout,
                &self.stderr,
                &accounting,
                &acceptance,
                &incident_receipt,
            ])
            .expect("inventory"),
        );
        let mut command = self.command.clone();
        if command.get("test_accounting").is_none() {
            command["test_accounting"] = json!(accounting);
        }
        let execution: Execution = decode(&serde_json::to_vec(&json!({"run_id":"fixture-run","source":{"before":self.source,"after":after},"inventory":inventory,"requirements":[],"owners":[],"commands":[command],"gates":[],"resources":[],"cleanup":after,"incidents":[]})).expect("execution"))?;
        let evidence = Evidence::open(self.repo.path(), self.evidence.path(), &execution)?;
        super::spec035_execution_commands::validate_commands(&evidence)?;
        if let Some(incident) = &self.incident {
            let mut incident = incident.clone();
            incident["receipt"] = json!(incident_receipt);
            let incident = decode(&serde_json::to_vec(&incident).expect("incident"))?;
            super::spec035_execution_receipts::validate_incident(&evidence, &incident)?;
        }
        Ok(())
    }
}

fn write(root: &std::path::Path, path: &str, bytes: &[u8]) -> FileRef {
    fs::write(root.join(path), bytes).expect("owned fixture");
    FileRef {
        path: path.to_owned(),
        sha256: sha256(bytes),
    }
}

#[test]
fn spec035_admission_command_accepts_bound_top_level_accounting() {
    assert!(CommandFixture::new().validate().is_ok());
}

#[test]
fn spec035_admission_command_rejects_forged_totals_even_with_valid_hashes() {
    let mut fixture = CommandFixture::new();
    fixture.command["tests"]["tests_run"] = json!(3);
    assert!(fixture.validate().is_err());
}

#[test]
fn spec035_admission_command_rejects_nested_aggregate_without_accounting() {
    let mut fixture = CommandFixture::new();
    fixture.command["test_accounting"] = Value::Null;
    fixture.command["tests"]["tests_run"] = json!(3);
    assert!(fixture.validate().is_err());
}

#[test]
fn spec035_admission_command_rejects_stale_accounting_bindings_and_targets() {
    for (key, value) in [
        ("run_id", json!("old-run")),
        ("source_sha256", json!("0".repeat(64))),
        ("schema", json!("spec035.workspace_test_accounting.v2")),
        (
            "targets",
            json!([{"header":"Running unrelated-target","summary_line":4}]),
        ),
    ] {
        let mut fixture = CommandFixture::new();
        fixture.accounting[key] = value;
        assert!(fixture.validate().is_err(), "{key}");
    }
    let mut fixture = CommandFixture::new();
    fixture.accounting["stdout"]["path"] = json!("unrelated.stdout");
    assert!(fixture.validate().is_err());
}

#[test]
fn spec035_admission_incident_accepts_only_explicit_scoped_disposition() {
    let mut fixture = CommandFixture::new();
    fixture.incident = Some(
        json!({"id":"default-config-rewrite","disposition":"user_accepted_repaired_baseline"}),
    );
    assert!(fixture.validate().is_ok());
}

#[test]
fn spec035_admission_incident_does_not_accept_baseline_as_verified_or_other_incident() {
    for incident in [
        json!({"id":"default-config-rewrite"}),
        json!({"id":"ambiguous-spec034-temp-roots","disposition":"user_accepted_repaired_baseline"}),
        json!({"id":"default-config-rewrite","disposition":"verified_restoration"}),
    ] {
        let mut fixture = CommandFixture::new();
        fixture.incident = Some(incident);
        assert!(fixture.validate().is_err());
    }
}

#[test]
fn spec035_admission_incident_rejects_stale_binding_and_subject() {
    for (field, value) in [
        ("run_id", json!("old-run")),
        ("source_sha256", json!("old-source")),
        ("subject", json!("missing-spec034-fixture")),
        ("schema", json!("spec035.incident_disposition.v2")),
    ] {
        let mut fixture = CommandFixture::new();
        fixture.incident = Some(
            json!({"id":"default-config-rewrite","disposition":"user_accepted_repaired_baseline"}),
        );
        fixture.incident_receipt[field] = value;
        assert!(fixture.validate().is_err());
    }
}
