use super::task_method_calc_parse::{marker, status};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const MARKER: &str = "SHACS_CALC";
const CODE_LIMIT: usize = 2048;
const OUTPUT_LIMIT: usize = 1024;
pub(crate) const FIELD_LIMIT: usize = 256;
pub(crate) const LIST_LIMIT: usize = 16;
pub(crate) const RUN_LIMIT: usize = 16;
const CLOSED_EXTRA_RUNS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunRole {
    Primary,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    NonZeroExit,
    Denied,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcResult {
    pub role: RunRole,
    pub answer: String,
    pub unit: String,
    pub inputs: Vec<String>,
    pub assumptions: Vec<String>,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationRun {
    pub seq: u64,
    pub call_id: String,
    pub selection_revision: u64,
    pub code: String,
    pub code_digest: String,
    pub output: String,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub result: Option<CalcResult>,
    pub marker_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalcState {
    NotRun,
    Unavailable,
    Failed,
    Unrecorded,
    Unverified,
    Conflict,
    Verified,
}

impl CalcState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotRun => "not run",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Unrecorded => "not recorded",
            Self::Unverified => "not verified",
            Self::Conflict => "conflict",
            Self::Verified => "verified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcVerdict {
    pub selection_revision: u64,
    pub state: CalcState,
    pub reason: String,
    pub primary: Option<u64>,
    pub checks: Vec<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CalculationEvidence {
    pub runs: Vec<CalculationRun>,
    pub next_seq: u64,
    pub omitted_runs: usize,
    pub verdict: Option<CalcVerdict>,
    /// The first method revision whose runs still count: runs carry over a
    /// method change that keeps the calculation part and are judged again.
    pub since: u64,
}

impl CalculationEvidence {
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty() && self.omitted_runs == 0 && self.verdict.is_none()
    }

    pub(crate) fn push(&mut self, mut run: CalculationRun, revision: u64) {
        self.next_seq = self.next_seq.saturating_add(1);
        run.seq = self.next_seq;
        run.selection_revision = revision;
        self.runs.push(run);
    }

    pub(crate) fn find(&self, seq: u64) -> Option<&CalculationRun> {
        self.runs.iter().find(|run| run.seq == seq)
    }

    pub(crate) fn bound(&mut self) {
        let removed = self.runs.len().saturating_sub(RUN_LIMIT);
        self.runs.drain(..removed);
        self.omitted_runs = self.omitted_runs.saturating_add(removed);
    }

    pub(crate) fn keep_verdict_runs(&mut self) {
        let Some(verdict) = &self.verdict else {
            return;
        };
        let before = self.runs.len();
        let mut others = 0;
        let mut kept: Vec<CalculationRun> = Vec::new();
        for run in self.runs.drain(..).rev() {
            let referenced = verdict.primary == Some(run.seq) || verdict.checks.contains(&run.seq);
            if referenced || others < CLOSED_EXTRA_RUNS {
                others += usize::from(!referenced);
                kept.push(run);
            }
        }
        kept.reverse();
        self.omitted_runs = self.omitted_runs.saturating_add(before - kept.len());
        self.runs = kept;
    }
}

pub(crate) fn run(call_id: &str, command: &str, output: &str) -> CalculationRun {
    let (status, exit_code) = status(output);
    let (result, marker_error) = match marker(output) {
        (Some(_), _) if status != RunStatus::Succeeded => (
            None,
            Some("result line ignored because the run did not succeed".to_owned()),
        ),
        parsed => parsed,
    };
    CalculationRun {
        seq: 0,
        call_id: bounded(call_id, FIELD_LIMIT),
        selection_revision: 0,
        code: bounded_text(command, CODE_LIMIT),
        code_digest: format!("sha256:{:x}", Sha256::digest(command.trim().as_bytes())),
        output: bounded_text(output, OUTPUT_LIMIT),
        status,
        exit_code,
        result,
        marker_error,
    }
}

/// Text the redaction pass sees beyond a limit. Redaction cost grows with the
/// input, so only this window is redacted; secrets are far shorter than it.
pub(crate) const REDACTION_MARGIN: usize = 1024;

/// `text` redacted and cut to `limit` characters, on one line: control
/// characters (line breaks among them) become spaces, so the text cannot add
/// lines of its own where it is shown.
pub(crate) fn bounded(text: &str, limit: usize) -> String {
    cut(text, limit, true)
}

/// `text` redacted and cut to `limit` characters, keeping its lines (code,
/// program output and file paths).
pub(crate) fn bounded_text(text: &str, limit: usize) -> String {
    cut(text, limit, false)
}

fn cut(text: &str, limit: usize, one_line: bool) -> String {
    const SUFFIX: &str = " …[truncated]";
    let mut chars = text.chars();
    let window: String = chars
        .by_ref()
        .take(limit.saturating_add(REDACTION_MARGIN))
        .map(|ch| if one_line && ch.is_control() { ' ' } else { ch })
        .collect();
    let cut = chars.next().is_some();
    let redacted = shacs_redaction::redact_string(&window);
    if !cut && redacted.chars().count() <= limit {
        return redacted;
    }
    let keep = limit.saturating_sub(SUFFIX.chars().count());
    let mut kept: String = redacted.chars().take(keep).collect();
    kept.push_str(SUFFIX);
    kept
}
