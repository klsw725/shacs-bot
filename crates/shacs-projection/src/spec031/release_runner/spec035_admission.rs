use super::model::{Spec031ReleaseArtifactError as Error, Spec031ReleaseRunId};
use super::spec035_admission_model::*;
use super::spec035_evidence_io::{evidence_error, sha256};
use super::spec035_execution_io::{decode, read_bound};
use super::spec035_execution_model::FileRef;
use super::spec035_test_counts::{workspace_counts, TargetSummary};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn inspect_spec035_receipt_admission(
    repo: &Path,
    adapter: &str,
    expected_run_id: &str,
) -> Result<Spec035ReceiptAdmissionReport, Error> {
    let bytes =
        std::fs::read(super::validate::require_safe_file(repo, adapter)?).map_err(|_| Error::Io)?;
    inspect_bytes(repo, &bytes, expected_run_id)
}

pub(super) fn inspect_bytes(
    repo: &Path,
    bytes: &[u8],
    expected_run_id: &str,
) -> Result<Spec035ReceiptAdmissionReport, Error> {
    let admission: Admission = decode(bytes)?;
    match admission.schema {
        AdmissionSchema::V1 => {}
    }
    Spec031ReleaseRunId::try_new(&admission.run_id)?;
    if admission.run_id != expected_run_id {
        return Err(evidence_error("admission run identity mismatch"));
    }
    let prefix = format!(".omo/evidence/spec035/{}/", admission.run_id);
    let mut artifacts = BTreeMap::new();
    for file in &admission.artifacts {
        let name = file
            .path
            .strip_prefix(&prefix)
            .ok_or(Error::InvalidArtifactPath)?;
        if artifacts.insert(name, read_bound(repo, file)?).is_some() {
            return Err(Error::InvalidCommandEvidence);
        }
    }
    let get = |name: &str| {
        artifacts
            .get(name)
            .map(Vec::as_slice)
            .ok_or(Error::MissingRequiredArtifact)
    };
    let inventory = text(get("artifact-hashes.sha256")?)?;
    let mut inventoried = BTreeSet::new();
    for line in inventory.lines() {
        let (hash, name) = line.split_once("  ").ok_or(Error::InvalidCommandEvidence)?;
        if !inventoried.insert(name) {
            return Err(Error::InvalidCommandEvidence);
        }
        read_bound(
            repo,
            &FileRef {
                path: format!("{prefix}{name}"),
                sha256: hash.to_owned(),
            },
        )?;
    }
    if artifacts
        .keys()
        .any(|name| *name != "artifact-hashes.sha256" && !inventoried.contains(name))
    {
        return Err(Error::InvalidCommandEvidence);
    }
    let source_bytes = get("source-verified.json")?;
    if source_bytes != get("source-after.json")? {
        return Err(evidence_error("historical source changed during run"));
    }
    let source = super::spec035_admission_source::snapshot(source_bytes)?;
    let audit: WorkspaceAudit = decode(get("audit.json")?)?;
    let receipt: WorkspaceReceipt = decode(get("workspace.json")?)?;
    let argv: Vec<_> = audit.command.iter().map(String::as_str).collect();
    if !super::spec035_execution_commands::workspace_argv(&argv)
        || audit.command.get(1..) != Some(receipt.args.as_slice())
        || audit.head != admission.original_head
        || source.head != admission.original_head
        || audit.source_digest != source.inventory_digest
        || audit.lockfile_digest != source.lockfile_digest
        || audit.started_at != admission.started_at
        || receipt.started_at != admission.started_at
        || audit.finished_at != admission.finished_at
        || receipt.finished_at != admission.finished_at
        || admission.started_at.is_empty()
        || admission.finished_at <= admission.started_at
        || audit.executions != 1
        || audit.code != 0
        || audit.timed_out
        || receipt.code != 0
        || receipt.timed_out
        || receipt.signal.is_some()
        || receipt.stdout_digest != format!("sha256:{}", sha256(get("workspace.stdout")?))
        || receipt.stderr_digest != format!("sha256:{}", sha256(get("workspace.stderr")?))
    {
        return Err(evidence_error(
            "historical command/run/source identity mismatch",
        ));
    }
    let regular: Vec<OriginalTarget> = decode(get("top-level-test-targets.json")?)?;
    let docs: Vec<OriginalTarget> = decode(get("doc-test-outcomes.json")?)?;
    let mut targets = Vec::new();
    for (rows, prefix) in [(&regular, "Running "), (&docs, "Doc-tests ")] {
        for row in rows {
            if row.nested {
                return Err(Error::InvalidCommandEvidence);
            }
            targets.push(TargetSummary {
                header: format!("{prefix}{}", row.target),
                summary_line: row.stdout_line,
            });
        }
    }
    let stdout = text(get("workspace.stdout")?)?;
    let workspace = workspace_counts(stdout, text(get("workspace.stderr")?)?, &targets)?;
    let lines: Vec<_> = stdout.lines().collect();
    for row in regular.iter().chain(&docs) {
        let summary = lines[row.stdout_line - 1];
        let counts = super::command::parse_cargo_test_counts_strict(summary)?;
        let ignored: u64 = summary
            .split("; ")
            .nth(2)
            .and_then(|part| part.strip_suffix(" ignored"))
            .ok_or(Error::InvalidCommandEvidence)?
            .parse()
            .map_err(|_| Error::InvalidCommandEvidence)?;
        if counts.tests_run != row.passed
            || counts.tests_failed != row.failed
            || ignored != row.ignored
        {
            return Err(Error::InvalidCommandEvidence);
        }
    }
    if !audit.combined_totals.matches(&workspace.top_level)
        || !audit.nested_totals_excluded.matches(&workspace.nested)
        || audit.top_level_targets != regular.len()
        || audit.doc_tests.targets != docs.len()
        || workspace.top_level.passed == 0
    {
        return Err(Error::InvalidCommandEvidence);
    }
    let config_incident = super::spec035_admission_incident::accepted_incident(&read_bound(
        repo,
        &admission.config_acceptance,
    )?)?;
    let source =
        super::spec035_admission_source::correspondence(repo, source, &admission.target_commit)?;
    Ok(Spec035ReceiptAdmissionReport {
        schema: "spec035.receipt_admission_report.v1",
        run_id: admission.run_id,
        adapter_sha256: sha256(bytes),
        workspace,
        regular_targets: regular.len(),
        doc_targets: docs.len(),
        artifacts_verified: inventoried.len(),
        source,
        config_incident,
        semantic_closure_evaluated: false,
        release_verdict: ReleaseDisposition::Blocked,
        multi_run_composition: CompositionDisposition::NotAdmitted,
    })
}

fn text(bytes: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(bytes).map_err(|_| Error::InvalidCommandEvidence)
}
