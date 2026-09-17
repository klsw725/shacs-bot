use super::model::Spec031ReleaseArtifactError;
use super::spec035_catalog::{PRDS, SPEC_ROOT};
use super::spec035_classification::{validate_classification, ROOT};
use super::spec035_classification_model::Classification;
use super::spec035_evidence::validate_spec035_closure_evidence;
use super::spec035_evidence_io::sha256;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub(super) fn classification_fixture() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("owned classification fixture");
    fs::create_dir_all(repo.path().join(ROOT)).expect("manifest directory");
    fs::create_dir_all(repo.path().join(".omo/evidence/spec035/closure"))
        .expect("inventory directory");
    let authorities: serde_json::Map<_, _> = PRDS.iter().enumerate().map(|(prd, authority)| (
        format!("PRD{prd:03}"), json!({"file": authority.file, "section": authority.section, "count": authority.count})
    )).collect();
    let requirements: Vec<_> = PRDS.iter().enumerate().flat_map(|(prd, authority)| {
        (1..=authority.count).map(move |item| json!({
            "id": format!("PRD{prd:03}-{}-{item}", authority.tag),
            "authority": format!("PRD{prd:03}"), "item": item,
            "line": authority.first_line + item - 1, "status": "BLOCKED", "evidenceSets": ["SUPPORT"]
        }))
    }).collect();
    let gates: Vec<_> = [
        "B-SOURCE",
        "external-owner-exact",
        "workspace-suite",
        "cleanup",
        "default-config-incident",
        "release-original-artifacts",
        "final-source-qa-and-docs",
    ]
    .into_iter()
    .map(|id| json!({"id": id, "status": "BLOCKED"}))
    .collect();
    write_json(
        &repo.path().join(ROOT).join("manifest.json"),
        &json!({
            "schema": "spec035.prd000_009_closure_classification.v2", "verdict": "BLOCKED",
            "finalCommittedSeal": false, "allRequirementsApproved": false,
            "summary": {"requirements":45,"implementationClosureEvidence":37,"prd007FinalConditions":8,"passed":0,"blocked":45,"unmapped":0,"externalOwners":6,"externalOwnersPassed":0,"externalOwnersBlocked":6,"commandsExecutedForThisCorrection":0},
            "provenance": {"classificationArtifactInventory":"../closure/f1-correction-artifacts.sha256","historicalSourceBindingStatus":"STALE_NOT_REBOUND","observationsAreExecutionBinding":false},
            "authorityRoot": format!("{SPEC_ROOT}/prds/"), "authorities": authorities,
            "evidencePathBase":"repository root", "evidenceSets":{"SUPPORT":[format!("{ROOT}/support.txt")]},
            "requirements":requirements, "gates":gates, "registries":{"externalOwners":"owners.json"}
        }),
    );
    let owners: Vec<_> = ["029", "030", "031", "032", "033", "034"].into_iter().map(|spec| json!({
        "spec":spec,"status":"BLOCKED","historicalPassed":1,"historicalFailed":0,"facts":[{"status":"BLOCKED"}]
    })).collect();
    write_json(
        &repo.path().join(ROOT).join("owners.json"),
        &json!({
            "schema":"spec035.external_owner_classification.v2","result":"BLOCKED",
            "summary":{"owners":6,"passed":0,"blocked":6,"historicalTestsPassed":6,"historicalTestsFailed":0,"newTestsExecuted":0},"owners":owners
        }),
    );
    fs::write(
        repo.path().join(ROOT).join("support.txt"),
        "historical support only\n",
    )
    .expect("support");
    refresh_inventory(repo.path());
    repo
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("JSON serializes"),
    )
    .expect("JSON writes");
}

fn mutate(repo: &Path, file: &str, change: impl FnOnce(&mut Value)) {
    let path = repo.join(ROOT).join(file);
    let mut value =
        serde_json::from_slice(&fs::read(&path).expect("fixture reads")).expect("JSON parses");
    change(&mut value);
    write_json(&path, &value);
    refresh_inventory(repo);
}

fn refresh_inventory(repo: &Path) {
    let inventory: String = ["manifest.json", "owners.json", "support.txt"]
        .into_iter()
        .map(|file| {
            let path = format!("{ROOT}/{file}");
            format!(
                "{}  {path}\n",
                sha256(&fs::read(repo.join(&path)).expect("inventory input"))
            )
        })
        .collect();
    fs::write(
        repo.join(".omo/evidence/spec035/closure/f1-correction-artifacts.sha256"),
        inventory,
    )
    .expect("inventory writes");
}

fn inspect(repo: &Path) -> Result<(), Spec031ReleaseArtifactError> {
    let manifest: Classification = serde_json::from_slice(
        &fs::read(repo.join(ROOT).join("manifest.json")).expect("manifest reads"),
    )
    .expect("typed classification");
    validate_classification(repo, &manifest)
}

