use shacs_projection::{
    Spec031Availability, Spec031Freshness, Spec031ProjectionKind, Spec031ReasonCode,
    Spec031Severity, Spec031SourceOwner,
};

mod build;
#[cfg(test)]
mod parity;
pub(crate) mod readiness;
mod readiness_observation;
mod readiness_queue;
mod readiness_render;
mod render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Projection {
    Session {
        active_turn_count: usize,
        available: bool,
    },
    Subagent {
        child_count: usize,
    },
    Tool {
        attempt_count: usize,
    },
    Diagnostics {
        component_count: usize,
        blocked: bool,
    },
    Readiness {
        available: bool,
    },
    Context {
        included: bool,
    },
    Plugin {
        total_count: usize,
        blocked_count: usize,
    },
    App {
        total_count: usize,
    },
    Media {
        artifact_count: usize,
    },
    Progress {
        blocked: bool,
    },
}

impl Projection {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Session { .. } => "session",
            Self::Subagent { .. } => "subagent",
            Self::Tool { .. } => "tool",
            Self::Diagnostics { .. } => "diagnostics",
            Self::Readiness { .. } => "readiness",
            Self::Context { .. } => "context",
            Self::Plugin { .. } => "plugin",
            Self::App { .. } => "app",
            Self::Media { .. } => "media",
            Self::Progress { .. } => "progress",
        }
    }

    fn kind(self) -> Spec031ProjectionKind {
        match self {
            Self::Session { .. } => Spec031ProjectionKind::Session,
            Self::Subagent { .. } => Spec031ProjectionKind::Subagent,
            Self::Tool { .. } => Spec031ProjectionKind::Tool,
            Self::Diagnostics { .. } => Spec031ProjectionKind::Diagnostics,
            Self::Readiness { .. } => Spec031ProjectionKind::Readiness,
            Self::Context { .. } => Spec031ProjectionKind::Context,
            Self::Plugin { .. } => Spec031ProjectionKind::Plugin,
            Self::App { .. } => Spec031ProjectionKind::App,
            Self::Media { .. } => Spec031ProjectionKind::Media,
            Self::Progress { .. } => Spec031ProjectionKind::Progress,
        }
    }

    pub(super) fn subject_ref(self) -> &'static str {
        match self {
            Self::Session { .. } => "subject:cli:session",
            Self::Subagent { .. } => "subject:cli:subagent",
            Self::Tool { .. } => "subject:cli:tool",
            Self::Diagnostics { .. } => "subject:cli:diagnostics",
            Self::Readiness { .. } => "subject:cli:readiness",
            Self::Context { .. } => "subject:cli:context",
            Self::Plugin { .. } => "subject:cli:plugin",
            Self::App { .. } => "subject:cli:app-owner",
            Self::Media { .. } => "subject:cli:media-owner",
            Self::Progress { .. } => "subject:cli:progress",
        }
    }

    pub(super) fn owner(self) -> Spec031SourceOwner {
        match self {
            Self::Session { .. } => Spec031SourceOwner::Session,
            Self::Subagent { .. } | Self::Tool { .. } => Spec031SourceOwner::Spec030,
            Self::Diagnostics { .. } => Spec031SourceOwner::Spec029,
            Self::Readiness { .. } => Spec031SourceOwner::Spec031,
            Self::App { .. } => Spec031SourceOwner::Spec032,
            Self::Media { .. } => Spec031SourceOwner::Spec034,
            Self::Progress { .. } => Spec031SourceOwner::Channel,
            Self::Context { .. } | Self::Plugin { .. } => Spec031SourceOwner::Spec031,
        }
    }

    pub(super) fn availability(self) -> Spec031Availability {
        match self {
            Self::Session {
                available: true, ..
            }
            | Self::Context { included: true } => Spec031Availability::Ready,
            Self::Session {
                available: false, ..
            } => Spec031Availability::Unavailable,
            Self::Plugin {
                blocked_count: 0, ..
            } => Spec031Availability::Ready,
            Self::Plugin { .. } => Spec031Availability::Degraded,
            Self::Diagnostics { blocked: true, .. }
            | Self::Subagent { .. }
            | Self::Tool { .. }
            | Self::Context { included: false }
            | Self::Progress { blocked: true } => Spec031Availability::Blocked,
            Self::Diagnostics { blocked: false, .. }
            | Self::Readiness { available: true }
            | Self::Progress { blocked: false } => Spec031Availability::Ready,
            Self::Readiness { available: false }
            | Self::App { total_count: 0 }
            | Self::Media { artifact_count: 0 } => Spec031Availability::Unavailable,
            Self::App { .. } | Self::Media { .. } => Spec031Availability::Ready,
        }
    }

    pub(super) fn reason_code(self) -> Spec031ReasonCode {
        match self {
            Self::App { total_count: 0 } | Self::Media { artifact_count: 0 } => {
                Spec031ReasonCode::MissingExternalOwnerEvidence
            }
            _ => match self.availability() {
                Spec031Availability::Ready => Spec031ReasonCode::Included,
                Spec031Availability::Degraded => Spec031ReasonCode::Degraded,
                Spec031Availability::Blocked => Spec031ReasonCode::Blocked,
                Spec031Availability::Unavailable | Spec031Availability::Unknown => {
                    Spec031ReasonCode::Missing
                }
            },
        }
    }

    pub(super) fn freshness(self) -> Spec031Freshness {
        match self.availability() {
            Spec031Availability::Unavailable => Spec031Freshness::Unavailable,
            Spec031Availability::Blocked | Spec031Availability::Degraded => Spec031Freshness::Stale,
            Spec031Availability::Ready => Spec031Freshness::Current,
            Spec031Availability::Unknown => Spec031Freshness::Unknown,
        }
    }
}

pub(crate) fn lines(projections: &[Projection]) -> Vec<String> {
    projections.iter().copied().map(render::line).collect()
}

pub(crate) fn push(lines: &mut Vec<String>, projections: &[Projection]) {
    lines.extend(self::lines(projections));
}

pub(super) fn severity(availability: Spec031Availability) -> Spec031Severity {
    match availability {
        Spec031Availability::Ready => Spec031Severity::Info,
        Spec031Availability::Degraded | Spec031Availability::Unknown => Spec031Severity::Warning,
        Spec031Availability::Blocked | Spec031Availability::Unavailable => Spec031Severity::Error,
    }
}

pub(super) fn reason_summary(code: Spec031ReasonCode) -> &'static str {
    match code {
        Spec031ReasonCode::Included => "included",
        Spec031ReasonCode::Skipped => "skipped",
        Spec031ReasonCode::Blocked => "blocked",
        Spec031ReasonCode::Degraded => "degraded",
        Spec031ReasonCode::Missing => "missing",
        Spec031ReasonCode::Unsupported => "unsupported",
        Spec031ReasonCode::ExtractionFailed => "extraction_failed",
        Spec031ReasonCode::MissingExternalOwnerEvidence => "missing_external_owner_evidence",
        Spec031ReasonCode::Requested => "requested",
        Spec031ReasonCode::Completed => "completed",
        Spec031ReasonCode::Progress => "progress",
        Spec031ReasonCode::Final => "final",
        Spec031ReasonCode::Interrupted => "interrupted",
        Spec031ReasonCode::RecoveryRequested => "recovery_requested",
        Spec031ReasonCode::RecoveryCompleted => "recovery_completed",
        Spec031ReasonCode::RepeatedInterruption => "repeated_interruption",
        Spec031ReasonCode::PendingFollowUp => "pending_follow_up",
        Spec031ReasonCode::RetryConsumed => "retry_consumed",
    }
}
