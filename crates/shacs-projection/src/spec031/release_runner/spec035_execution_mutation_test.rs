use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_execution::admit;
use super::spec035_execution_fixture::{file_ref, write_json, Fixture};
use serde_json::{json, Value};
use std::fs;

fn mutate_receipt(fixture: &mut Fixture, pointer: &str, change: impl FnOnce(&mut Value)) {
    let path = fixture.manifest.pointer(pointer).expect("reference")["path"]
        .as_str()
        .expect("path")
        .to_owned();
    let mut receipt: Value =
        serde_json::from_slice(&fs::read(fixture.root.join(&path)).expect("receipt"))
            .expect("JSON");
    change(&mut receipt);
    write_json(&fixture.root.join(&path), &receipt);
    *fixture.manifest.pointer_mut(pointer).expect("reference") = file_ref(&fixture.root, &path);
    fixture.save();
}

#[test]
fn spec035_execution_rejects_each_missing_requirement_owner_fact_gate_and_incident() {
    for (field, count) in [
        ("requirements", 80),
        ("owners", 6),
        ("gates", 19),
        ("incidents", 3),
    ] {
        for index in 0..count {
            let mut fixture = Fixture::new();
            fixture.manifest[field]
                .as_array_mut()
                .expect("entries")
                .remove(index);
            fixture.save();
            assert!(admit(fixture.repo.path()).is_err(), "{field}[{index}]");
        }
    }
    for (owner, facts) in [2, 3, 2, 2, 2, 2].into_iter().enumerate() {
        for fact in 0..facts {
            let mut fixture = Fixture::new();
            fixture.manifest["owners"][owner]["facts"]
                .as_array_mut()
                .expect("facts")
                .remove(fact);
            fixture.save();
            assert!(
                admit(fixture.repo.path()).is_err(),
                "owner {owner} fact {fact}"
            );
        }
    }
}

#[test]
fn spec035_execution_rejects_authority_cross_reference_and_identity_mutations() {
    for (pointer, replacement) in [
        ("/requirements/0/authority/owner", json!("spec031")),
        (
            "/requirements/0/authority/closure_ids/0",
            json!("PRD004-CE-5"),
        ),
        (
            "/requirements/0/authority/source_locator",
            json!("crates/probe.rs:1"),
        ),
        ("/requirements/0/authority/id", json!("spec035:must:02")),
        ("/owners/0/owner", json!("030")),
        ("/owners/0/owner", json!("035")),
        ("/owners/0/facts/0/id", json!("spec029:unknown")),
        ("/gates/0/id", json!("visual")),
        ("/commands/0/source_sha256", json!("stale")),
        ("/commands/0/run_id", json!("other-run")),
        ("/commands/0/tests/tests_run", json!(2)),
        ("/commands/0/tests/tests_failed", json!(1)),
        ("/commands/0/exit_code", json!(1)),
    ] {
        let mut fixture = Fixture::new();
        *fixture.manifest.pointer_mut(pointer).expect("field") = replacement;
        fixture.save();
        assert!(admit(fixture.repo.path()).is_err(), "{pointer}");
    }
}

#[test]
fn spec035_execution_rejects_blocked_nested_requirements_owners_reviews_and_incidents() {
    for pointer in [
        "/requirements/0/receipt",
        "/owners/0/facts/0/receipt",
        "/gates/15/receipt",
        "/gates/16/receipt",
        "/incidents/0/receipt",
    ] {
        for failed in [false, true] {
            let mut fixture = Fixture::new();
            mutate_receipt(&mut fixture, pointer, |receipt| {
                if failed {
                    receipt["verdict"] = json!("FAILED");
                } else {
                    receipt["checks"][0]["verdict"] = json!("BLOCKED");
                }
            });
            assert!(
                matches!(
                    admit(fixture.repo.path()),
                    Err(Error::BlockedExternalEvidence)
                ),
                "{pointer}"
            );
        }
    }
}

#[test]
fn spec035_execution_rejects_receipt_relabeling_and_prose_only_reviews() {
    for (pointer, replacement) in [
        ("/subject", json!("spec035:must:02")),
        ("/source_sha256", json!("stale")),
        ("/run_id", json!("historical")),
        ("/checks", json!([])),
        ("/checks/0/commands", json!(["unknown"])),
        ("/checks/0/artifacts", json!([])),
        ("/checks/0/producer", json!("crates/unbound.rs:1")),
    ] {
        let mut fixture = Fixture::new();
        mutate_receipt(&mut fixture, "/requirements/0/receipt", |receipt| {
            *receipt.pointer_mut(pointer).expect("field") = replacement
        });
        assert!(admit(fixture.repo.path()).is_err(), "{pointer}");
    }
}

