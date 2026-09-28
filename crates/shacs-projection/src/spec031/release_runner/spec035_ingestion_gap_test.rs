use super::super::spec035_evidence::validate_spec035_closure_evidence;
use super::spec035_evidence_test::{evidence, mutate_json, refresh_inventory, valid_fixture};
use std::fs;

#[test]
fn spec035_f2_rejects_pass_owner_with_matching_failed_transcript() {
    use super::super::model::Spec031ReleaseArtifactError;
    use sha2::{Digest, Sha256};

    let repo = valid_fixture("f2-failed-owner");
    let transcript = "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    fs::write(evidence(&repo, "commands/owner.txt"), transcript).expect("owner transcript");
    mutate_json(&evidence(&repo, "external-owner-audits.json"), |value| {
        value["owners"][0]["tests"] = serde_json::json!(["commands/owner.txt"]);
        value["owners"][0]["failed"] = serde_json::json!(1);
    });
    refresh_inventory(&repo);
    let inventory_path = evidence(&repo, "artifact-hashes.sha256");
    let mut inventory = fs::read_to_string(&inventory_path).expect("inventory reads");
    inventory.push_str(&format!(
        "{:x}  .omo/evidence/spec035/prd000-009/commands/owner.txt\n",
        Sha256::digest(transcript.as_bytes())
    ));
    fs::write(inventory_path, inventory).expect("inventory writes");

    let result = validate_spec035_closure_evidence(&repo);
    fs::remove_dir_all(&repo).expect("owned fixture removed before assertion");

    assert_eq!(
        result,
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
}

#[test]
fn rejects_pass_owner_registry_hiding_blocked_owner() {
    let repo = valid_fixture("hidden-owner-blocker");
    mutate_json(&evidence(&repo, "external-owner-audits.json"), |value| {
        value["owners"][0]["status"] = serde_json::json!("BLOCKED");
    });
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("blocked child must fail");

    assert!(error.to_string().contains("owner 029 status BLOCKED"));
}

#[test]
fn rejects_pass_cleanup_registry_hiding_blocked_disposition() {
    let repo = valid_fixture("hidden-cleanup-blocker");
    mutate_json(&evidence(&repo, "cleanup-registry.json"), |value| {
        value["items"][0]["disposition"] = serde_json::json!("blocked");
    });
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("blocked cleanup must fail");

    assert!(error
        .to_string()
        .contains("cleanup fixture disposition blocked"));
}

#[test]
fn rejects_pass_incident_registry_hiding_unresolved_incident() {
    let repo = valid_fixture("hidden-incident-blocker");
    mutate_json(&evidence(&repo, "incident-registry.json"), |value| {
        value["incidents"][0]["status"] = serde_json::json!("UNRESOLVED");
    });
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("unresolved child must fail");

    assert!(error
        .to_string()
        .contains("incident resolved status UNRESOLVED"));
}

#[test]
fn rejects_mandatory_registry_removed_from_inventory() {
    let repo = valid_fixture("registry-inventory-omission");
    remove_inventory_entry(&repo, "command-registry.json");

    let error = validate_spec035_closure_evidence(&repo).expect_err("registry hash is mandatory");

    assert!(error
        .to_string()
        .contains("SHA-256 inventory is missing command-registry.json"));
}

#[test]
fn rejects_command_transcript_removed_from_inventory() {
    let repo = valid_fixture("transcript-inventory-omission");
    remove_inventory_entry(&repo, "commands/focused.txt");

    let error = validate_spec035_closure_evidence(&repo).expect_err("transcript hash is mandatory");

    assert!(error
        .to_string()
        .contains("SHA-256 inventory is missing commands/focused.txt"));
}

#[test]
fn rejects_test_command_without_passed_count() {
    let repo = valid_fixture("missing-passed-count");
    mutate_json(&evidence(&repo, "command-registry.json"), |value| {
        value["commands"][0]
            .as_object_mut()
            .expect("command object")
            .remove("passed");
    });
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("test count is mandatory");

    assert!(error.to_string().contains("focused missing passed count"));
}

#[test]
fn rejects_empty_test_transcript_claiming_one_pass() {
    let repo = valid_fixture("empty-test-transcript");
    fs::write(evidence(&repo, "commands/focused.txt"), "").expect("transcript empties");
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("empty transcript must fail");

    assert!(error
        .to_string()
        .contains("focused transcript has no Cargo test summary"));
}

#[test]
fn rejects_test_count_not_correlated_with_transcript() {
    let repo = valid_fixture("mismatched-test-count");
    mutate_json(&evidence(&repo, "command-registry.json"), |value| {
        value["commands"][0]["passed"] = serde_json::json!(2);
        value["testSummary"]["passed"] = serde_json::json!(2);
    });
    refresh_inventory(&repo);

    let error = validate_spec035_closure_evidence(&repo).expect_err("claimed count must match");

    assert!(error
        .to_string()
        .contains("focused transcript count mismatch"));
}

fn remove_inventory_entry(repo: &std::path::Path, suffix: &str) {
    let path = evidence(repo, "artifact-hashes.sha256");
    let inventory = fs::read_to_string(&path).expect("inventory reads");
    let retained = inventory
        .lines()
        .filter(|line| !line.ends_with(suffix))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{retained}\n")).expect("inventory rewrites");
}
