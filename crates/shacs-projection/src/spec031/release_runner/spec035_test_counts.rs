use super::command::parse_cargo_test_counts_strict;
use super::model::{Spec031ReleaseArtifactError as Error, Spec031ReleaseTestCounts};
use super::spec035_execution_model::FileRef;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Accounting {
    pub(super) schema: AccountingSchema,
    pub(super) run_id: String,
    pub(super) source_sha256: String,
    pub(super) stdout: FileRef,
    pub(super) stderr: FileRef,
    pub(super) targets: Vec<TargetSummary>,
}

#[derive(Deserialize)]
pub(super) enum AccountingSchema {
    #[serde(rename = "spec035.workspace_test_accounting.v1")]
    V1,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetSummary {
    pub(super) header: String,
    pub(super) summary_line: usize,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct HarnessTotals {
    pub targets: u64,
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct WorkspaceCounts {
    pub top_level: HarnessTotals,
    pub nested: HarnessTotals,
    pub top_level_summary_lines: Vec<usize>,
    pub nested_summary_lines: Vec<usize>,
}

impl WorkspaceCounts {
    pub(super) fn counts(&self) -> Spec031ReleaseTestCounts {
        Spec031ReleaseTestCounts {
            tests_run: self.top_level.passed,
            tests_failed: self.top_level.failed,
        }
    }
}

pub(super) fn workspace_counts(
    text: &str,
    stderr: &str,
    targets: &[TargetSummary],
) -> Result<WorkspaceCounts, Error> {
    let headers: Vec<_> = target_headers(stderr).collect();
    if targets.is_empty()
        || headers
            != targets
                .iter()
                .map(|target| target.header.as_str())
                .collect::<Vec<_>>()
    {
        return Err(Error::InvalidCommandEvidence);
    }
    let result = count_targets(text, true)?;
    if !result
        .top_level_summary_lines
        .iter()
        .copied()
        .eq(targets.iter().map(|target| target.summary_line))
    {
        return Err(Error::InvalidCommandEvidence);
    }
    Ok(result)
}

pub(super) fn flat_workspace_counts(
    text: &str,
    stderr: &str,
) -> Result<Spec031ReleaseTestCounts, Error> {
    let result = count_targets(text, false)?;
    if result.top_level_summary_lines.len() != target_headers(stderr).count() {
        return Err(Error::InvalidCommandEvidence);
    }
    Ok(result.counts())
}

fn target_headers(stderr: &str) -> impl Iterator<Item = &str> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("Running ") || line.starts_with("Doc-tests "))
}

fn count_targets(text: &str, allow_nested: bool) -> Result<WorkspaceCounts, Error> {
    let mut result = WorkspaceCounts::default();
    let mut outer = None;
    let mut children = Vec::new();
    for (offset, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("running ") {
            let announced = rest
                .strip_suffix(" tests")
                .or_else(|| rest.strip_suffix(" test"))
                .ok_or(Error::InvalidCommandEvidence)?
                .parse::<u64>()
                .map_err(|_| Error::InvalidCommandEvidence)?;
            match outer {
                None => outer = Some(announced),
                Some(_) if allow_nested => children.push(announced),
                Some(_) => return Err(Error::InvalidCommandEvidence),
            }
            continue;
        }
        if !line.starts_with("test result:") {
            if outer.is_none() && !line.is_empty() {
                return Err(Error::InvalidCommandEvidence);
            }
            continue;
        }
        let counts = parse_cargo_test_counts_strict(line)?;
        if counts.tests_failed != 0 {
            return Err(Error::NonzeroTestsFailed);
        }
        let parts: Vec<_> = line.split("; ").collect();
        let ignored = number(parts[2], " ignored")?;
        let measured = number(parts[3], " measured")?;
        let completed = add(add(counts.tests_run, ignored)?, measured)?;
        let totals = if outer == Some(completed) {
            if children.contains(&completed)
                || measured != 0
                || number(parts[4], " filtered out")? != 0
            {
                return Err(Error::InvalidCommandEvidence);
            }
            outer = None;
            children.clear();
            result.top_level_summary_lines.push(offset + 1);
            &mut result.top_level
        } else {
            let child = children
                .iter()
                .position(|announced| *announced == completed)
                .ok_or(Error::InvalidCommandEvidence)?;
            children.swap_remove(child);
            result.nested_summary_lines.push(offset + 1);
            &mut result.nested
        };
        totals.targets = add(totals.targets, 1)?;
        totals.passed = add(totals.passed, counts.tests_run)?;
        totals.ignored = add(totals.ignored, ignored)?;
    }
    if outer.is_some() || result.top_level.targets == 0 {
        return Err(Error::InvalidCommandEvidence);
    }
    Ok(result)
}

fn number(text: &str, suffix: &str) -> Result<u64, Error> {
    text.strip_suffix(suffix)
        .ok_or(Error::InvalidCommandEvidence)?
        .parse()
        .map_err(|_| Error::InvalidCommandEvidence)
}

fn add(left: u64, right: u64) -> Result<u64, Error> {
    left.checked_add(right).ok_or(Error::InvalidCommandEvidence)
}
