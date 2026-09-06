use crate::support::*;
use serde_json::{json, Value};
use shacs_core::runtime::audit_spec034_release_artifacts_against;
use std::error::Error;
use std::path::Path;

#[test]
fn validation_rejects_digest_rebound_semantic_forgery() -> Result<(), Box<dyn Error>> {
    let baseline = release_evidence()?;
    let evidence = &baseline.evidence;
    let repo = &baseline.repo;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["command"]["gate"] = json!("full_cargo_gate");
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["command"]["filter"] = json!("forged-filter");
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        let command = &mut value["commands"][0]["command"];
        command["stderr_path"] = command["stdout_path"].clone();
        value["commands"][0]["stderr_digest"] = value["commands"][0]["stdout_digest"].clone();
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["command"]["duration_ms"] = json!(1);
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["portable_process_receipt"]["pid"] = json!(999999);
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["portable_process_receipt"]["stdout_temp_locator"] =
            json!(".spec034-schema-contract.stdout.tmp.999999.0");
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["portable_process_receipt"]["reaped"] = json!(false);
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["portable_process_receipt"]["temp_paths_published"] = json!(false);
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["source_digest"] = json!("sha256:forged");
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["tool"]["executable_digest"] = json!("sha256:forged");
    })?;
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["tool"]["version"] = json!("cargo forged");
    })?;
    let results = read_json(&evidence.join("results.json"))?;
    let serialized = serde_json::to_string(&results)?;
    for forbidden in [
        "pid",
        "duration_ms",
        "process_receipt",
        "stdout_temp_locator",
        "stderr_temp_locator",
    ] {
        assert!(!contains_key(&results, forbidden));
    }
    assert!(!serialized.contains(".tmp."));
    rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["command"]["tests"]["tests_run"] = json!(999);
    })?;
    rejects_json_mutation(evidence, repo, "coverage-matrix.json", |value| {
        value["schema"] = json!("forged.schema");
    })?;
    rejects_json_mutation(evidence, repo, "review-records.json", |value| {
        value["records"][0]["evidence"]["digest"] = json!("sha256:forged");
    })?;
    rejects_json_mutation(evidence, repo, "owner-audits.json", |value| {
        value["audits"][0]["evidence"]["locator"] = json!("summary.json");
    })?;
    rejects_extra_artifact(evidence, repo)?;
    Ok(())
}

#[test]
fn standalone_validation_rejects_writer_invalid_run_ids() -> Result<(), Box<dyn Error>> {
    let baseline = release_evidence()?;
    for run_id in ["invalid.id", "invalid id", &"a".repeat(81)] {
        rejects_run_id(&baseline.evidence, &baseline.repo, run_id)?;
    }
    Ok(())
}

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(key) || object.values().any(|value| contains_key(value, key))
        }
        Value::Array(values) => values.iter().any(|value| contains_key(value, key)),
        _ => false,
    }
}

fn rejects_json_mutation(
    evidence: &Path,
    repo: &Path,
    locator: &str,
    mutate: impl FnOnce(&mut Value),
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let copy = root.path().join("release");
    copy_tree(evidence, &copy)?;
    let path = copy.join(locator);
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    mutate(&mut value);
    std::fs::write(&path, serde_json::to_vec_pretty(&value)?)?;
    rebind_digest(&copy, locator)?;
    assert!(audit_spec034_release_artifacts_against(&copy, repo).is_err());
    Ok(())
}

fn rejects_extra_artifact(evidence: &Path, repo: &Path) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let copy = root.path().join("release");
    copy_tree(evidence, &copy)?;
    std::fs::write(copy.join("extra.json"), b"{}")?;
    let mut manifest = read_json(&copy.join("manifest.json"))?;
    manifest["artifact_digests"]
        .as_array_mut()
        .ok_or("artifact digests missing")?
        .push(json!({"locator": "extra.json", "digest": digest(&copy.join("extra.json"))?}));
    manifest["artifact_digests"]
        .as_array_mut()
        .ok_or("artifact digests missing")?
        .sort_by(|left, right| left["locator"].as_str().cmp(&right["locator"].as_str()));
    write_json(&copy.join("manifest.json"), &manifest)?;
    assert!(audit_spec034_release_artifacts_against(&copy, repo).is_err());
    Ok(())
}

fn rejects_run_id(evidence: &Path, repo: &Path, run_id: &str) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let copy = root.path().join("release");
    copy_tree(evidence, &copy)?;
    replace_run_ids(&copy, run_id)?;
    assert!(audit_spec034_release_artifacts_against(&copy, repo).is_err());
    Ok(())
}
