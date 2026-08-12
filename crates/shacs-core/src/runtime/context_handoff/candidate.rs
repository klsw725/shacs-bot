use super::types::ContextArtifactPriority;
use crate::runtime::context_files::{
    ContextFileProjection, ContextFileReadStatus, ContextFileSource,
};
use crate::runtime::context_refs::{ContextResolutionState, ResolvedContextArtifact};
use crate::runtime::context_safety::{
    context_trust_label_name, trust_label_for_kind, ContextTrustLabel,
};
use sha2::{Digest, Sha256};
use shacs_redaction::redact_string;

pub(super) struct ArtifactCandidate {
    pub(super) source_label: String,
    pub(super) trust_label: String,
    pub(super) priority: ContextArtifactPriority,
    pub(super) source_depth: usize,
    pub(super) content: Option<String>,
    pub(super) digest: Option<String>,
    pub(super) token_estimate: Option<usize>,
    pub(super) safety_allowed: bool,
    pub(super) skip_reason: Option<String>,
}

impl ArtifactCandidate {
    pub(super) fn from_inline_artifact(artifact: &ResolvedContextArtifact) -> Self {
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

    pub(super) fn from_context_file(file: &ContextFileProjection) -> Self {
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
            priority: priority_for_file(file),
            source_depth: file.source_directory_depth,
            content,
            digest,
            token_estimate,
            safety_allowed: includable,
            skip_reason: (!includable).then(|| format!("context file status is {:?}", file.status)),
        }
    }
}

fn priority_for_file(file: &ContextFileProjection) -> ContextArtifactPriority {
    match file.source {
        ContextFileSource::ConfiguredExtra => ContextArtifactPriority::ConfiguredExtra,
        ContextFileSource::DefaultCandidate if file.source_directory_depth > 0 => {
            ContextArtifactPriority::NearestContextFile
        }
        ContextFileSource::DefaultCandidate => ContextArtifactPriority::AncestorContextFile,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn estimate_tokens(content: &str) -> usize {
    content.split_whitespace().count().max(content.len() / 4)
}
