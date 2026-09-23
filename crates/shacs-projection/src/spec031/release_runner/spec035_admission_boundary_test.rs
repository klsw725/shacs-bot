use super::spec035_admission_incident::accepted_incident;
use super::spec035_evidence_io::sha256;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn acceptance() -> Value {
    json!({
        "schema":"spec035.config_user_baseline_acceptance.v1",
        "incident":"default-config-rewrite",
        "disposition":"USER_ACCEPTED_REPAIRED_BASELINE",
        "preincident_backup_available":false,
        "user_confirmed_intent":{},
        "authorized_changes":[
            {"key":"agents.defaults.workspace","value":"fixture","backup":"fixture"},
            {"key":"agents.defaults.model","value":"fixture","backup":"fixture"}
        ],
        "verification":{
            "each_edit_changed_only_authorized_value":true,
            "config_permissions":"0600",
            "provider_matches_user_intent":true,
            "discord_enabled":true,
            "discord_credential_field_nonempty":true,
            "credential_values_disclosed":false
        },
        "limits":["constructed fixture, not a user acceptance receipt"]
    })
}

#[test]
fn spec035_admission_acceptance_never_becomes_verified_restoration() {
    let bytes = serde_json::to_vec(&acceptance()).expect("fixture");
    let report = accepted_incident(&bytes).expect("scoped acceptance");
    let result = serde_json::to_value(report).expect("report");
    assert_eq!(result["disposition"], "USER_ACCEPTED_REPAIRED_BASELINE");
    assert_eq!(result["preincident_restoration"], "unverified");
    assert_eq!(result["credential_validity"], "unverified");
    assert!(result.get("verdict").is_none());
}

#[test]
fn spec035_admission_rejects_accepted_incident_overclaims_and_wrong_subjects() {
    for (key, value) in [
        ("disposition", json!("PASS")),
        ("incident", json!("missing-spec034-fixture")),
        ("preincident_backup_available", json!(true)),
        ("preincident_restoration", json!("verified")),
        ("credential_validity", json!("verified")),
        (
            "schema",
            json!("spec035.config_user_baseline_acceptance.v2"),
        ),
        ("limits", json!([])),
    ] {
        let mut receipt = acceptance();
        receipt[key] = value;
        assert!(
            accepted_incident(&serde_json::to_vec(&receipt).expect("fixture")).is_err(),
            "{key}"
        );
    }
}

#[test]
fn spec035_admission_rejects_forged_source_inventory_digest() {
    let snapshot = json!({
        "head":"old-head", "inventoryDigest":"sha256:forged", "lockfileDigest":"sha256:lock",
        "files":[{"locator":"crates/Cargo.lock","digest":"sha256:lock"},{"locator":"crates/Cargo.toml","digest":"sha256:manifest"}]
    });
    assert!(super::spec035_admission_source::snapshot(
        &serde_json::to_vec(&snapshot).expect("snapshot")
    )
    .is_err());
}

#[test]
#[ignore = "requires preserved real evidence; mutates in-memory adapter copies only"]
fn spec035_admission_rejects_stale_mismatched_run_and_source_identity() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(repo.join(".omo/evidence/spec035/closure/receipt-admission-20260918.json"))
            .expect("adapter");
    let adapter: Value = serde_json::from_slice(&bytes).expect("adapter JSON");
    assert!(super::spec035_admission::inspect_bytes(&repo, &bytes, "stale-run").is_err());
    for (key, value) in [
        ("run_id", json!("stale-run")),
        (
            "original_head",
            json!("d8f7554f05af89d2c00ce24f8988138eceefde81"),
        ),
        (
            "target_commit",
            json!("0000000000000000000000000000000000000000"),
        ),
        ("started_at", json!("2026-09-18T11:09:59.838Z")),
        ("schema", json!("spec035.receipt_admission.v2")),
    ] {
        let mut changed = adapter.clone();
        changed[key] = value;
        assert!(
            super::spec035_admission::inspect_bytes(
                &repo,
                &serde_json::to_vec(&changed).expect("copy"),
                "workspace-no-fail-fast-20260917"
            )
            .is_err(),
            "{key}"
        );
    }
    let mut changed = adapter;
    changed["artifacts"][0]["sha256"] = json!("0".repeat(64));
    assert!(super::spec035_admission::inspect_bytes(
        &repo,
        &serde_json::to_vec(&changed).expect("copy"),
        "workspace-no-fail-fast-20260917"
    )
    .is_err());
}

