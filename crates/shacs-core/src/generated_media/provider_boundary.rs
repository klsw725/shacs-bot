use super::{
    CommittedArtifact, GeneratedArtifactMetadata, GeneratedArtifactRef, MediaLineageId,
    ProviderMediaCandidate,
};
use serde::{Deserialize, Serialize};

pub struct ArtifactWriteRequest {
    pub(crate) candidate: ProviderMediaCandidate,
    pub(crate) metadata: GeneratedArtifactMetadata,
}

impl ArtifactWriteRequest {
    pub fn new(candidate: ProviderMediaCandidate, metadata: GeneratedArtifactMetadata) -> Self {
        Self {
            candidate,
            metadata,
        }
    }
}

impl std::fmt::Debug for ArtifactWriteRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactWriteRequest")
            .field("candidate", &self.candidate)
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMediaFailureReason {
    MalformedPayload,
    ProviderFailure,
    PersistenceFailure,
    UnsupportedOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMediaLifecycleStatus {
    Started,
    Partial,
    Final,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProviderMediaLifecycleEvent(ProviderMediaLifecycleState);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum ProviderMediaLifecycleState {
    Started {
        lineage_id: MediaLineageId,
    },
    Partial {
        lineage_id: MediaLineageId,
        sequence: u32,
    },
    Final {
        lineage_id: MediaLineageId,
        artifact: GeneratedArtifactRef,
    },
    Failed {
        lineage_id: MediaLineageId,
        reason: ProviderMediaFailureReason,
        retryable: bool,
    },
    Cancelled {
        lineage_id: MediaLineageId,
    },
}

impl ProviderMediaLifecycleEvent {
    pub fn started(lineage_id: MediaLineageId) -> Self {
        Self(ProviderMediaLifecycleState::Started { lineage_id })
    }

    pub fn partial(lineage_id: MediaLineageId, sequence: u32) -> Self {
        Self(ProviderMediaLifecycleState::Partial {
            lineage_id,
            sequence,
        })
    }

    pub fn final_artifact(lineage_id: MediaLineageId, committed: &CommittedArtifact) -> Self {
        Self(ProviderMediaLifecycleState::Final {
            lineage_id,
            artifact: committed.artifact_ref(),
        })
    }

    pub fn failed(
        lineage_id: MediaLineageId,
        reason: ProviderMediaFailureReason,
        retryable: bool,
    ) -> Self {
        Self(ProviderMediaLifecycleState::Failed {
            lineage_id,
            reason,
            retryable,
        })
    }

    pub fn cancelled(lineage_id: MediaLineageId) -> Self {
        Self(ProviderMediaLifecycleState::Cancelled { lineage_id })
    }

    pub fn status(&self) -> ProviderMediaLifecycleStatus {
        match &self.0 {
            ProviderMediaLifecycleState::Started { .. } => ProviderMediaLifecycleStatus::Started,
            ProviderMediaLifecycleState::Partial { .. } => ProviderMediaLifecycleStatus::Partial,
            ProviderMediaLifecycleState::Final { .. } => ProviderMediaLifecycleStatus::Final,
            ProviderMediaLifecycleState::Failed { .. } => ProviderMediaLifecycleStatus::Failed,
            ProviderMediaLifecycleState::Cancelled { .. } => {
                ProviderMediaLifecycleStatus::Cancelled
            }
        }
    }
}
