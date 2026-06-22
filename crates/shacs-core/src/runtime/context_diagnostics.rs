use super::context_files::{ContextFileProjection, ContextFileReadStatus};
use super::context_handoff::{ContextBudgetDecision, ContextProviderHandoff};
use super::context_refs::{
    ContextRedactionStatus, ContextReferenceKind, ContextReferenceParse, ContextResolutionState,
    ContextTruncationStatus, ResolvedContextArtifact,
};
use super::context_safety::{ContextPermissionDecision, ContextSafetyReport, ContextTrustLabel};
use serde::{Deserialize, Serialize};
use shacs_redaction::redact_string;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct ContextDiagnosticsInput<'a> {
    pub reference_parse: Option<&'a ContextReferenceParse>,
    pub context_files: &'a [ContextFileProjection],
    pub resolved_artifacts: &'a [ResolvedContextArtifact],
    pub safety_report: Option<&'a ContextSafetyReport>,
    pub provider_handoff: Option<&'a ContextProviderHandoff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextDiagnosticsSummary {
    pub references: Option<ContextReferenceDiagnosticsSummary>,
    pub context_files: ContextFileDiagnosticsSummary,
    pub artifacts: ContextArtifactDiagnosticsSummary,
    pub safety: Option<ContextSafetyDiagnosticsSummary>,
    pub budget: Option<ContextBudgetDiagnosticsSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReferenceDiagnosticsSummary {
    pub reference_count: usize,
    pub diagnostic_count: usize,
    pub kind_counts: Vec<ContextDiagnosticsCount>,
    pub diagnostic_kind_counts: Vec<ContextDiagnosticsCount>,
    pub references: Vec<ContextReferenceDiagnosticEntry>,
    pub diagnostics: Vec<ContextReferenceParseDiagnosticEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReferenceDiagnosticEntry {
    pub kind: ContextReferenceKind,
    pub source_label: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReferenceParseDiagnosticEntry {
    pub kind: String,
    pub message: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileDiagnosticsSummary {
    pub total_count: usize,
    pub included_count: usize,
    pub skipped_count: usize,
    pub truncated_count: usize,
    pub denied_count: usize,
    pub status_counts: Vec<ContextDiagnosticsCount>,
    pub entries: Vec<ContextFileDiagnosticEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileDiagnosticEntry {
    pub order: usize,
    pub source_label: String,
    pub filename: String,
    pub source: String,
    pub source_directory_depth: Option<usize>,
    pub status: ContextFileReadStatus,
    pub reason: Option<String>,
    pub digest: Option<String>,
    pub byte_count: Option<usize>,
    pub token_estimate: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextArtifactDiagnosticsSummary {
    pub total_count: usize,
    pub resolved_count: usize,
    pub skipped_count: usize,
    pub denied_count: usize,
    pub failed_count: usize,
    pub truncated_count: usize,
    pub redacted_count: usize,
    pub state_counts: Vec<ContextDiagnosticsCount>,
    pub entries: Vec<ContextArtifactDiagnosticEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextArtifactDiagnosticEntry {
    pub kind: ContextReferenceKind,
    pub source_label: String,
    pub display_label: String,
    pub state: ContextResolutionState,
    pub digest: Option<String>,
    pub byte_count: Option<usize>,
    pub token_estimate: Option<usize>,
    pub redaction_status: ContextRedactionStatus,
    pub truncation_status: ContextTruncationStatus,
    pub permission_status: String,
    pub permission_evidence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSafetyDiagnosticsSummary {
    pub diagnostic_count: usize,
    pub replay_evidence_count: usize,
    pub permission_decision_counts: Vec<ContextDiagnosticsCount>,
    pub trust_label_counts: Vec<ContextDiagnosticsCount>,
    pub redaction_status_counts: Vec<ContextDiagnosticsCount>,
    pub diagnostics: Vec<ContextSafetyDiagnosticEntry>,
    pub replay_evidence: Vec<ContextReplayDiagnosticEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSafetyDiagnosticEntry {
    pub source_label: String,
    pub permission_decision: ContextPermissionDecision,
    pub redaction_status: ContextRedactionStatus,
    pub trust_label: ContextTrustLabel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReplayDiagnosticEntry {
    pub source_label: String,
    pub source_digest: Option<String>,
    pub resolution_metadata: String,
    pub no_live_refetch: bool,
    pub state: ContextResolutionState,
    pub trust_label: ContextTrustLabel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudgetDiagnosticsSummary {
    pub block_count: usize,
    pub evidence_count: usize,
    pub used_context_bytes: usize,
    pub budget_bytes: usize,
    pub included_count: usize,
    pub skipped_count: usize,
    pub truncated_count: usize,
    pub decision_counts: Vec<ContextDiagnosticsCount>,
    pub blocks: Vec<ContextProviderBlockDiagnosticEntry>,
    pub evidence: Vec<ContextBudgetDiagnosticEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextProviderBlockDiagnosticEntry {
    pub source_label: String,
    pub trust_label: String,
    pub truncation_label: Option<String>,
    pub digest: Option<String>,
    pub byte_count: usize,
    pub token_estimate: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudgetDiagnosticEntry {
    pub source_label: String,
    pub priority: String,
    pub decision: ContextBudgetDecision,
    pub reason: Option<String>,
    pub digest: Option<String>,
    pub estimated_tokens: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextDiagnosticsCount {
    pub label: String,
    pub count: usize,
}

pub fn build_context_diagnostics_summary(
    input: ContextDiagnosticsInput<'_>,
) -> ContextDiagnosticsSummary {
    ContextDiagnosticsSummary {
        references: input.reference_parse.map(summarize_references),
        context_files: summarize_context_files(input.context_files),
        artifacts: summarize_artifacts(input.resolved_artifacts),
        safety: input.safety_report.map(summarize_safety),
        budget: input.provider_handoff.map(summarize_budget),
    }
}

fn summarize_references(parse: &ContextReferenceParse) -> ContextReferenceDiagnosticsSummary {
    let mut kind_counts = BTreeMap::new();
    for reference in &parse.references {
        increment(&mut kind_counts, format!("{:?}", reference.kind));
    }
    let mut diagnostic_kind_counts = BTreeMap::new();
    for diagnostic in &parse.diagnostics {
        increment(
            &mut diagnostic_kind_counts,
            format!("{:?}", diagnostic.kind),
        );
    }

    ContextReferenceDiagnosticsSummary {
        reference_count: parse.references.len(),
        diagnostic_count: parse.diagnostics.len(),
        kind_counts: counts(kind_counts),
        diagnostic_kind_counts: counts(diagnostic_kind_counts),
        references: parse
            .references
            .iter()
            .map(|reference| ContextReferenceDiagnosticEntry {
                kind: reference.kind,
                source_label: safe_text(&reference.normalized_target),
                start: reference.start,
                end: reference.end,
            })
            .collect(),
        diagnostics: parse
            .diagnostics
            .iter()
            .map(|diagnostic| ContextReferenceParseDiagnosticEntry {
                kind: format!("{:?}", diagnostic.kind),
                message: safe_text(&diagnostic.message),
                start: diagnostic.start,
                end: diagnostic.end,
            })
            .collect(),
    }
}

fn summarize_context_files(files: &[ContextFileProjection]) -> ContextFileDiagnosticsSummary {
    let mut status_counts = BTreeMap::new();
    for file in files {
        increment(&mut status_counts, format!("{:?}", file.status));
    }

    ContextFileDiagnosticsSummary {
        total_count: files.len(),
        included_count: files
            .iter()
            .filter(|file| file.status == ContextFileReadStatus::Included)
            .count(),
        skipped_count: files
            .iter()
            .filter(|file| matches!(file.status, ContextFileReadStatus::SkippedMissing))
            .count(),
        truncated_count: files
            .iter()
            .filter(|file| file.status == ContextFileReadStatus::Truncated)
            .count(),
        denied_count: files
            .iter()
            .filter(|file| file.status == ContextFileReadStatus::DeniedBoundary)
            .count(),
        status_counts: counts(status_counts),
        entries: files
            .iter()
            .map(|file| ContextFileDiagnosticEntry {
                order: file.order,
                source_label: safe_text(&file.path.display().to_string()),
                filename: safe_text(&file.filename),
                source: format!("{:?}", file.source),
                source_directory_depth: (file.source_directory_depth != usize::MAX)
                    .then_some(file.source_directory_depth),
                status: file.status,
                reason: file.reason.as_deref().map(safe_text),
                digest: file.digest.as_ref().map(|digest| digest.sha256.clone()),
                byte_count: file.digest.as_ref().map(|digest| digest.byte_count),
                token_estimate: file.digest.as_ref().map(|digest| digest.token_estimate),
            })
            .collect(),
    }
}

fn summarize_artifacts(artifacts: &[ResolvedContextArtifact]) -> ContextArtifactDiagnosticsSummary {
    let mut state_counts = BTreeMap::new();
    for artifact in artifacts {
        increment(&mut state_counts, format!("{:?}", artifact.state));
    }

    ContextArtifactDiagnosticsSummary {
        total_count: artifacts.len(),
        resolved_count: artifacts
            .iter()
            .filter(|artifact| artifact.state == ContextResolutionState::Resolved)
            .count(),
        skipped_count: artifacts
            .iter()
            .filter(|artifact| artifact.state == ContextResolutionState::Skipped)
            .count(),
        denied_count: artifacts
            .iter()
            .filter(|artifact| artifact.state == ContextResolutionState::Denied)
            .count(),
        failed_count: artifacts
            .iter()
            .filter(|artifact| artifact.state == ContextResolutionState::Failed)
            .count(),
        truncated_count: artifacts
            .iter()
            .filter(|artifact| artifact.truncation_status == ContextTruncationStatus::Truncated)
            .count(),
        redacted_count: artifacts
            .iter()
            .filter(|artifact| artifact.redaction_status == ContextRedactionStatus::Redacted)
            .count(),
        state_counts: counts(state_counts),
        entries: artifacts
            .iter()
            .map(|artifact| ContextArtifactDiagnosticEntry {
                kind: artifact.kind,
                source_label: safe_text(&artifact.source),
                display_label: safe_text(&artifact.display_name),
                state: artifact.state,
                digest: artifact.digest.clone(),
                byte_count: artifact.byte_count,
                token_estimate: artifact.token_estimate,
                redaction_status: artifact.redaction_status,
                truncation_status: artifact.truncation_status,
                permission_status: format!("{:?}", artifact.permission_evidence.status),
                permission_evidence: artifact
                    .permission_evidence
                    .evidence
                    .as_deref()
                    .map(safe_text),
            })
            .collect(),
    }
}

fn summarize_safety(report: &ContextSafetyReport) -> ContextSafetyDiagnosticsSummary {
    let mut permission_decision_counts = BTreeMap::new();
    let mut trust_label_counts = BTreeMap::new();
    let mut redaction_status_counts = BTreeMap::new();
    for diagnostic in &report.diagnostics {
        increment(
            &mut permission_decision_counts,
            format!("{:?}", diagnostic.permission_decision),
        );
        increment(
            &mut trust_label_counts,
            format!("{:?}", diagnostic.trust_label),
        );
        increment(
            &mut redaction_status_counts,
            format!("{:?}", diagnostic.redaction_status),
        );
    }

    ContextSafetyDiagnosticsSummary {
        diagnostic_count: report.diagnostics.len(),
        replay_evidence_count: report.replay_evidence.len(),
        permission_decision_counts: counts(permission_decision_counts),
        trust_label_counts: counts(trust_label_counts),
        redaction_status_counts: counts(redaction_status_counts),
        diagnostics: report
            .diagnostics
            .iter()
            .map(|diagnostic| ContextSafetyDiagnosticEntry {
                source_label: safe_text(&diagnostic.source_label),
                permission_decision: diagnostic.permission_decision,
                redaction_status: diagnostic.redaction_status,
                trust_label: diagnostic.trust_label,
                message: safe_text(&diagnostic.message),
            })
            .collect(),
        replay_evidence: report
            .replay_evidence
            .iter()
            .map(|evidence| ContextReplayDiagnosticEntry {
                source_label: safe_text(&evidence.source_label),
                source_digest: evidence.source_digest.clone(),
                resolution_metadata: safe_text(&evidence.resolution_metadata),
                no_live_refetch: evidence.no_live_refetch,
                state: evidence.state,
                trust_label: evidence.trust_label,
            })
            .collect(),
    }
}

fn summarize_budget(handoff: &ContextProviderHandoff) -> ContextBudgetDiagnosticsSummary {
    let mut decision_counts = BTreeMap::new();
    for evidence in &handoff.evidence {
        increment(&mut decision_counts, format!("{:?}", evidence.decision));
    }

    ContextBudgetDiagnosticsSummary {
        block_count: handoff.blocks.len(),
        evidence_count: handoff.evidence.len(),
        used_context_bytes: handoff.used_context_bytes,
        budget_bytes: handoff.budget_bytes,
        included_count: handoff
            .evidence
            .iter()
            .filter(|evidence| evidence.decision == ContextBudgetDecision::Included)
            .count(),
        skipped_count: handoff
            .evidence
            .iter()
            .filter(|evidence| {
                matches!(
                    evidence.decision,
                    ContextBudgetDecision::SkippedBudget | ContextBudgetDecision::SkippedSafety
                )
            })
            .count(),
        truncated_count: handoff
            .evidence
            .iter()
            .filter(|evidence| evidence.decision == ContextBudgetDecision::Truncated)
            .count(),
        decision_counts: counts(decision_counts),
        blocks: handoff
            .blocks
            .iter()
            .map(|block| ContextProviderBlockDiagnosticEntry {
                source_label: safe_text(&block.source_label),
                trust_label: safe_text(&block.trust_label),
                truncation_label: block.truncation_label.as_deref().map(safe_text),
                digest: block.digest.clone(),
                byte_count: block.byte_count,
                token_estimate: block.token_estimate,
            })
            .collect(),
        evidence: handoff
            .evidence
            .iter()
            .map(|evidence| ContextBudgetDiagnosticEntry {
                source_label: safe_text(&evidence.source_label),
                priority: format!("{:?}", evidence.priority),
                decision: evidence.decision,
                reason: evidence.reason.as_deref().map(safe_text),
                digest: evidence.digest.clone(),
                estimated_tokens: evidence.estimated_tokens,
            })
            .collect(),
    }
}

fn increment(counts: &mut BTreeMap<String, usize>, label: String) {
    *counts.entry(label).or_insert(0) += 1;
}

fn counts(counts: BTreeMap<String, usize>) -> Vec<ContextDiagnosticsCount> {
    counts
        .into_iter()
        .map(|(label, count)| ContextDiagnosticsCount { label, count })
        .collect()
}

fn safe_text(value: &str) -> String {
    redact_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        build_context_provider_handoff, ContextBudgetInput, ContextFileDigest, ContextFileSource,
        ContextPermissionEvidence, ContextPermissionStatus,
    };
    use std::error::Error;
    use std::path::PathBuf;

    fn artifact(
        source: &str,
        content: &str,
        state: ContextResolutionState,
    ) -> ResolvedContextArtifact {
        ResolvedContextArtifact {
            kind: ContextReferenceKind::File,
            source: source.to_owned(),
            display_name: source.to_owned(),
            content: Some(content.to_owned()),
            digest: Some("artifact-digest".to_owned()),
            byte_count: Some(content.len()),
            token_estimate: Some(content.split_whitespace().count()),
            redaction_status: ContextRedactionStatus::NotApplied,
            truncation_status: ContextTruncationStatus::NotApplied,
            permission_evidence: ContextPermissionEvidence {
                status: if state == ContextResolutionState::Denied {
                    ContextPermissionStatus::Denied
                } else {
                    ContextPermissionStatus::Allowed
                },
                evidence: Some("context resolver read-only gate passed".to_owned()),
            },
            state,
        }
    }

    fn context_file(
        path: &str,
        status: ContextFileReadStatus,
        content: Option<&str>,
    ) -> ContextFileProjection {
        ContextFileProjection {
            order: 0,
            path: PathBuf::from(path),
            filename: path.to_owned(),
            source: ContextFileSource::DefaultCandidate,
            source_directory_depth: 0,
            status,
            reason: (status != ContextFileReadStatus::Included)
                .then(|| format!("status {status:?}")),
            digest: content.map(|content| ContextFileDigest {
                sha256: format!("digest-{path}"),
                byte_count: content.len(),
                token_estimate: content.split_whitespace().count(),
            }),
            content: content.map(str::to_owned),
        }
    }

    #[test]
    fn context_diagnostics_counts_statuses_and_budget_evidence() {
        let parse = ContextReferenceParse {
            original_message: "use @src/lib.rs and @missing.md".to_owned(),
            references: vec![
                super::super::context_refs::ContextReferenceSpan {
                    start: 4,
                    end: 15,
                    raw_token: "@src/lib.rs".to_owned(),
                    normalized_target: "src/lib.rs".to_owned(),
                    kind: ContextReferenceKind::File,
                },
                super::super::context_refs::ContextReferenceSpan {
                    start: 20,
                    end: 31,
                    raw_token: "@missing.md".to_owned(),
                    normalized_target: "missing.md".to_owned(),
                    kind: ContextReferenceKind::File,
                },
            ],
            diagnostics: Vec::new(),
        };
        let files = vec![
            context_file(
                "AGENTS.md",
                ContextFileReadStatus::Included,
                Some("workspace rules"),
            ),
            context_file(
                "large.md",
                ContextFileReadStatus::Truncated,
                Some("large context"),
            ),
            context_file("missing.md", ContextFileReadStatus::SkippedMissing, None),
            context_file("outside.md", ContextFileReadStatus::DeniedBoundary, None),
        ];
        let artifacts = vec![
            artifact(
                "src/lib.rs",
                "hello world",
                ContextResolutionState::Resolved,
            ),
            artifact(
                "missing.md",
                "file metadata could not be read",
                ContextResolutionState::Failed,
            ),
            artifact("secret.txt", "protected", ContextResolutionState::Denied),
            artifact(
                "https://example.com",
                "network disabled",
                ContextResolutionState::Skipped,
            ),
        ];
        let safety = crate::runtime::apply_context_safety_gate(&artifacts);
        let handoff = build_context_provider_handoff(
            &safety.artifacts,
            &files,
            ContextBudgetInput {
                max_context_bytes: Some(8),
                ..ContextBudgetInput::default()
            },
        );

        let summary = build_context_diagnostics_summary(ContextDiagnosticsInput {
            reference_parse: Some(&parse),
            context_files: &files,
            resolved_artifacts: &safety.artifacts,
            safety_report: Some(&safety),
            provider_handoff: Some(&handoff),
        });

        assert_eq!(
            summary
                .references
                .as_ref()
                .map(|value| value.reference_count),
            Some(2)
        );
        assert_eq!(summary.context_files.total_count, 4);
        assert_eq!(summary.context_files.included_count, 1);
        assert_eq!(summary.context_files.skipped_count, 1);
        assert_eq!(summary.context_files.truncated_count, 1);
        assert_eq!(summary.context_files.denied_count, 1);
        assert_eq!(summary.artifacts.resolved_count, 1);
        assert_eq!(summary.artifacts.skipped_count, 1);
        assert_eq!(summary.artifacts.denied_count, 1);
        assert_eq!(summary.artifacts.failed_count, 1);
        assert_eq!(
            summary.safety.as_ref().map(|value| value.diagnostic_count),
            Some(4)
        );
        assert_eq!(
            summary.budget.as_ref().map(|value| value.evidence_count),
            Some(handoff.evidence.len())
        );
        assert!(summary
            .budget
            .as_ref()
            .is_some_and(|value| value.truncated_count + value.skipped_count > 0));
    }

    #[test]
    fn context_diagnostics_omits_raw_context_provider_and_replay_content(
    ) -> Result<(), Box<dyn Error>> {
        let raw_secret = "VERY_SECRET_CONTEXT_BODY";
        let api_secret = "sk-secret-token";
        let mut artifact = artifact(
            &format!("config-OPENAI_API_KEY={api_secret}.txt"),
            &format!("{raw_secret} OPENAI_API_KEY={api_secret}"),
            ContextResolutionState::Resolved,
        );
        artifact.permission_evidence.evidence =
            Some(format!("allowed OPENAI_API_KEY={api_secret}"));
        let safety = crate::runtime::apply_context_safety_gate(&[artifact]);
        let file = ContextFileProjection {
            order: 0,
            path: PathBuf::from("AGENTS.md"),
            filename: "AGENTS.md".to_owned(),
            source: ContextFileSource::DefaultCandidate,
            source_directory_depth: 0,
            status: ContextFileReadStatus::Included,
            reason: None,
            digest: Some(ContextFileDigest {
                sha256: "file-digest".to_owned(),
                byte_count: raw_secret.len(),
                token_estimate: 1,
            }),
            content: Some(raw_secret.to_owned()),
        };
        let handoff = build_context_provider_handoff(
            &safety.artifacts,
            std::slice::from_ref(&file),
            ContextBudgetInput::default(),
        );

        let summary = build_context_diagnostics_summary(ContextDiagnosticsInput {
            reference_parse: None,
            context_files: &[file],
            resolved_artifacts: &safety.artifacts,
            safety_report: Some(&safety),
            provider_handoff: Some(&handoff),
        });
        let serialized = serde_json::to_string(&summary)?;

        assert!(!serialized.contains(raw_secret));
        assert!(!serialized.contains(api_secret));
        assert!(!serialized.contains("\"content\":"));
        assert!(!serialized.contains("redacted_excerpt"));
        assert!(serialized.contains("file-digest"));
        assert!(serialized.contains("byte_count"));
        Ok(())
    }
}
