use crate::support::*;
use serde_json::json;
use shacs_core::runtime::audit_spec034_release_artifacts_against;
use std::error::Error;
use std::path::Path;

#[test]
fn validation_rejects_digest_rebound_path_bypasses() -> Result<(), Box<dyn Error>> {
    let baseline = release_evidence()?;
    let mut long_csi = b"\n\x1b[".to_vec();
    long_csi.extend(std::iter::repeat(b'?').take(4_096));
    long_csi.extend_from_slice(b"l/Users/alice/private.txt\n");
    rejects_stdout_path_mutation(&baseline.evidence, &baseline.repo, &long_csi)?;
    for payload in [
        b"\nhttps://example.com/a?next=/Users/alice/private.txt\n".as_slice(),
        b"\nhttps://example.com/a?next=%2FUsers%2Falice%2Fprivate.txt\n".as_slice(),
        b"\nhttps://%@example.com/a?next=/Users/alice/private.txt\n".as_slice(),
        b"\n\x1b]8;;https://example.com\x1b\\/Users/alice/private.txt\n".as_slice(),
        b"\n\x1bc/Users/alice/private.txt\n".as_slice(),
        b"\n\x1b[2 q/Users/alice/private.txt\n".as_slice(),
        "\n\u{9b}31m/Users/alice/private.txt\n".as_bytes(),
    ] {
        rejects_stdout_path_mutation(&baseline.evidence, &baseline.repo, payload)?;
    }
    Ok(())
}

fn rejects_stdout_path_mutation(
    evidence: &Path,
    repo: &Path,
    payload: &[u8],
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let copy = root.path().join("release");
    copy_tree(evidence, &copy)?;
    let locator = "spec034-schema-contract.stdout";
    let path = copy.join(locator);
    let mut bytes = std::fs::read(&path)?;
    bytes.extend_from_slice(payload);
    std::fs::write(&path, bytes)?;
    let rebound = digest(&path)?;
    let results_path = copy.join("results.json");
    let mut results = read_json(&results_path)?;
    results["commands"][0]["stdout_digest"] = json!(rebound);
    write_json(&results_path, &results)?;
    for (document, rows) in [
        ("coverage-matrix.json", "blockers"),
        ("review-records.json", "records"),
    ] {
        let path = copy.join(document);
        let mut value = read_json(&path)?;
        for row in value[rows].as_array_mut().into_iter().flatten() {
            row["evidence"]["digest"] = json!(rebound);
        }
        write_json(&path, &value)?;
        rebind_digest(&copy, document)?;
    }
    rebind_digest(&copy, locator)?;
    rebind_digest(&copy, "results.json")?;
    assert!(audit_spec034_release_artifacts_against(&copy, repo).is_err());
    Ok(())
}