#[test]
#[ignore = "requires preserved source receipt and committed tree; in-memory mutation only"]
fn spec035_admission_rejects_version_labels_without_byte_correspondence() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes = std::fs::read(
        repo.join(".omo/evidence/spec035/workspace-no-fail-fast-20260917/source-verified.json"),
    )
    .expect("original snapshot");
    assert!(
        super::spec035_execution_io::decode::<Vec<super::spec035_execution_model::FileRef>>(&bytes)
            .is_err()
    );
    let mut snapshot: Value = serde_json::from_slice(&bytes).expect("snapshot");
    snapshot["files"][0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    let files: Vec<super::spec035_admission_source::SourceFile> =
        serde_json::from_value(snapshot["files"].clone()).expect("typed source files");
    snapshot["inventoryDigest"] = json!(format!(
        "sha256:{}",
        sha256(&serde_json::to_vec(&files).expect("files"))
    ));
    let source =
        super::spec035_admission_source::snapshot(&serde_json::to_vec(&snapshot).expect("copy"))
            .expect("mutation reaches committed-byte check");
    assert!(super::spec035_admission_source::correspondence(
        &repo,
        source,
        "d8f7554f05af89d2c00ce24f8988138eceefde81"
    )
    .is_err());
}

#[test]
#[ignore = "requires retained immutable evidence inventories; hash checks only"]
fn spec035_admission_preserves_original_evidence_and_two_document_changes() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Reconciliation {
        immutable_inputs: Vec<super::spec035_execution_model::FileRef>,
    }
    #[derive(serde::Deserialize)]
    struct Inventory {
        entries: Vec<super::spec035_execution_model::FileRef>,
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| std::fs::read(repo.join(path)).expect("retained input");
    let prior: Reconciliation = serde_json::from_slice(&read(
        ".omo/evidence/spec035/closure/final-reconciliation-source-20260918.json",
    ))
    .expect("source ledger");
    for file in prior.immutable_inputs {
        super::spec035_execution_io::read_bound(&repo, &file).expect("immutable input unchanged");
    }
    let compiled: Inventory = serde_json::from_slice(&read(
        ".omo/evidence/spec035/closure/final-compiled-artifacts.json",
    ))
    .expect("compiled inventory");
    assert_eq!(compiled.entries.len(), 260);
    for file in compiled.entries {
        super::spec035_execution_io::read_bound(&repo, &file).expect("compiled evidence unchanged");
    }
    for (file, hash) in [
        (
            "CLOSURE.md",
            "a4eec6073d8e1e84448c112256c9091ad97bfc6922f824625b4f91348023174b",
        ),
        (
            "SPEC.md",
            "a850344cd16ee15b0423e6ed76d0628451009add9cbfd34406bb3131aac3e472",
        ),
    ] {
        assert_eq!(
            sha256(&read(&format!(
                "docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/{file}"
            ))),
            hash
        );
    }
    let acceptance = read(".omo/evidence/spec035/closure/config-user-baseline-acceptance.json");
    assert!(
        super::spec035_execution_io::decode::<super::spec035_execution_model::Receipt>(&acceptance)
            .is_err()
    );
    assert!(accepted_incident(&acceptance).is_ok());
    let canonical = super::spec035_execution_io::decode(&read(
        ".omo/evidence/spec035/prd000-009/manifest.json",
    ))
    .expect("canonical classification");
    assert!(matches!(
        canonical,
        super::spec035_classification_model::ClosureDocument::ClassificationV2(_)
    ));
}
