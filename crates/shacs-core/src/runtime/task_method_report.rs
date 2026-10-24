pub(crate) use super::task_method_report_source::{
    digest, failure, file_state, freshness, read_source,
};
use serde::{Deserialize, Serialize};

pub(crate) const LOCATOR_LIMIT: usize = 256;
pub(crate) const TEXT_LIMIT: usize = 256;
pub(crate) const SOURCE_LIMIT: usize = 32;
pub(crate) const CLAIM_LIMIT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    File,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Read,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportSource {
    pub seq: u64,
    pub call_id: String,
    pub selection_revision: u64,
    pub version: u64,
    pub carried: bool,
    pub kind: SourceKind,
    pub locator: String,
    #[serde(default)]
    pub match_keys: Vec<String>,
    pub status: SourceStatus,
    pub content_digest: Option<String>,
    pub file_digest: Option<String>,
    pub resolved_path: Option<String>,
    #[serde(default)]
    pub requested_path: Option<String>,
    pub read_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationStatus {
    Linked,
    LinkedUnchanged,
    Stale,
    CarriedUnchecked,
    Unread,
    ReadFailed,
    /// `[calculation]` in a report file: only the runtime's own line in the
    /// answer states the checked result, so the file's words are not checked.
    ResultUnchecked,
    ResultMissing,
}

impl CitationStatus {
    pub fn linked(self) -> bool {
        matches!(self, Self::Linked | Self::LinkedUnchanged)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Linked => "read in this request",
            Self::LinkedUnchanged => "read in an earlier version; file unchanged since",
            Self::Stale => "source changed after it was read",
            Self::CarriedUnchecked => "read in an earlier version; not re-checked",
            Self::Unread => "never read",
            Self::ReadFailed => "read failed",
            Self::ResultUnchecked => {
                "not checked; only the runtime's [calculation] line in the answer shows the checked result"
            }
            Self::ResultMissing => "no such part in this request",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub locator: String,
    #[serde(default)]
    pub key: String,
    pub status: CitationStatus,
    pub source: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportClaim {
    pub text: String,
    pub citations: Vec<Citation>,
    pub unresolved: bool,
    pub conflict: Vec<String>,
    /// False for a cited heading, lead-in or file pointer: its citations are
    /// still checked, but it never counts as report content.
    #[serde(default = "statement")]
    pub statement: bool,
    /// Quotes a cited file read in this request word for word (judged only
    /// where a composed request has an image part).
    #[serde(default)]
    pub quoted: bool,
}

fn statement() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Answer,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportOutput {
    pub kind: OutputKind,
    pub locator: String,
    /// Exact-match key of the whole locator (the stored one is bounded).
    #[serde(default)]
    pub key: String,
    pub selection_revision: u64,
    pub digest: String,
    pub claims: Vec<ReportClaim>,
    pub uncited_lines: usize,
    #[serde(default)]
    pub uncited: Vec<String>,
    #[serde(default)]
    pub unchecked: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportState {
    NotWritten,
    NoSources,
    Unavailable,
    Uncited,
    Problems,
    PartlyCited,
    Linked,
}

impl ReportState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotWritten => "not written",
            Self::NoSources => "no sources read",
            Self::Unavailable => "unavailable",
            Self::Uncited => "no citations",
            Self::Problems => "citation problems",
            Self::PartlyCited => "partly cited",
            Self::Linked => "source-linked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportVerdict {
    pub selection_revision: u64,
    pub state: ReportState,
    pub reason: String,
    pub claims: Vec<ReportClaim>,
    pub uncited_lines: usize,
    #[serde(default)]
    pub uncited: Vec<String>,
    #[serde(default)]
    pub unchecked: usize,
    pub changed_claims: usize,
    #[serde(default)]
    pub removed_claims: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReportEvidence {
    pub version: u64,
    pub revision_of: Option<String>,
    pub purpose: Option<String>,
    pub previous_claims: Vec<String>,
    pub sources: Vec<ReportSource>,
    pub next_seq: u64,
    pub omitted_sources: usize,
    pub outputs: Vec<ReportOutput>,
    pub dropped_outputs: Vec<u64>,
    pub verdict: Option<ReportVerdict>,
}

impl Default for ReportEvidence {
    fn default() -> Self {
        Self {
            version: 1,
            revision_of: None,
            purpose: None,
            previous_claims: Vec::new(),
            sources: Vec::new(),
            next_seq: 0,
            omitted_sources: 0,
            outputs: Vec::new(),
            dropped_outputs: Vec::new(),
            verdict: None,
        }
    }
}

impl ReportEvidence {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
            && self.outputs.is_empty()
            && self.verdict.is_none()
            && self.revision_of.is_none()
            && self.omitted_sources == 0
            && self.dropped_outputs.is_empty()
    }
}
