mod canonical;
mod error;
mod file_store;
mod matcher;
mod rule;
mod store;
mod workspace;

pub use error::{RememberedPermissionStoreError, RememberedPermissionStoreErrorKind};
pub use file_store::RememberedPermissionFileStore;
pub use matcher::{RememberedPermissionEffect, RememberedPermissionMatcher, WorkspacePathScope};
pub use rule::{RememberedPermissionRule, RememberedPermissionRuleId};
pub type RememberedPermissionRemoveByPrefixOutcome =
    store::RememberedPermissionRemoveByPrefixOutcome;
pub use store::RememberedPermissionStore;
pub use workspace::WorkspacePermissionId;

const SCHEMA_VERSION_V1: u32 = 1;
