use super::context_refs::{
    ContextPermissionEvidence, ContextPermissionStatus, ContextRedactionStatus,
    ContextReferenceKind, ContextResolutionState, ResolvedContextArtifact,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_redaction::redact_string;
use std::path::Path;

const REPLAY_EXCERPT_MAX_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSafetyReport {
    pub artifacts: Vec<ResolvedContextArtifact>,
    pub diagnostics: Vec<ContextSafetyDiagnostic>,
    pub replay_evidence: Vec<ContextReplayEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSafetyDiagnostic {
    pub source_label: String,
    pub permission_decision: ContextPermissionDecision,
    pub redaction_status: ContextRedactionStatus,
    pub trust_label: ContextTrustLabel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReplayEvidence {
    pub source_label: String,
    pub source_digest: Option<String>,
    pub redacted_excerpt: Option<String>,
    pub resolution_metadata: String,
    pub no_live_refetch: bool,
    pub state: ContextResolutionState,
    pub trust_label: ContextTrustLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPermissionDecision {
    Allowed,
    DeniedProtected,
    DeniedNetwork,
    DeniedOutsideWorkspace,
    RequiresApproval,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTrustLabel {
    WorkspaceUserAuthored,
    WorkspaceFile,
    GitReadonly,
    ExternalUntrusted,
}

pub fn apply_context_safety_gate(artifacts: &[ResolvedContextArtifact]) -> ContextSafetyReport {
    let mut gated = Vec::with_capacity(artifacts.len());
    let mut diagnostics = Vec::with_capacity(artifacts.len());
    let mut replay_evidence = Vec::with_capacity(artifacts.len());

    for artifact in artifacts {
        let mut artifact = artifact.clone();
        let decision = permission_decision_for_artifact(&artifact);
        let trust_label = trust_label_for_kind(artifact.kind);

        if artifact.state == ContextResolutionState::Resolved {
            apply_redaction(&mut artifact);
        }

        diagnostics.push(ContextSafetyDiagnostic {
            source_label: artifact.source.clone(),
            permission_decision: decision,
            redaction_status: artifact.redaction_status,
            trust_label,
            message: diagnostic_message(&artifact, decision),
        });
        replay_evidence.push(ContextReplayEvidence {
            source_label: artifact.source.clone(),
            source_digest: artifact.digest.clone(),
            redacted_excerpt: artifact.content.as_deref().map(redacted_excerpt),
            resolution_metadata: "recorded_context_artifact_no_live_refetch".to_owned(),
            no_live_refetch: true,
            state: artifact.state,
            trust_label,
        });
        gated.push(artifact);
    }

    ContextSafetyReport {
        artifacts: gated,
        diagnostics,
        replay_evidence,
    }
}

pub fn replay_context_artifact_from_evidence(
    evidence: &ContextReplayEvidence,
) -> ResolvedContextArtifact {
    ResolvedContextArtifact {
        kind: ContextReferenceKind::Unresolved,
        source: evidence.source_label.clone(),
        display_name: evidence.source_label.clone(),
        content: evidence.redacted_excerpt.clone(),
        digest: evidence.source_digest.clone(),
        byte_count: evidence
            .redacted_excerpt
            .as_ref()
            .map(|content| content.len()),
        token_estimate: evidence.redacted_excerpt.as_deref().map(estimate_tokens),
        redaction_status: if evidence.redacted_excerpt.is_some() {
            ContextRedactionStatus::Redacted
        } else {
            ContextRedactionStatus::NotApplied
        },
        truncation_status: super::context_refs::ContextTruncationStatus::NotApplied,
        permission_evidence: ContextPermissionEvidence {
            status: ContextPermissionStatus::NotChecked,
            evidence: Some("replay used recorded context evidence without live refetch".to_owned()),
        },
        state: evidence.state,
    }
}

pub fn protected_context_path_reason(path: &Path) -> Option<&'static str> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let lower = filename.to_ascii_lowercase();
    if lower == ".env" || lower.ends_with(".pem") || lower.ends_with(".key") {
        return Some("protected context target was denied before reading content");
    }
    if lower.contains("id_rsa") || lower.contains("id_ed25519") {
        return Some("protected context target was denied before reading content");
    }
    path.components()
        .any(|component| component.as_os_str() == ".ssh")
        .then_some("protected context target was denied before reading content")
}

pub fn trust_label_for_kind(kind: ContextReferenceKind) -> ContextTrustLabel {
    match kind {
        ContextReferenceKind::Url => ContextTrustLabel::ExternalUntrusted,
        ContextReferenceKind::Git | ContextReferenceKind::Diff | ContextReferenceKind::Staged => {
            ContextTrustLabel::GitReadonly
        }
        ContextReferenceKind::File | ContextReferenceKind::Folder => {
            ContextTrustLabel::WorkspaceFile
        }
        ContextReferenceKind::Unsupported | ContextReferenceKind::Unresolved => {
            ContextTrustLabel::WorkspaceUserAuthored
        }
    }
}

pub fn context_trust_label_name(label: ContextTrustLabel) -> &'static str {
    match label {
        ContextTrustLabel::WorkspaceUserAuthored => "workspace_user_authored",
        ContextTrustLabel::WorkspaceFile => "workspace_file",
        ContextTrustLabel::GitReadonly => "git_readonly",
        ContextTrustLabel::ExternalUntrusted => "external_untrusted",
    }
}

fn apply_redaction(artifact: &mut ResolvedContextArtifact) {
    let Some(content) = artifact.content.as_deref() else {
        return;
    };
    let redacted = redact_string(content);
    if redacted == content {
        return;
    }
    artifact.content = Some(redacted.clone());
    artifact.digest = Some(sha256_hex(redacted.as_bytes()));
    artifact.byte_count = Some(redacted.len());
    artifact.token_estimate = Some(estimate_tokens(&redacted));
    artifact.redaction_status = ContextRedactionStatus::Redacted;
}

fn permission_decision_for_artifact(
    artifact: &ResolvedContextArtifact,
) -> ContextPermissionDecision {
    if artifact.state == ContextResolutionState::Skipped
        && artifact.kind == ContextReferenceKind::Url
        && artifact
            .permission_evidence
            .evidence
            .as_deref()
            .is_some_and(|evidence| evidence.contains("network"))
    {
        return ContextPermissionDecision::DeniedNetwork;
    }
    if artifact.permission_evidence.status == ContextPermissionStatus::Denied {
        let evidence = artifact
            .permission_evidence
            .evidence
            .as_deref()
            .unwrap_or_default();
        if evidence.contains("protected") {
            return ContextPermissionDecision::DeniedProtected;
        }
        return ContextPermissionDecision::DeniedOutsideWorkspace;
    }
    match artifact.state {
        ContextResolutionState::Resolved => ContextPermissionDecision::Allowed,
        ContextResolutionState::Skipped => ContextPermissionDecision::Skipped,
        ContextResolutionState::Failed => ContextPermissionDecision::Failed,
        ContextResolutionState::Parsed => ContextPermissionDecision::RequiresApproval,
        ContextResolutionState::Denied => ContextPermissionDecision::DeniedOutsideWorkspace,
    }
}

fn diagnostic_message(
    artifact: &ResolvedContextArtifact,
    decision: ContextPermissionDecision,
) -> String {
    match decision {
        ContextPermissionDecision::Allowed
            if artifact.redaction_status == ContextRedactionStatus::Redacted =>
        {
            "included after redacting secret-like content".to_owned()
        }
        ContextPermissionDecision::Allowed => "included by context safety gate".to_owned(),
        ContextPermissionDecision::DeniedProtected => "skipped protected context target".to_owned(),
        ContextPermissionDecision::DeniedNetwork => {
            "skipped because network references are disabled".to_owned()
        }
        ContextPermissionDecision::DeniedOutsideWorkspace => {
            "skipped outside workspace boundary".to_owned()
        }
        ContextPermissionDecision::RequiresApproval => "not resolved by safety gate".to_owned(),
        ContextPermissionDecision::Skipped => artifact
            .permission_evidence
            .evidence
            .clone()
            .unwrap_or_else(|| "skipped by resolver".to_owned()),
        ContextPermissionDecision::Failed => artifact
            .permission_evidence
            .evidence
            .clone()
            .unwrap_or_else(|| "resolver failed".to_owned()),
    }
}

fn redacted_excerpt(content: &str) -> String {
    let redacted = redact_string(content);
    if redacted.len() <= REPLAY_EXCERPT_MAX_CHARS {
        return redacted;
    }
    truncate_to_char_boundary(&redacted, REPLAY_EXCERPT_MAX_CHARS)
}

fn truncate_to_char_boundary(content: &str, max_bytes: usize) -> String {
    let mut end = max_bytes.min(content.len());
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_owned()
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
        build_context_provider_handoff, ContextBudgetInput, ContextTruncationStatus,
    };

    fn resolved(
        kind: ContextReferenceKind,
        source: &str,
        content: &str,
    ) -> ResolvedContextArtifact {
        ResolvedContextArtifact {
            kind,
            source: source.to_owned(),
            display_name: source.to_owned(),
            content: Some(content.to_owned()),
            digest: Some("original-digest".to_owned()),
            byte_count: Some(content.len()),
            token_estimate: Some(estimate_tokens(content)),
            redaction_status: ContextRedactionStatus::NotApplied,
            truncation_status: ContextTruncationStatus::NotApplied,
            permission_evidence: ContextPermissionEvidence {
                status: ContextPermissionStatus::Allowed,
                evidence: Some("context resolver read-only gate passed".to_owned()),
            },
            state: ContextResolutionState::Resolved,
        }
    }

    #[test]
    fn context_safety_redacts_secret_before_provider_handoff() {
        let artifact = resolved(
            ContextReferenceKind::File,
            "config.txt",
            "OPENAI_API_KEY=sk-secret-token visible",
        );

        let report = apply_context_safety_gate(&[artifact]);
        let gated = &report.artifacts[0];

        assert_eq!(gated.redaction_status, ContextRedactionStatus::Redacted);
        assert!(!gated
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("sk-secret-token"));
        assert!(report.replay_evidence[0]
            .redacted_excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("[REDACTED]"));

        let handoff =
            build_context_provider_handoff(&report.artifacts, &[], ContextBudgetInput::default());
        assert!(!handoff.blocks[0].content.contains("sk-secret-token"));
        assert!(handoff.blocks[0].content.contains("[REDACTED]"));
    }

    #[test]
    fn context_safety_labels_external_url_as_untrusted() {
        let artifact = resolved(
            ContextReferenceKind::Url,
            "https://example.com",
            "web content",
        );

        let report = apply_context_safety_gate(&[artifact]);

        assert_eq!(
            report.diagnostics[0].trust_label,
            ContextTrustLabel::ExternalUntrusted
        );
        assert_eq!(
            report.replay_evidence[0].trust_label,
            ContextTrustLabel::ExternalUntrusted
        );
    }

    #[test]
    fn context_safety_replay_uses_recorded_evidence_without_live_refetch() {
        let artifact = resolved(
            ContextReferenceKind::Url,
            "https://example.com",
            "token=secret-value",
        );
        let report = apply_context_safety_gate(&[artifact]);

        let replayed = replay_context_artifact_from_evidence(&report.replay_evidence[0]);

        assert!(report.replay_evidence[0].no_live_refetch);
        assert_eq!(replayed.source, "https://example.com");
        assert!(!replayed
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("secret-value"));
    }

    #[test]
    fn context_safety_recognizes_protected_context_paths() {
        assert!(protected_context_path_reason(Path::new("/workspace/.env")).is_some());
        assert!(protected_context_path_reason(Path::new("/workspace/.ssh/id_rsa")).is_some());
        assert!(protected_context_path_reason(Path::new("/workspace/src/lib.rs")).is_none());
    }
}