#[test]
fn spec035_execution_rejects_zero_failed_and_overflowing_test_transcripts() {
    for (text,expected) in [
        ("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",Error::ZeroTestsRun),
        ("test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",Error::NonzeroTestsFailed),
        ("test result: FAILED. 18446744073709551615 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",Error::InvalidCommandEvidence),
    ] {
        let mut fixture = Fixture::new();
        fs::write(fixture.root.join("commands/focused.stdout"),text).expect("mutated transcript");
        fixture.manifest["commands"][0]["stdout"] = file_ref(&fixture.root,"commands/focused.stdout");
        fixture.save();
        assert!(matches!(admit(fixture.repo.path()), Err(error) if error == expected));
    }
}

#[test]
fn spec035_execution_rejects_stale_incomplete_and_changed_source_snapshots() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.path().join("crates/probe.rs"), "changed\n").expect("source drift");
    assert!(admit(fixture.repo.path()).is_err());
    let fixture = Fixture::new();
    fs::write(fixture.repo.path().join("new-source.rs"), "new\n").expect("new untracked source");
    assert!(admit(fixture.repo.path()).is_err());
    let mut fixture = Fixture::new();
    let mut snapshot: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("source-before.json")).expect("snapshot"),
    )
    .expect("JSON");
    snapshot.as_array_mut().expect("files").pop();
    for path in ["source-before.json", "source-after.json"] {
        write_json(&fixture.root.join(path), &snapshot);
    }
    fixture.manifest["source"]["before"] = file_ref(&fixture.root, "source-before.json");
    fixture.manifest["source"]["after"] = file_ref(&fixture.root, "source-after.json");
    fixture.save();
    assert!(admit(fixture.repo.path()).is_err());
}

#[test]
fn spec035_execution_rejects_missing_tampered_and_unbound_artifacts() {
    for path in [
        "observation.json",
        "receipts/visual.json",
        "receipts/spec035-PRD004-CE-5.json",
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.root.join(path), "tampered\n").expect("tamper");
        assert!(admit(fixture.repo.path()).is_err());
        fs::remove_file(fixture.root.join(path)).expect("owned file removed");
        assert!(admit(fixture.repo.path()).is_err());
    }
    let mut fixture = Fixture::new();
    let mut inventory: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("inventory.json")).expect("inventory"))
            .expect("JSON");
    inventory
        .as_array_mut()
        .expect("files")
        .retain(|file| file["path"] != "observation.json");
    write_json(&fixture.root.join("inventory.json"), &inventory);
    fixture.manifest["inventory"] = file_ref(&fixture.root, "inventory.json");
    write_json(&fixture.root.join("manifest.json"), &fixture.manifest);
    assert!(admit(fixture.repo.path()).is_err());
}

#[test]
fn spec035_execution_requires_every_resource_clean_with_bound_absence_proof() {
    for (pointer, replacement) in [
        ("/verdict", json!("BLOCKED")),
        ("/resources", json!([])),
        ("/resources/0/disposition", json!("retained")),
        ("/resources/0/id", json!("different-resource")),
    ] {
        let mut fixture = Fixture::new();
        mutate_receipt(&mut fixture, "/cleanup", |receipt| {
            *receipt.pointer_mut(pointer).expect("field") = replacement
        });
        assert!(admit(fixture.repo.path()).is_err(), "{pointer}");
    }
    let mut fixture = Fixture::new();
    write_json(
        &fixture.root.join("absence.json"),
        &json!({"run_id":"constructed-execution","source_sha256":fixture.manifest["source"]["before"]["sha256"],"resource_id":"owned-probe","absent":false}),
    );
    let proof = file_ref(&fixture.root, "absence.json");
    mutate_receipt(&mut fixture, "/cleanup", |receipt| {
        receipt["resources"][0]["proof"] = proof
    });
    assert!(matches!(
        admit(fixture.repo.path()),
        Err(Error::MissingCleanupReceipt)
    ));
}
