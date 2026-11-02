use serde::Serialize;
use std::fmt::Display;

use super::memory_index::IndexStatus;
use super::memory_snapshot::ItemSource;
use super::search_ranking::{MatchedBy, SearchItem};

const MAX_DETAIL_CHARS: usize = 500;
const MEMORY_FILE: &str = "memory/MEMORY.md";
const HISTORY_FILE: &str = "memory/history.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SemanticReason {
    Disabled,
    ModelMissing,
    ModelUnverified,
    ModelLoadFailed,
    EmbeddingFailed,
    InvalidQueryEmbedding,
    NotRequested,
    SourceUnreadable,
    IndexUnavailable,
    IndexSuperseded,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum SemanticStatus {
    Available,
    Unavailable {
        reason: SemanticReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl SemanticStatus {
    /// Keeps the first reason only; the later ones follow from it.
    pub fn fail(&mut self, reason: SemanticReason, detail: impl Display) {
        if self.is_available() {
            *self = Self::Unavailable {
                reason,
                detail: Some(capped(detail)),
            };
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    fn without_detail(self) -> Self {
        match self {
            Self::Unavailable { reason, .. } => Self::Unavailable {
                reason,
                detail: None,
            },
            available => available,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct IndexCounts {
    vectors_remaining: usize,
    vectors_failed: usize,
    skipped_history_lines: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum IndexState {
    Complete(IndexCounts),
    Incomplete(IndexCounts),
    IndexUnavailable {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        skipped_history_lines: Option<usize>,
    },
}

impl IndexState {
    pub fn of(status: &IndexStatus) -> Self {
        let counts = IndexCounts {
            vectors_remaining: status.vectors_remaining,
            vectors_failed: status.vectors_failed,
            skipped_history_lines: status.skipped_history_lines,
        };
        if status.vectors_remaining == 0 {
            Self::Complete(counts)
        } else {
            Self::Incomplete(counts)
        }
    }

    /// `skipped_history_lines` is known only when the memory files were read.
    pub fn unavailable(reason: impl Display, skipped_history_lines: Option<usize>) -> Self {
        Self::IndexUnavailable {
            reason: capped(reason),
            skipped_history_lines,
        }
    }

    fn without_reason(self) -> Self {
        match self {
            Self::IndexUnavailable {
                skipped_history_lines,
                ..
            } => Self::IndexUnavailable {
                reason: "omitted".to_owned(),
                skipped_history_lines,
            },
            ready => ready,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorCode {
    EmptyQuery,
    SourceUnreadable,
    ResultLimitExceeded,
    SearchUnavailable,
}

#[derive(Debug, Clone, Serialize)]
struct SearchError {
    code: ErrorCode,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ResultSource {
    Memory {
        file: &'static str,
        first_line: usize,
        last_line: usize,
    },
    History {
        file: &'static str,
        cursor: u64,
        timestamp: String,
        item: usize,
    },
}

#[derive(Debug, Clone, Copy, Serialize)]
struct TextTruncated {
    original_chars: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ResultItem {
    rank: usize,
    key: String,
    source: ResultSource,
    text: String,
    matched_by: MatchedBy,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_truncated: Option<TextTruncated>,
}

/// What `memory_search` and `memory search --json` print.
#[derive(Debug, Clone, Serialize)]
pub(super) struct SearchResult {
    results: Vec<ResultItem>,
    semantic: SemanticStatus,
    index: IndexState,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<SearchError>,
}

impl SearchResult {
    pub fn found(
        merged: Vec<(&SearchItem, MatchedBy)>,
        semantic: SemanticStatus,
        index: IndexState,
    ) -> Self {
        let results = merged
            .into_iter()
            .enumerate()
            .map(|(position, (item, matched_by))| ResultItem {
                rank: position + 1,
                key: item.key.clone(),
                source: match &item.source {
                    ItemSource::Memory {
                        first_line,
                        last_line,
                    } => ResultSource::Memory {
                        file: MEMORY_FILE,
                        first_line: *first_line,
                        last_line: *last_line,
                    },
                    ItemSource::History {
                        cursor,
                        timestamp,
                        ordinal,
                    } => ResultSource::History {
                        file: HISTORY_FILE,
                        cursor: *cursor,
                        timestamp: timestamp.clone(),
                        item: *ordinal,
                    },
                },
                text: item.text.clone(),
                matched_by,
                text_truncated: None,
            })
            .collect();
        Self {
            results,
            semantic,
            index,
            error: None,
        }
    }

    pub fn empty_query(mut semantic: SemanticStatus, index: IndexState) -> Self {
        semantic.fail(SemanticReason::NotRequested, "the query has no terms");
        Self::failed(
            semantic,
            index,
            ErrorCode::EmptyQuery,
            "the query has no terms; write words to look for",
        )
    }

    pub fn source_unreadable(error: impl Display) -> Self {
        let detail = capped(error);
        Self::failed(
            SemanticStatus::Unavailable {
                reason: SemanticReason::SourceUnreadable,
                detail: Some(detail.clone()),
            },
            IndexState::unavailable(&detail, None),
            ErrorCode::SourceUnreadable,
            detail,
        )
    }

    pub fn search_unavailable() -> Self {
        let detail = "an earlier memory search in this process panicked; restart shacs-bot";
        Self::failed(
            SemanticStatus::Unavailable {
                reason: SemanticReason::IndexUnavailable,
                detail: Some(detail.to_owned()),
            },
            IndexState::unavailable(detail, None),
            ErrorCode::SearchUnavailable,
            detail,
        )
    }

    fn failed(
        semantic: SemanticStatus,
        index: IndexState,
        code: ErrorCode,
        detail: impl Display,
    ) -> Self {
        Self {
            results: Vec::new(),
            semantic,
            index,
            error: Some(SearchError {
                code,
                detail: capped(detail),
            }),
        }
    }

    /// The result as JSON of at most `max_chars` characters (0 is no limit). Over the limit, the
    /// texts are cut from the last rank up, each cut text marked with its length; when even
    /// empty texts do not fit, the result becomes a fixed one without results or free text.
    pub fn render(mut self, max_chars: usize) -> String {
        let full = to_json(&self);
        if max_chars == 0 || full.chars().count() <= max_chars {
            return full;
        }
        for index in (0..self.results.len()).rev() {
            let original = std::mem::take(&mut self.results[index].text);
            let original_chars = original.chars().count();
            self.results[index].text_truncated = Some(TextTruncated { original_chars });
            let Some(mut best) = self.fitted(index, &original, 0, max_chars) else {
                continue;
            };
            let (mut fits, mut too_long) = (0, original_chars);
            while too_long - fits > 1 {
                let middle = fits + (too_long - fits) / 2;
                match self.fitted(index, &original, middle, max_chars) {
                    Some(json) => {
                        fits = middle;
                        best = json;
                    }
                    None => too_long = middle,
                }
            }
            return best;
        }
        to_json(&Self {
            results: Vec::new(),
            semantic: self.semantic.without_detail(),
            index: self.index.without_reason(),
            error: Some(SearchError {
                code: ErrorCode::ResultLimitExceeded,
                detail: "result exceeds maxToolResultChars".to_owned(),
            }),
        })
    }

    fn fitted(
        &mut self,
        index: usize,
        original: &str,
        kept: usize,
        max_chars: usize,
    ) -> Option<String> {
        self.results[index].text = original.chars().take(kept).collect();
        let json = to_json(self);
        (json.chars().count() <= max_chars).then_some(json)
    }
}

fn to_json(result: &SearchResult) -> String {
    serde_json::to_string(result).unwrap_or_default()
}

fn capped(text: impl Display) -> String {
    text.to_string().chars().take(MAX_DETAIL_CHARS).collect()
}
