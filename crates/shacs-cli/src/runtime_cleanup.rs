use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovedRuntimePathKind {
    UpdateMarker,
    OwnershipMarker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedRuntimePath {
    pub kind: RemovedRuntimePathKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeCleanupReceipt {
    pub removed: Vec<RemovedRuntimePath>,
}
