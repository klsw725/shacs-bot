use super::canonical::sha256_hex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspacePermissionId(String);

impl WorkspacePermissionId {
    pub fn from_canonical_workspace_path(path: &str) -> Self {
        Self(format!("workspace:sha256:{}", sha256_hex(path.as_bytes())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
