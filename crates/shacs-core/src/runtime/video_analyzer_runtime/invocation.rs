use super::staging::next_staging_directory;
use crate::controlled_child::{ControlledChildAbort, ControlledChildCommand};
use crate::runtime::{CancellationToken, VideoAnalyzerSnapshotProjection};
use shacs_projection::Spec031ExternalOwnerRef;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzerMediaProvenance {
    Inbound,
    Generated,
}

#[derive(Debug, Clone)]
pub struct AnalyzerInvocation {
    cancellation: CancellationToken,
    deadline: Option<Instant>,
    staging_root: PathBuf,
    staging_directory: PathBuf,
    owner_ref: Option<Spec031ExternalOwnerRef>,
    snapshot_ref: Option<VideoAnalyzerSnapshotProjection>,
    provenance: AnalyzerMediaProvenance,
}

impl AnalyzerInvocation {
    pub fn new(staging_root: impl Into<PathBuf>, cancellation: CancellationToken) -> Self {
        let staging_root = staging_root.into();
        let staging_directory = next_staging_directory(&staging_root);
        Self {
            cancellation,
            deadline: None,
            staging_root,
            staging_directory,
            owner_ref: None,
            snapshot_ref: None,
            provenance: AnalyzerMediaProvenance::Inbound,
        }
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_owner_refs(
        mut self,
        owner_ref: Spec031ExternalOwnerRef,
        snapshot_ref: VideoAnalyzerSnapshotProjection,
    ) -> Self {
        self.owner_ref = Some(owner_ref);
        self.snapshot_ref = Some(snapshot_ref);
        self
    }

    pub fn with_provenance(mut self, provenance: AnalyzerMediaProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub fn deadline_elapsed(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn remaining_duration(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn staging_directory(&self) -> &Path {
        &self.staging_directory
    }

    pub fn owner_ref(&self) -> Option<&Spec031ExternalOwnerRef> {
        self.owner_ref.as_ref()
    }

    pub fn snapshot_ref(&self) -> Option<&VideoAnalyzerSnapshotProjection> {
        self.snapshot_ref.as_ref()
    }

    pub const fn provenance(&self) -> AnalyzerMediaProvenance {
        self.provenance
    }

    pub fn controlled_child_abort(&self) -> ControlledChildAbort {
        self.cancellation.controlled_child_abort()
    }

    pub fn apply_to_controlled_child(&self, command: &mut ControlledChildCommand) {
        if let Some(remaining) = self.remaining_duration() {
            command.timeout = command.timeout.min(remaining);
        }
    }

    pub(super) fn staging_root(&self) -> &Path {
        &self.staging_root
    }

    pub(crate) fn child(&self) -> Self {
        Self {
            cancellation: self.cancellation.clone(),
            deadline: self.deadline,
            staging_root: self.staging_root.clone(),
            staging_directory: next_staging_directory(&self.staging_root),
            owner_ref: self.owner_ref.clone(),
            snapshot_ref: self.snapshot_ref.clone(),
            provenance: self.provenance,
        }
    }
}
