use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RememberedPermissionStoreErrorKind {
    Malformed,
    UnknownSchemaVersion,
    ForbiddenRawField,
    RuleIdMismatch,
    Io,
    LockUnavailable,
    NotRegularFile,
    Oversized,
    ProjectRuleLimitExceeded,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedPermissionStoreError {
    kind: RememberedPermissionStoreErrorKind,
}

impl RememberedPermissionStoreError {
    pub(crate) const fn new(kind: RememberedPermissionStoreErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> RememberedPermissionStoreErrorKind {
        self.kind
    }
}

impl fmt::Display for RememberedPermissionStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            RememberedPermissionStoreErrorKind::Malformed => {
                "remembered permission store is malformed"
            }
            RememberedPermissionStoreErrorKind::UnknownSchemaVersion => {
                "remembered permission store schema version is unsupported"
            }
            RememberedPermissionStoreErrorKind::ForbiddenRawField => {
                "remembered permission store contains a forbidden raw field"
            }
            RememberedPermissionStoreErrorKind::RuleIdMismatch => {
                "remembered permission rule id does not match its canonical content"
            }
            RememberedPermissionStoreErrorKind::Io => {
                "remembered permission store I/O operation failed"
            }
            RememberedPermissionStoreErrorKind::LockUnavailable => {
                "remembered permission store lock is unavailable"
            }
            RememberedPermissionStoreErrorKind::NotRegularFile => {
                "remembered permission store path is not a regular file"
            }
            RememberedPermissionStoreErrorKind::Oversized => {
                "remembered permission store exceeds the size limit"
            }
            RememberedPermissionStoreErrorKind::ProjectRuleLimitExceeded => {
                "remembered permission project rule limit exceeded"
            }
            RememberedPermissionStoreErrorKind::Symlink => {
                "remembered permission store path is a symlink"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RememberedPermissionStoreError {}

impl From<std::io::Error> for RememberedPermissionStoreError {
    fn from(_error: std::io::Error) -> Self {
        Self::new(RememberedPermissionStoreErrorKind::Io)
    }
}
