use super::candidate::ArtifactCandidate;
use super::estimator::TokenEstimatorSelection;
use super::types::{
    ContextBudgetDecision, ContextBudgetEvidence, ContextBudgetInput, ContextProviderHandoff,
    ProviderContextBlock, RequiredBudgetEvidence, RequiredContextKind,
    DEFAULT_CONTEXT_HANDOFF_BUDGET_TOKENS,
};
use crate::runtime::context_files::ContextFileProjection;
use crate::runtime::context_refs::ResolvedContextArtifact;

pub fn build_context_provider_handoff(
    inline_artifacts: &[ResolvedContextArtifact],
    context_files: &[ContextFileProjection],
    budget: ContextBudgetInput,
) -> ContextProviderHandoff {
    let budget_tokens = budget
        .max_context_tokens
        .unwrap_or(DEFAULT_CONTEXT_HANDOFF_BUDGET_TOKENS);
    let user_tokens = budget.estimator.estimate(&budget.active_user_message);
    let instruction_tokens = budget
        .estimator
        .estimate(&budget.required_runtime_instructions);
    let required_tokens = user_tokens.saturating_add(instruction_tokens);
    let required_overflow_tokens = required_tokens.saturating_sub(budget_tokens);
    let mut remaining = budget_tokens.saturating_sub(required_tokens);
    let mut blocks = Vec::new();
    let mut evidence = Vec::new();
    for artifact in inline_artifacts {
        consume_candidate(
            ArtifactCandidate::from_inline_artifact(artifact),
            &mut remaining,
            &mut blocks,
            &mut evidence,
            &budget.estimator,
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
        consume_candidate(
            candidate,
            &mut remaining,
            &mut blocks,
            &mut evidence,
            &budget.estimator,
        );
    }
    ContextProviderHandoff {
        used_context_tokens: budget_tokens
            .saturating_sub(required_tokens)
            .saturating_sub(remaining),
        budget_tokens,
        estimator: budget.estimator,
        required: required_evidence(
            user_tokens,
            instruction_tokens,
            budget_tokens,
            required_overflow_tokens,
        ),
        required_overflow_tokens,
        blocks,
        evidence,
    }
}

fn required_evidence(
    user_tokens: usize,
    instruction_tokens: usize,
    budget_tokens: usize,
    required_overflow_tokens: usize,
) -> Vec<RequiredBudgetEvidence> {
    let user_overflow = user_tokens.saturating_sub(budget_tokens);
    vec![
        RequiredBudgetEvidence {
            kind: RequiredContextKind::ActiveUserMessage,
            estimated_tokens: user_tokens,
            overflow_tokens: user_overflow,
        },
        RequiredBudgetEvidence {
            kind: RequiredContextKind::RuntimeInstructions,
            estimated_tokens: instruction_tokens,
            overflow_tokens: required_overflow_tokens.saturating_sub(user_overflow),
        },
    ]
}

fn consume_candidate(
    candidate: ArtifactCandidate,
    remaining: &mut usize,
    blocks: &mut Vec<ProviderContextBlock>,
    evidence: &mut Vec<ContextBudgetEvidence>,
    estimator: &TokenEstimatorSelection,
) {
    if candidate
        .skip_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("SkippedDuplicate"))
    {
        evidence.push(evidence_entry(
            &candidate,
            ContextBudgetDecision::SkippedDuplicate,
            candidate.skip_reason.clone(),
            0,
        ));
        return;
    }
    if !candidate.safety_allowed {
        evidence.push(evidence_entry(
            &candidate,
            ContextBudgetDecision::SkippedSafety,
            candidate.skip_reason.clone(),
            0,
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
            0,
        ));
        return;
    };
    if *remaining == 0 {
        evidence.push(evidence_entry(
            &candidate,
            ContextBudgetDecision::SkippedBudget,
            Some("context budget exhausted".to_owned()),
            0,
        ));
        return;
    }
    let full = format_block(&candidate, content, ContextBudgetDecision::Included);
    let (decision, truncation_label, provider_content) = if estimator.estimate(&full) <= *remaining
    {
        (ContextBudgetDecision::Included, None, full)
    } else {
        let Some(formatted) = truncate_for_budget(&candidate, content, *remaining, estimator)
        else {
            evidence.push(evidence_entry(
                &candidate,
                ContextBudgetDecision::SkippedBudget,
                Some("context budget cannot fit provider context block metadata".to_owned()),
                0,
            ));
            return;
        };
        (
            ContextBudgetDecision::Truncated,
            Some("truncated_by_context_budget".to_owned()),
            formatted,
        )
    };
    let included_tokens = estimator.estimate(&provider_content);
    *remaining = remaining.saturating_sub(included_tokens);
    blocks.push(ProviderContextBlock {
        source_label: candidate.source_label.clone(),
        trust_label: candidate.trust_label.clone(),
        truncation_label,
        byte_count: provider_content.len(),
        content: provider_content,
        digest: candidate.digest.clone(),
        token_estimate: candidate.token_estimate,
        included_tokens,
    });
    evidence.push(evidence_entry(&candidate, decision, None, included_tokens));
}

fn evidence_entry(
    candidate: &ArtifactCandidate,
    decision: ContextBudgetDecision,
    reason: Option<String>,
    included_tokens: usize,
) -> ContextBudgetEvidence {
    ContextBudgetEvidence {
        source_label: candidate.source_label.clone(),
        priority: candidate.priority,
        decision,
        reason,
        digest: candidate.digest.clone(),
        estimated_tokens: candidate.token_estimate,
        included_tokens,
    }
}

fn format_block(
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

fn truncate_for_budget(
    candidate: &ArtifactCandidate,
    content: &str,
    max_tokens: usize,
    estimator: &TokenEstimatorSelection,
) -> Option<String> {
    let empty = format_block(candidate, "", ContextBudgetDecision::Truncated);
    if estimator.estimate(&empty) > max_tokens {
        return None;
    }
    let mut low = 0usize;
    let mut high = content.len();
    let mut best = empty;
    while low <= high {
        let mid = (low + high) / 2;
        let end = (0..=mid.min(content.len()))
            .rev()
            .find(|end| content.is_char_boundary(*end))
            .unwrap_or_default();
        let formatted = format_block(candidate, &content[..end], ContextBudgetDecision::Truncated);
        if estimator.estimate(&formatted) <= max_tokens {
            best = formatted;
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    Some(best)
}
