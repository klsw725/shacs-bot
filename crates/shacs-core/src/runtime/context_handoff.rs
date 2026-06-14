use super::context_files::{ContextFileProjection, ContextFileReadStatus};
use super::context_refs::{ContextResolutionState, ResolvedContextArtifact};
use super::context_safety::{context_trust_label_name, trust_label_for_kind, ContextTrustLabel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_redaction::redact_string;

pub const DEFAULT_CONTEXT_HANDOFF_BUDGET_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBudgetInput {
    pub reserved_user_message_bytes: usize,
    pub reserved_runtime_instruction_bytes: usize,
    pub max_context_bytes: Option<usize>,
}

impl Default for ContextBudgetInput {
    fn default() -> Self {
        Self {
            reserved_user_message_bytes: 0,
            reserved_runtime_instruction_bytes: 0,
            max_context_bytes: Some(DEFAULT_CONTEXT_HANDOFF_BUDGET_BYTES),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextProviderHandoff {
    pub blocks: Vec<ProviderContextBlock>,
    pub evidence: Vec<ContextBudgetEvidence>,
    pub used_context_bytes: usize,
    pub budget_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContextBlock {
    pub source_label: String,
    pub trust_label: String,
    pub truncation_label: Option<String>,
    pub content: String,
    pub digest: Option<String>,
    pub byte_count: usize,
    pub token_estimate: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudgetEvidence {
    pub source_label: String,
    pub priority: ContextArtifactPriority,
    pub decision: ContextBudgetDecision,
    pub reason: Option<String>,
    pub digest: Option<String>,
    pub estimated_tokens: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextArtifactPriority {
    ExplicitInline,
    NearestContextFile,
    AncestorContextFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudgetDecision {
    Included,
    Truncated,
    SkippedBudget,
    SkippedSafety,
}

pub fn build_context_provider_handoff(
    inline_artifacts: &[ResolvedContextArtifact],
    context_files: &[ContextFileProjection],
    budget: ContextBudgetInput,
) -> ContextProviderHandoff {
    let budget_bytes = budget
        .max_context_bytes
        .unwrap_or(DEFAULT_CONTEXT_HANDOFF_BUDGET_BYTES);
    let mut remaining = budget_bytes
        .saturating_sub(budget.reserved_user_message_bytes)
        .saturating_sub(budget.reserved_runtime_instruction_bytes);
    let mut blocks = Vec::new();
    let mut evidence = Vec::new();

    for artifact in inline_artifacts {
        consume_artifact_candidate(
            ArtifactCandidate::from_inline_artifact(artifact),
            &mut remaining,
            &mut blocks,
            &mut evidence,
        );
    }

    let mut file_candidates = context_files
        .iter()
        .map(ArtifactCandidate::from_context_file)
        .collect::<Vec<_>>();
    file_candidates.sort_by(|left, right| {
        right
            .source_depth
            .cmp(&left.source_depth)
            .then_with(|| left.source_label.cmp(&right.source_label))
    });
    for candidate in file_candidates {
        consume_artifact_candidate(candidate, &mut remaining, &mut blocks, &mut evidence);
    }

    ContextProviderHandoff {
        used_context_bytes: budget_bytes
            .saturating_sub(budget.reserved_user_message_bytes)
            .saturating_sub(budget.reserved_runtime_instruction_bytes)
            .saturating_sub(remaining),
        budget_bytes,
        blocks,
        evidence,
    }
}

struct ArtifactCandidate {
    source_label: String,
    trust_label: String,
    priority: ContextArtifactPriority,
    source_depth: usize,
    content: Option<String>,
    digest: Option<String>,
    token_estimate: Option<usize>,
    safety_allowed: bool,
    skip_reason: Option<String>,
}

impl ArtifactCandidate {
    fn from_inline_artifact(artifact: &ResolvedContextArtifact) -> Self {
        let safety_allowed = artifact.state == ContextResolutionState::Resolved;
        Self {
            source_label: format!("inline:{}", artifact.source),
            trust_label: context_trust_label_name(trust_label_for_kind(artifact.kind)).to_owned(),
            priority: ContextArtifactPriority::ExplicitInline,
            source_depth: usize::MAX,
            content: artifact.content.clone(),
            digest: artifact.digest.clone(),
            token_estimate: artifact.token_estimate,
            safety_allowed,
            skip_reason: (!safety_allowed)
                .then(|| format!("resolver state is {:?}", artifact.state)),
        }
    }

    fn from_context_file(file: &ContextFileProjection) -> Self {
        let includable = matches!(
            file.status,
            ContextFileReadStatus::Included | ContextFileReadStatus::Truncated
        );
        let content = file.content.as_deref().map(redact_string);
        let digest = content
            .as_deref()
            .map(|content| sha256_hex(content.as_bytes()))
            .or_else(|| file.digest.as_ref().map(|digest| digest.sha256.clone()));
        let token_estimate = content
            .as_deref()
            .map(estimate_tokens)
            .or_else(|| file.digest.as_ref().map(|digest| digest.token_estimate));
        Self {
            source_label: format!("context-file:{}", file.path.display()),
            trust_label: context_trust_label_name(ContextTrustLabel::WorkspaceUserAuthored)
                .to_owned(),
            priority: if file.source_directory_depth == usize::MAX
                || file.source_directory_depth > 0
            {
                ContextArtifactPriority::NearestContextFile
            } else {
                ContextArtifactPriority::AncestorContextFile
            },
            source_depth: file.source_directory_depth,
            content,
            digest,
            token_estimate,
            safety_allowed: includable,
            skip_reason: (!includable).then(|| format!("context file status is {:?}", file.status)),
        }
    }
}

fn consume_artifact_candidate(
    candidate: ArtifactCandidate,
    remaining: &mut usize,
    blocks: &mut Vec<ProviderContextBlock>,
    evidence: &mut Vec<ContextBudgetEvidence>,
) {
    if !candidate.safety_allowed {
        evidence.push(evidence_entry(
            &candidate,
            ContextBudgetDecision::SkippedSafety,
            candidate.skip_reason.clone(),
        ));
        return;
    }
    let Some(content) = candidate
        .content
        .as_deref()
        .filter(|content| !content.is_empty())
    else {
        evidence.push(evidence_entry(
            &candidate,
            ContextBudgetDecision::SkippedBudget,
            Some("artifact has no content".to_owned()),
        ));
        return;
    };
    if *remaining == 0 {
        evidence.push(evidence_entry(
            &candidate,
            ContextBudgetDecision::SkippedBudget,
            Some("context budget exhausted".to_owned()),
        ));
        return;
    }

    let full_block =
        format_provider_context_block(&candidate, content, ContextBudgetDecision::Included);
    let (decision, truncation_label, provider_content) = if full_block.len() <= *remaining {
        (ContextBudgetDecision::Included, None, full_block)
    } else {
        let Some((_, formatted)) = truncate_for_provider_budget(&candidate, content, *remaining)
        else {
            evidence.push(evidence_entry(
                &candidate,
                ContextBudgetDecision::SkippedBudget,
                Some("context budget cannot fit provider context block metadata".to_owned()),
            ));
            return;
        };
        (
            ContextBudgetDecision::Truncated,
            Some("truncated_by_context_budget".to_owned()),
            formatted,
        )
    };

    let byte_count = provider_content.len();
    *remaining = remaining.saturating_sub(byte_count);
    blocks.push(ProviderContextBlock {
        source_label: candidate.source_label.clone(),
        trust_label: candidate.trust_label.clone(),
        truncation_label,
        content: provider_content,
        digest: candidate.digest.clone(),
        byte_count,
        token_estimate: candidate.token_estimate,
    });
    evidence.push(evidence_entry(&candidate, decision, None));
}

fn evidence_entry(
    candidate: &ArtifactCandidate,
    decision: ContextBudgetDecision,
    reason: Option<String>,
) -> ContextBudgetEvidence {
    ContextBudgetEvidence {
        source_label: candidate.source_label.clone(),
        priority: candidate.priority,
        decision,
        reason,
        digest: candidate.digest.clone(),
        estimated_tokens: candidate.token_estimate,
    }
}

fn format_provider_context_block(
    candidate: &ArtifactCandidate,
    content: &str,
    decision: ContextBudgetDecision,
) -> String {
    let truncation = if decision == ContextBudgetDecision::Truncated {
        "\nTruncation: truncated_by_context_budget"
    } else {
        ""
    };
    format!(
        "[Context Artifact]\nSource: {}\nTrust: {}{}\n\n{}\n[/Context Artifact]",
        candidate.source_label, candidate.trust_label, truncation, content
    )
}

fn truncate_to_byte_boundary(content: &str, max_bytes: usize) -> String {
    if max_bytes == 0 {
        return String::new();
    }
    let mut end = max_bytes.min(content.len());
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_owned()
}

fn truncate_for_provider_budget(
    candidate: &ArtifactCandidate,
    content: &str,
    max_bytes: usize,
) -> Option<(String, String)> {
    let empty = format_provider_context_block(candidate, "", ContextBudgetDecision::Truncated);
    if empty.len() > max_bytes {
        return None;
    }
    let mut low = 0usize;
    let mut high = content.len();
    let mut best = String::new();
    let mut best_formatted = empty;
    while low <= high {
        let mid = (low + high) / 2;
        let truncated = truncate_to_byte_boundary(content, mid);
        let formatted =
            format_provider_context_block(candidate, &truncated, ContextBudgetDecision::Truncated);
        if formatted.len() <= max_bytes {
            best = truncated;
            best_formatted = formatted;
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    Some((best, best_formatted))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn estimate_tokens(content: &str) -> usize {
    content.split_whitespace().count().max(content.len() / 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        ContextFileDigest, ContextFileReadStatus, ContextFileSource, ContextPermissionEvidence,
        ContextPermissionStatus, ContextRedactionStatus, ContextReferenceKind,
        ContextResolutionState, ContextTruncationStatus,
    };
    use std::path::PathBuf;

    fn resolved_artifact(
        source: &str,
        content: &str,
        kind: ContextReferenceKind,
    ) -> ResolvedContextArtifact {
        ResolvedContextArtifact {
            kind,
            source: source.to_owned(),
            display_name: source.to_owned(),
            content: Some(content.to_owned()),
            digest: Some(format!("digest-{source}")),
            byte_count: Some(content.len()),
            token_estimate: Some(content.split_whitespace().count()),
            redaction_status: ContextRedactionStatus::NotApplied,
            truncation_status: ContextTruncationStatus::NotApplied,
            permission_evidence: ContextPermissionEvidence {
                status: ContextPermissionStatus::Allowed,
                evidence: None,
            },
            state: ContextResolutionState::Resolved,
        }
    }

    fn context_file(path: &str, depth: usize, content: &str) -> ContextFileProjection {
        ContextFileProjection {
            order: depth,
            path: PathBuf::from(path),
            filename: path.to_owned(),
            source: ContextFileSource::DefaultCandidate,
            source_directory_depth: depth,
            status: ContextFileReadStatus::Included,
            reason: None,
            digest: Some(ContextFileDigest {
                sha256: format!("digest-{path}"),
                byte_count: content.len(),
                token_estimate: content.split_whitespace().count(),
            }),
            content: Some(content.to_owned()),
        }
    }

    #[test]
    fn context_budget_explicit_reference_precedes_context_files() {
        let inline = vec![resolved_artifact(
            "src/lib.rs",
            "inline content",
            ContextReferenceKind::File,
        )];
        let files = vec![context_file("AGENTS.md", 0, "root context")];

        let handoff =
            build_context_provider_handoff(&inline, &files, ContextBudgetInput::default());

        assert_eq!(handoff.blocks.len(), 2);
        assert!(handoff.blocks[0].source_label.contains("inline:src/lib.rs"));
        assert_eq!(
            handoff.evidence[0].priority,
            ContextArtifactPriority::ExplicitInline
        );
        assert_eq!(
            handoff.evidence[0].decision,
            ContextBudgetDecision::Included
        );
    }

    #[test]
    fn context_budget_context_files_are_nearest_first_after_inline() {
        let files = vec![
            context_file("AGENTS.md", 0, "root context"),
            context_file("nested/AGENTS.md", 2, "nested context"),
        ];

        let handoff = build_context_provider_handoff(&[], &files, ContextBudgetInput::default());

        assert!(handoff.blocks[0].source_label.contains("nested/AGENTS.md"));
        assert!(handoff.blocks[1].source_label.contains("AGENTS.md"));
    }

    #[test]
    fn context_budget_overflow_truncates_and_records_evidence() {
        let content = "a".repeat(200);
        let inline = vec![resolved_artifact(
            "src/lib.rs",
            &content,
            ContextReferenceKind::File,
        )];
        let full = build_context_provider_handoff(&inline, &[], ContextBudgetInput::default());
        let budget = full.blocks[0].content.len().saturating_sub(100);

        let handoff = build_context_provider_handoff(
            &inline,
            &[],
            ContextBudgetInput {
                max_context_bytes: Some(budget),
                ..ContextBudgetInput::default()
            },
        );

        assert_eq!(handoff.blocks.len(), 1);
        assert_eq!(
            handoff.blocks[0].byte_count,
            handoff.blocks[0].content.len()
        );
        assert_eq!(handoff.used_context_bytes, handoff.blocks[0].content.len());
        assert!(handoff.blocks[0].content.len() <= budget);
        assert_eq!(
            handoff.blocks[0].truncation_label.as_deref(),
            Some("truncated_by_context_budget")
        );
        assert_eq!(
            handoff.evidence[0].decision,
            ContextBudgetDecision::Truncated
        );
        assert!(handoff.blocks[0].content.contains("Truncation:"));
    }

    #[test]
    fn context_budget_redacts_context_file_content_before_provider_block() {
        let files = vec![context_file(
            "AGENTS.md",
            0,
            "OPENAI_API_KEY=sk-context-file-secret visible",
        )];

        let handoff = build_context_provider_handoff(&[], &files, ContextBudgetInput::default());

        assert_eq!(handoff.blocks.len(), 1);
        assert!(!handoff.blocks[0].content.contains("sk-context-file-secret"));
        assert!(
            handoff.blocks[0].content.contains("[REDACTED]")
                || !handoff.blocks[0].content.contains("OPENAI_API_KEY")
        );
        assert_eq!(
            handoff.blocks[0].byte_count,
            handoff.blocks[0].content.len()
        );
        assert_eq!(handoff.used_context_bytes, handoff.blocks[0].content.len());
    }

    #[test]
    fn context_budget_denied_explicit_artifact_is_not_included() {
        let mut denied = resolved_artifact("secret.txt", "secret", ContextReferenceKind::File);
        denied.state = ContextResolutionState::Denied;
        denied.permission_evidence.status = ContextPermissionStatus::Denied;

        let handoff = build_context_provider_handoff(&[denied], &[], ContextBudgetInput::default());

        assert!(handoff.blocks.is_empty());
        assert_eq!(
            handoff.evidence[0].decision,
            ContextBudgetDecision::SkippedSafety
        );
    }

    #[test]
    fn context_budget_provider_block_contains_source_trust_and_truncation_labels() {
        let inline = vec![resolved_artifact(
            "https://example.com",
            "web",
            ContextReferenceKind::Url,
        )];

        let handoff = build_context_provider_handoff(&inline, &[], ContextBudgetInput::default());

        let block = &handoff.blocks[0];
        assert_eq!(block.trust_label, "external_untrusted");
        assert!(block.content.contains("Source: inline:https://example.com"));
        assert!(block.content.contains("Trust: external_untrusted"));
    }
}
