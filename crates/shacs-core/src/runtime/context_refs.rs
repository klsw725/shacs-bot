use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReferenceParse {
    pub original_message: String,
    pub references: Vec<ContextReferenceSpan>,
    pub diagnostics: Vec<ReferenceParseDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReferenceSpan {
    pub start: usize,
    pub end: usize,
    pub raw_token: String,
    pub normalized_target: String,
    pub kind: ContextReferenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextReferenceKind {
    File,
    Folder,
    Diff,
    Staged,
    Git,
    Url,
    Unsupported,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceParseDiagnostic {
    pub start: usize,
    pub end: usize,
    pub raw_token: String,
    pub kind: ReferenceParseDiagnosticKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceParseDiagnosticKind {
    Escaped,
    CodeBlockIgnored,
    Ambiguous,
    Unsupported,
    MissingTarget,
    MalformedTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedContextArtifact {
    pub kind: ContextReferenceKind,
    pub source: String,
    pub display_name: String,
    pub content: Option<String>,
    pub digest: Option<String>,
    pub byte_count: Option<usize>,
    pub token_estimate: Option<usize>,
    pub redaction_status: ContextRedactionStatus,
    pub truncation_status: ContextTruncationStatus,
    pub permission_evidence: ContextPermissionEvidence,
    pub state: ContextResolutionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRedactionStatus {
    NotApplied,
    Redacted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTruncationStatus {
    NotApplied,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPermissionEvidence {
    pub status: ContextPermissionStatus,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPermissionStatus {
    NotChecked,
    Allowed,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextResolutionState {
    Parsed,
    Resolved,
    Skipped,
    Denied,
    Failed,
}

impl ResolvedContextArtifact {
    pub fn parsed_shell(reference: &ContextReferenceSpan) -> Self {
        Self {
            kind: reference.kind,
            source: reference.normalized_target.clone(),
            display_name: reference.normalized_target.clone(),
            content: None,
            digest: None,
            byte_count: None,
            token_estimate: None,
            redaction_status: ContextRedactionStatus::NotApplied,
            truncation_status: ContextTruncationStatus::NotApplied,
            permission_evidence: ContextPermissionEvidence {
                status: ContextPermissionStatus::NotChecked,
                evidence: None,
            },
            state: ContextResolutionState::Parsed,
        }
    }
}

pub fn parse_context_references(message: &str) -> ContextReferenceParse {
    let code_ranges = fenced_code_ranges(message);
    let mut references = Vec::new();
    let mut diagnostics = Vec::new();
    let mut index = 0;

    while let Some(relative) = message[index..].find('@') {
        let at = index + relative;
        if let Some(range) = containing_range(&code_ranges, at) {
            let end = token_end(message, at + 1);
            diagnostics.push(diagnostic(
                at,
                end,
                &message[at..end],
                ReferenceParseDiagnosticKind::CodeBlockIgnored,
                "reference-like token ignored inside fenced code block",
            ));
            index = range.end.max(end);
            continue;
        }

        if is_escaped_at(message, at) {
            let end = token_end(message, at + 1);
            diagnostics.push(diagnostic(
                at,
                end,
                &message[at..end],
                ReferenceParseDiagnosticKind::Escaped,
                "escaped @ token ignored",
            ));
            index = end;
            continue;
        }

        let end = token_end(message, at + 1);
        let raw = &message[at..end];
        let target = &message[at + 1..end];
        if target.is_empty() {
            diagnostics.push(diagnostic(
                at,
                end,
                raw,
                ReferenceParseDiagnosticKind::MissingTarget,
                "@ reference is missing a target",
            ));
            references.push(span(at, end, raw, target, ContextReferenceKind::Unresolved));
            index = end.max(at + 1);
            continue;
        }

        if previous_is_word(message, at) || is_handle_like(target) {
            diagnostics.push(diagnostic(
                at,
                end,
                raw,
                ReferenceParseDiagnosticKind::Ambiguous,
                "ambiguous email or handle-like token ignored",
            ));
            index = end;
            continue;
        }

        match classify_target(target) {
            TargetClassification::Reference(kind, normalized) => {
                references.push(span(at, end, raw, &normalized, kind));
            }
            TargetClassification::Diagnostic(kind, reference_kind, message_text) => {
                diagnostics.push(diagnostic(at, end, raw, kind, message_text));
                references.push(span(at, end, raw, target, reference_kind));
            }
        }
        index = end;
    }

    ContextReferenceParse {
        original_message: message.to_owned(),
        references,
        diagnostics,
    }
}

enum TargetClassification<'a> {
    Reference(ContextReferenceKind, String),
    Diagnostic(ReferenceParseDiagnosticKind, ContextReferenceKind, &'a str),
}

fn classify_target(target: &str) -> TargetClassification<'_> {
    if target == "diff" {
        return TargetClassification::Reference(ContextReferenceKind::Diff, target.to_owned());
    }
    if target == "staged" {
        return TargetClassification::Reference(ContextReferenceKind::Staged, target.to_owned());
    }
    if let Some(rest) = target.strip_prefix("url:") {
        if rest.is_empty() {
            return TargetClassification::Diagnostic(
                ReferenceParseDiagnosticKind::MissingTarget,
                ContextReferenceKind::Unresolved,
                "url reference is missing a target",
            );
        }
        if is_supported_url(rest) {
            return TargetClassification::Reference(ContextReferenceKind::Url, rest.to_owned());
        }
        return TargetClassification::Diagnostic(
            ReferenceParseDiagnosticKind::MalformedTarget,
            ContextReferenceKind::Unsupported,
            "url reference target must start with http:// or https://",
        );
    }
    if is_supported_url(target) {
        return TargetClassification::Reference(ContextReferenceKind::Url, target.to_owned());
    }
    if let Some(rest) = target.strip_prefix("git:") {
        if rest.is_empty() {
            return TargetClassification::Diagnostic(
                ReferenceParseDiagnosticKind::MissingTarget,
                ContextReferenceKind::Unresolved,
                "git reference is missing a revision",
            );
        }
        if rest.starts_with(':') || rest.contains("::") {
            return TargetClassification::Diagnostic(
                ReferenceParseDiagnosticKind::MalformedTarget,
                ContextReferenceKind::Unsupported,
                "git reference target must be @git:<rev> or @git:<rev>:<path>",
            );
        }
        return TargetClassification::Reference(ContextReferenceKind::Git, rest.to_owned());
    }
    if target.contains(':') {
        return TargetClassification::Diagnostic(
            ReferenceParseDiagnosticKind::Unsupported,
            ContextReferenceKind::Unsupported,
            "reference scheme is unsupported",
        );
    }
    let kind = if target.ends_with('/') {
        ContextReferenceKind::Folder
    } else {
        ContextReferenceKind::File
    };
    TargetClassification::Reference(kind, target.to_owned())
}

fn fenced_code_ranges(message: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence_start = None;
    let mut line_start = 0;

    for line in message.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            if let Some(start) = fence_start.take() {
                ranges.push(start..line_start + line.len());
            } else {
                fence_start = Some(line_start);
            }
        }
        line_start += line.len();
    }

    if let Some(start) = fence_start {
        ranges.push(start..message.len());
    }

    ranges
}

fn containing_range(ranges: &[Range<usize>], index: usize) -> Option<&Range<usize>> {
    ranges
        .iter()
        .find(|range| range.start <= index && index < range.end)
}

fn is_escaped_at(message: &str, at: usize) -> bool {
    at > 0 && message[..at].ends_with('\\')
}

fn previous_is_word(message: &str, at: usize) -> bool {
    message[..at]
        .chars()
        .next_back()
        .map(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
        .unwrap_or(false)
}

fn token_end(message: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, character) in message[start..].char_indices() {
        if character.is_whitespace() {
            break;
        }
        end = start + offset + character.len_utf8();
    }
    trim_trailing_token_punctuation(message, end, start)
}

fn trim_trailing_token_punctuation(message: &str, mut end: usize, start: usize) -> usize {
    while end > start {
        let Some(character) = message[start..end].chars().next_back() else {
            break;
        };
        if !matches!(character, '.' | ',' | ';' | '!' | '?' | ')' | ']' | '}') {
            break;
        }
        end -= character.len_utf8();
    }
    end
}

fn is_handle_like(target: &str) -> bool {
    !matches!(target, "diff" | "staged")
        && !target.contains('/')
        && !target.contains('.')
        && !target.contains(':')
        && target.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn is_supported_url(target: &str) -> bool {
    target.starts_with("https://") || target.starts_with("http://")
}

fn span(
    start: usize,
    end: usize,
    raw_token: &str,
    normalized_target: &str,
    kind: ContextReferenceKind,
) -> ContextReferenceSpan {
    ContextReferenceSpan {
        start,
        end,
        raw_token: raw_token.to_owned(),
        normalized_target: normalized_target.to_owned(),
        kind,
    }
}

fn diagnostic(
    start: usize,
    end: usize,
    raw_token: &str,
    kind: ReferenceParseDiagnosticKind,
    message: &str,
) -> ReferenceParseDiagnostic {
    ReferenceParseDiagnostic {
        start,
        end,
        raw_token: raw_token.to_owned(),
        kind,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_reference_parse_escape_ignores_escaped_at() {
        let parsed = parse_context_references(r"literal \@src/main.rs and @src/lib.rs");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].raw_token, "@src/lib.rs");
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::File);
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ReferenceParseDiagnosticKind::Escaped));
    }

    #[test]
    fn context_reference_parse_fenced_code_block_ignores_references() {
        let parsed =
            parse_context_references("before\n```rust\n@src/main.rs\n```\nafter @src/lib.rs");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].raw_token, "@src/lib.rs");
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ReferenceParseDiagnosticKind::CodeBlockIgnored));
    }

    #[test]
    fn context_reference_parse_email_and_handle_are_ignored() {
        let parsed =
            parse_context_references("mail me@example.com or ask @alice about @src/lib.rs");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].raw_token, "@src/lib.rs");
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::File);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == ReferenceParseDiagnosticKind::Ambiguous)
                .count(),
            2
        );
    }

    #[test]
    fn context_reference_parse_adjacent_punctuation_keeps_span_tight() {
        let parsed = parse_context_references("review (@src/lib.rs), then @src/bin/.");

        assert_eq!(parsed.references.len(), 2);
        assert_eq!(parsed.references[0].raw_token, "@src/lib.rs");
        assert_eq!(parsed.references[1].raw_token, "@src/bin/");
        assert_eq!(parsed.references[1].kind, ContextReferenceKind::Folder);
    }

    #[test]
    fn context_reference_parse_bare_url() {
        let parsed = parse_context_references("read @https://example.com/docs before continuing");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::Url);
        assert_eq!(
            parsed.references[0].normalized_target,
            "https://example.com/docs"
        );
    }

    #[test]
    fn context_reference_parse_url_scheme() {
        let parsed = parse_context_references("read @url:https://example.com/docs");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::Url);
        assert_eq!(
            parsed.references[0].normalized_target,
            "https://example.com/docs"
        );
    }

    #[test]
    fn context_reference_parse_git_revision() {
        let parsed = parse_context_references("compare @git:HEAD");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::Git);
        assert_eq!(parsed.references[0].normalized_target, "HEAD");
    }

    #[test]
    fn context_reference_parse_git_revision_and_path() {
        let parsed = parse_context_references("compare @git:HEAD:src/lib.rs");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::Git);
        assert_eq!(parsed.references[0].normalized_target, "HEAD:src/lib.rs");
    }

    #[test]
    fn context_reference_parse_diff_and_staged() {
        let parsed = parse_context_references("review @diff and @staged");

        assert_eq!(parsed.references.len(), 2);
        assert_eq!(parsed.references[0].kind, ContextReferenceKind::Diff);
        assert_eq!(parsed.references[1].kind, ContextReferenceKind::Staged);
    }

    #[test]
    fn context_reference_parse_missing_and_malformed_target() {
        let parsed = parse_context_references("bad @ @git: @url:ftp://example.com @thing:value");

        assert_eq!(parsed.references.len(), 4);
        assert!(parsed
            .references
            .iter()
            .any(|reference| reference.kind == ContextReferenceKind::Unresolved));
        assert!(parsed
            .references
            .iter()
            .any(|reference| reference.kind == ContextReferenceKind::Unsupported));
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ReferenceParseDiagnosticKind::MissingTarget));
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ReferenceParseDiagnosticKind::MalformedTarget));
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ReferenceParseDiagnosticKind::Unsupported));
    }

    #[test]
    fn context_reference_parse_artifact_shell_serializes_boundary_fields() {
        let parsed = parse_context_references("read @src/lib.rs");
        let artifact = ResolvedContextArtifact::parsed_shell(&parsed.references[0]);
        let value = serde_json::to_value(&artifact);

        assert!(value.is_ok());
        let Ok(value) = value else {
            return;
        };
        assert_eq!(value["source"], "src/lib.rs");
        assert_eq!(value["display_name"], "src/lib.rs");
        assert_eq!(value["digest"], serde_json::Value::Null);
        assert_eq!(value["byte_count"], serde_json::Value::Null);
        assert_eq!(value["token_estimate"], serde_json::Value::Null);
        assert_eq!(value["redaction_status"], "not_applied");
        assert_eq!(value["truncation_status"], "not_applied");
        assert_eq!(value["permission_evidence"]["status"], "not_checked");
        assert_eq!(value["state"], "parsed");
    }
}