#[test]
fn spec035_v2_reads_complete_classification_without_execution_approval() {
    let repo = classification_fixture();
    assert_eq!(inspect(repo.path()), Ok(()));
    assert_eq!(
        validate_spec035_closure_evidence(repo.path()),
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
}

#[test]
fn spec035_v2_rejects_every_missing_authoritative_closure_row() {
    for index in 0..45 {
        let repo = classification_fixture();
        mutate(repo.path(), "manifest.json", |value| {
            value["requirements"]
                .as_array_mut()
                .expect("rows")
                .remove(index);
        });
        assert_eq!(
            inspect(repo.path()),
            Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement),
            "row {index}"
        );
    }
}

#[test]
fn spec035_v2_rejects_duplicate_unknown_and_reassigned_authority() {
    for (field, replacement) in [
        ("id", json!("PRD009-CE-4")),
        ("id", json!("PRD000-1")),
        ("authority", json!("PRD009")),
        ("item", json!(2)),
        ("line", json!(1)),
    ] {
        let repo = classification_fixture();
        mutate(repo.path(), "manifest.json", |value| {
            value["requirements"][0][field] = replacement
        });
        assert!(inspect(repo.path()).is_err(), "{field}");
    }
}

#[test]
fn spec035_v2_rejects_tampered_and_missing_artifacts() {
    let repo = classification_fixture();
    fs::write(repo.path().join(ROOT).join("support.txt"), "changed").expect("tamper");
    assert!(matches!(
        inspect(repo.path()),
        Err(Spec031ReleaseArtifactError::Spec035Evidence(_))
    ));
    fs::remove_file(repo.path().join(ROOT).join("support.txt")).expect("owned file removes");
    assert_eq!(
        inspect(repo.path()),
        Err(Spec031ReleaseArtifactError::MissingRequiredArtifact)
    );
}

#[test]
fn spec035_v2_rejects_stale_source_promoted_to_execution_binding() {
    let repo = classification_fixture();
    mutate(repo.path(), "manifest.json", |value| {
        value["provenance"]["observationsAreExecutionBinding"] = json!(true)
    });
    assert!(matches!(
        inspect(repo.path()),
        Err(Spec031ReleaseArtifactError::Spec035Evidence(_))
    ));
}

#[test]
fn spec035_v2_rejects_zero_execution_even_when_all_classifications_claim_pass() {
    let repo = classification_fixture();
    mutate(repo.path(), "manifest.json", |value| {
        value["verdict"] = json!("PASS");
        value["summary"]["passed"] = json!(45);
        value["summary"]["blocked"] = json!(0);
        value["summary"]["externalOwnersPassed"] = json!(6);
        value["summary"]["externalOwnersBlocked"] = json!(0);
        for row in value["requirements"].as_array_mut().expect("rows") {
            row["status"] = json!("PASS");
        }
        for gate in value["gates"].as_array_mut().expect("gates") {
            gate["status"] = json!("PASS");
        }
    });
    mutate(repo.path(), "owners.json", |value| {
        value["result"] = json!("PASS");
        value["summary"]["passed"] = json!(6);
        value["summary"]["blocked"] = json!(0);
        for owner in value["owners"].as_array_mut().expect("owners") {
            owner["status"] = json!("PASS");
            owner["facts"][0]["status"] = json!("PASS");
        }
    });
    assert_eq!(inspect(repo.path()), Ok(()));
    assert_eq!(
        validate_spec035_closure_evidence(repo.path()),
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
}

#[test]
fn spec035_v2_rejects_missing_duplicate_and_unknown_owner_sets() {
    for replacement in [json!("030"), json!("035")] {
        let repo = classification_fixture();
        mutate(repo.path(), "owners.json", |value| {
            value["owners"][0]["spec"] = replacement
        });
        assert_eq!(
            inspect(repo.path()),
            Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)
        );
    }
    let repo = classification_fixture();
    mutate(repo.path(), "owners.json", |value| {
        value["owners"].as_array_mut().expect("owners").pop();
    });
    assert!(inspect(repo.path()).is_err());
}

#[test]
fn spec035_v2_rejects_nested_verdicts_and_count_mismatches() {
    for (file, pointer, replacement) in [
        ("manifest.json", "/summary/passed", json!(45)),
        ("manifest.json", "/summary/prd007FinalConditions", json!(4)),
        ("owners.json", "/owners/0/status", json!("PASS")),
        ("owners.json", "/summary/historicalTestsFailed", json!(1)),
        ("owners.json", "/summary/newTestsExecuted", json!(1)),
    ] {
        let repo = classification_fixture();
        mutate(repo.path(), file, |value| {
            *value.pointer_mut(pointer).expect("field exists") = replacement
        });
        assert!(inspect(repo.path()).is_err(), "{pointer}");
    }
}
