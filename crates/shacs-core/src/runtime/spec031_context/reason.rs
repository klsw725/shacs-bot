use super::super::context_files::ContextFileReadStatus;
use super::super::context_refs::{
    ContextReferenceKind, ContextResolutionState, ResolvedContextArtifact,
};
use super::types::Spec031ContextEvidenceReason;
use shacs_projection::{Spec031Availability, Spec031InclusionReason, Spec031Severity};

pub(super) const fn reason_for_inline(
    artifact: &ResolvedContextArtifact,
) -> Spec031InclusionReason {
    match artifact.state {
        ContextResolutionState::Resolved => Spec031InclusionReason::Included,
        ContextResolutionState::Skipped => match artifact.kind {
            ContextReferenceKind::Unsupported | ContextReferenceKind::Unresolved => {
                Spec031InclusionReason::Unsupported
            }
            ContextReferenceKind::File
            | ContextReferenceKind::Folder
            | ContextReferenceKind::Diff
            | ContextReferenceKind::Staged
            | ContextReferenceKind::Git
            | ContextReferenceKind::Url => Spec031InclusionReason::Skipped,
        },
        ContextResolutionState::Denied => Spec031InclusionReason::Blocked,
        ContextResolutionState::Failed => Spec031InclusionReason::ExtractionFailed,
        ContextResolutionState::Parsed => Spec031InclusionReason::Missing,
    }
}

pub(super) const fn reason_for_file(status: ContextFileReadStatus) -> Spec031InclusionReason {
    match status {
        ContextFileReadStatus::Included | ContextFileReadStatus::Truncated => {
            Spec031InclusionReason::Included
        }
        ContextFileReadStatus::SkippedMissing => Spec031InclusionReason::Missing,
        ContextFileReadStatus::DeniedBoundary => Spec031InclusionReason::Blocked,
        ContextFileReadStatus::ParseError => Spec031InclusionReason::ExtractionFailed,
    }
}

pub(super) const fn evidence_reason(
    reason: Spec031InclusionReason,
) -> Spec031ContextEvidenceReason {
    match reason {
        Spec031InclusionReason::Included => Spec031ContextEvidenceReason::Included,
        Spec031InclusionReason::Skipped | Spec031InclusionReason::Degraded => {
            Spec031ContextEvidenceReason::Skipped
        }
        Spec031InclusionReason::Blocked => Spec031ContextEvidenceReason::Blocked,
        Spec031InclusionReason::Missing => Spec031ContextEvidenceReason::Missing,
        Spec031InclusionReason::Unsupported => Spec031ContextEvidenceReason::Unsupported,
        Spec031InclusionReason::ExtractionFailed => Spec031ContextEvidenceReason::ExtractionFailed,
    }
}

pub(super) const fn availability(reason: Spec031InclusionReason) -> Spec031Availability {
    match reason {
        Spec031InclusionReason::Included => Spec031Availability::Ready,
        Spec031InclusionReason::Skipped | Spec031InclusionReason::Degraded => {
            Spec031Availability::Degraded
        }
        Spec031InclusionReason::Blocked | Spec031InclusionReason::ExtractionFailed => {
            Spec031Availability::Blocked
        }
        Spec031InclusionReason::Missing | Spec031InclusionReason::Unsupported => {
            Spec031Availability::Unavailable
        }
    }
}

pub(super) const fn severity(reason: Spec031InclusionReason) -> Spec031Severity {
    match reason {
        Spec031InclusionReason::Included => Spec031Severity::Info,
        Spec031InclusionReason::Skipped
        | Spec031InclusionReason::Degraded
        | Spec031InclusionReason::Missing
        | Spec031InclusionReason::Unsupported => Spec031Severity::Warning,
        Spec031InclusionReason::Blocked | Spec031InclusionReason::ExtractionFailed => {
            Spec031Severity::Error
        }
    }
}

pub(super) const fn summary(reason: Spec031InclusionReason) -> &'static str {
    match reason {
        Spec031InclusionReason::Included => "context owner evidence included",
        Spec031InclusionReason::Skipped => "context owner evidence skipped",
        Spec031InclusionReason::Blocked => "context owner evidence blocked",
        Spec031InclusionReason::Degraded => "context owner evidence degraded",
        Spec031InclusionReason::Missing => "context owner evidence missing",
        Spec031InclusionReason::Unsupported => "context resolver unsupported",
        Spec031InclusionReason::ExtractionFailed => "context extraction failed",
    }
}
