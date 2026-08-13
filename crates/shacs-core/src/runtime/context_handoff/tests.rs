use super::*;
use crate::runtime::{
    ContextFileDigest, ContextFileProjection, ContextFileReadStatus, ContextFileSource,
    ContextPermissionEvidence, ContextPermissionStatus, ContextRedactionStatus,
    ContextReferenceKind, ContextResolutionState, ContextTruncationStatus, ResolvedContextArtifact,
};
use std::path::PathBuf;

fn artifact(source: &str, content: &str, kind: ContextReferenceKind) -> ResolvedContextArtifact {
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
fn explicit_reference_precedes_context_files() {
    let inline = vec![artifact(
        "src/lib.rs",
        "inline content",
        ContextReferenceKind::File,
    )];
    let files = vec![context_file("AGENTS.md", 0, "root context")];
    let handoff = build_context_provider_handoff(&inline, &files, ContextBudgetInput::default());
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
fn context_files_are_nearest_first_after_inline() {
    let files = vec![
        context_file("AGENTS.md", 0, "root context"),
        context_file("nested/AGENTS.md", 2, "nested context"),
    ];
    let handoff = build_context_provider_handoff(&[], &files, ContextBudgetInput::default());
    assert!(handoff.blocks[0].source_label.contains("nested/AGENTS.md"));
    assert!(handoff.blocks[1].source_label.contains("AGENTS.md"));
}

#[test]
fn overflow_truncates_and_records_evidence() {
    let inline = vec![artifact(
        "src/lib.rs",
        &"a".repeat(200),
        ContextReferenceKind::File,
    )];
    let full = build_context_provider_handoff(&inline, &[], ContextBudgetInput::default());
    let budget = full.blocks[0].included_tokens.saturating_sub(10);
    let handoff = build_context_provider_handoff(
        &inline,
        &[],
        ContextBudgetInput {
            max_context_tokens: Some(budget),
            ..ContextBudgetInput::default()
        },
    );
    assert_eq!(handoff.blocks.len(), 1);
    assert_eq!(
        handoff.blocks[0].byte_count,
        handoff.blocks[0].content.len()
    );
    assert_eq!(
        handoff.used_context_tokens,
        handoff.blocks[0].included_tokens
    );
    assert!(handoff.blocks[0].included_tokens <= budget);
    assert_eq!(
        handoff.blocks[0].truncation_label.as_deref(),
        Some("truncated_by_context_budget")
    );
    assert_eq!(
        handoff.evidence[0].decision,
        ContextBudgetDecision::Truncated
    );
}

#[test]
fn context_file_content_is_redacted_before_provider_block() {
    let files = vec![context_file(
        "AGENTS.md",
        0,
        "OPENAI_API_KEY=sk-context-file-secret visible",
    )];
    let handoff = build_context_provider_handoff(&[], &files, ContextBudgetInput::default());
    assert_eq!(handoff.blocks.len(), 1);
    assert!(!handoff.blocks[0].content.contains("sk-context-file-secret"));
    assert_eq!(
        handoff.blocks[0].byte_count,
        handoff.blocks[0].content.len()
    );
    assert_eq!(
        handoff.used_context_tokens,
        handoff.blocks[0].included_tokens
    );
}

#[test]
fn denied_explicit_artifact_is_not_included() {
    let mut denied = artifact("secret.txt", "secret", ContextReferenceKind::File);
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
fn provider_block_contains_source_trust_and_truncation_labels() {
    let inline = vec![artifact(
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

#[test]
fn estimator_selection_records_provider_model_and_uncertainty() {
    let anthropic = select_token_estimator("anthropic", "claude-4");
    let fallback = select_token_estimator("custom", "private-model");
    assert_eq!(anthropic.name, "estimator:anthropic_chars_v1");
    assert_eq!(anthropic.model, "claude-4");
    assert_eq!(anthropic.uncertainty_percent, 20);
    assert_eq!(fallback.name, "estimator:generic_chars_v1");
    assert_eq!(fallback.provider, "custom");
    assert_eq!(fallback.uncertainty_percent, 50);
}

#[test]
fn required_messages_overflow_is_evidenced_without_context_inclusion() {
    let inline = vec![artifact(
        "note.md",
        "optional context",
        ContextReferenceKind::File,
    )];
    let handoff = build_context_provider_handoff(
        &inline,
        &[],
        ContextBudgetInput {
            active_user_message: "active user message".repeat(4),
            required_runtime_instructions: "required runtime instructions".repeat(4),
            max_context_tokens: Some(2),
            estimator: select_token_estimator("custom", "private-model"),
        },
    );
    assert!(handoff.required_overflow_tokens > 0);
    assert_eq!(handoff.required.len(), 2);
    assert!(handoff.blocks.is_empty());
    assert_eq!(
        handoff.evidence[0].decision,
        ContextBudgetDecision::SkippedBudget
    );
}
