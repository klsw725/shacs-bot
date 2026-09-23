use super::spec035_catalog::{catalog, SPEC_ROOT};
use super::spec035_execution_fixture::{file_ref, write_json, Fixture};
use serde_json::json;

pub(super) const DEFERRED: [&str; 6] = [
    "spec035:PRD007-FC-5",
    "spec035:must:10",
    "spec035:acceptance:08",
    "spec035:acceptance:09",
    "spec035:closure:07",
    "spec035:closure:08",
];

pub(super) fn pending_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    for row in fixture.manifest["requirements"]
        .as_array_mut()
        .expect("rows")
    {
        if DEFERRED.contains(&row["authority"]["id"].as_str().expect("id")) {
            let path = row["receipt"]["path"].as_str().expect("path").to_owned();
            let mut receipt: serde_json::Value =
                serde_json::from_slice(&std::fs::read(fixture.root.join(&path)).expect("receipt"))
                    .expect("json");
            receipt["verdict"] = json!("BLOCKED");
            receipt["checks"][0]["verdict"] = json!("BLOCKED");
            receipt["checks"][0]["commands"] = json!([]);
            receipt["checks"][0]["artifacts"] = json!([]);
            write_json(&fixture.root.join(&path), &receipt);
            row["receipt"] = file_ref(&fixture.root, &path);
        }
    }
    fixture.save();
    fixture
}

#[test]
fn spec035_preflight_authorizes_only_catalog_derived_postrun_dependencies() {
    let fixture = pending_fixture();
    let preflight = super::spec035_execution::preflight(fixture.repo.path()).expect("preflight");
    let blocked: std::collections::HashSet<_> = catalog()
        .into_iter()
        .filter(|row| preflight.proof_for(row).is_none())
        .map(|row| row.id)
        .collect();
    assert_eq!(blocked, DEFERRED.map(str::to_owned).into_iter().collect());
    assert!(super::spec035_execution::admit(fixture.repo.path()).is_err());
}

#[test]
fn spec035_authority_tracks_sections_in_provided_repo_not_compiled_checkout() {
    let fixture = Fixture::new();
    let path = format!("{SPEC_ROOT}/prds/003-readiness-degraded-health-and-diagnostics.md");
    let text = std::fs::read_to_string(fixture.repo.path().join(&path)).expect("authority");
    let first = text
        .lines()
        .position(|line| line.starts_with("1. **Unavailable**"))
        .expect("first closure row")
        + 1;
    std::fs::write(fixture.repo.path().join(&path), format!("\n\n{text}")).expect("shift");
    let rows = super::spec035_catalog::catalog_at(fixture.repo.path()).expect("dynamic authority");
    let row = rows
        .iter()
        .find(|row| row.id == "spec035:PRD003-CE-1")
        .expect("row");
    assert_eq!(row.source_locator, format!("{path}:{}", first + 2));
}

#[test]
fn spec035_authority_rejects_duplicate_missing_and_malformed_numbered_rows() {
    for replacement in ["2. duplicate", "", "x. malformed", "01. malformed"] {
        let fixture = Fixture::new();
        let path = fixture.repo.path().join(format!(
            "{SPEC_ROOT}/prds/003-readiness-degraded-health-and-diagnostics.md"
        ));
        let text = std::fs::read_to_string(&path).expect("authority");
        let mut lines: Vec<_> = text.lines().collect();
        let first = lines
            .iter()
            .position(|line| line.starts_with("1. **Unavailable**"))
            .expect("closure");
        lines[first] = replacement;
        std::fs::write(path, lines.join("\n")).expect("mutation");
        assert!(
            super::spec035_catalog::catalog_at(fixture.repo.path()).is_err(),
            "{replacement}"
        );
    }
}

#[test]
fn spec035_preflight_never_defers_observed_failures_or_unbound_bytes() {
    for mutation in [
        "failed",
        "observed-block",
        "hash",
        "source",
        "command",
        "owner",
        "gate",
    ] {
        let mut fixture = pending_fixture();
        let row = fixture.manifest["requirements"]
            .as_array_mut()
            .expect("rows")
            .iter_mut()
            .find(|row| row["authority"]["id"] == "spec035:PRD007-FC-5")
            .expect("FC5");
        let path = row["receipt"]["path"].as_str().expect("receipt").to_owned();
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture.root.join(&path)).expect("read"))
                .expect("receipt");
        match mutation {
            "failed" => receipt["checks"][0]["verdict"] = json!("FAILED"),
            "observed-block" => {
                receipt["checks"][0]["artifacts"] =
                    json!([file_ref(&fixture.root, "observation.json")])
            }
            "hash" => receipt["checks"][0]["producer"] = json!("crates/probe.rs:2"),
            "source" => {
                std::fs::write(fixture.repo.path().join("crates/probe.rs"), "stale")
                    .expect("source");
            }
            "command" => fixture.manifest["commands"][0]["exit_code"] = json!(1),
            "owner" => {
                fixture.manifest["owners"]
                    .as_array_mut()
                    .expect("owners")
                    .pop();
            }
            "gate" => {
                fixture.manifest["gates"]
                    .as_array_mut()
                    .expect("gates")
                    .pop();
            }
            _ => unreachable!(),
        }
        write_json(&fixture.root.join(&path), &receipt);
        if mutation != "hash" {
            let row = fixture.manifest["requirements"]
                .as_array_mut()
                .expect("rows")
                .iter_mut()
                .find(|row| row["authority"]["id"] == "spec035:PRD007-FC-5")
                .expect("FC5");
            row["receipt"] = file_ref(&fixture.root, &path);
        }
        fixture.save();
        assert!(
            super::spec035_execution::preflight(fixture.repo.path()).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn spec035_preflight_requires_all_eighty_rows_and_rejects_other_pending_rows() {
    for mutation in ["missing", "duplicate", "other-pending"] {
        let mut fixture = pending_fixture();
        let rows = fixture.manifest["requirements"]
            .as_array_mut()
            .expect("rows");
        match mutation {
            "missing" => {
                rows.pop();
            }
            "duplicate" => {
                rows[0] = rows[1].clone();
            }
            "other-pending" => {
                let path = rows[0]["receipt"]["path"]
                    .as_str()
                    .expect("path")
                    .to_owned();
                let mut receipt: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(fixture.root.join(&path)).expect("read"))
                        .expect("json");
                receipt["verdict"] = json!("BLOCKED");
                receipt["checks"][0]["verdict"] = json!("BLOCKED");
                receipt["checks"][0]["commands"] = json!([]);
                receipt["checks"][0]["artifacts"] = json!([]);
                write_json(&fixture.root.join(&path), &receipt);
                rows[0]["receipt"] = file_ref(&fixture.root, &path);
            }
            _ => unreachable!(),
        }
        fixture.save();
        assert!(
            super::spec035_execution::preflight(fixture.repo.path()).is_err(),
            "{mutation}"
        );
    }
}
