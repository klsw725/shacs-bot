use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RememberedPermissionEffect {
    Allow,
    Deny,
}

impl RememberedPermissionEffect {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePathScope {
    Exact,
    Subtree,
}

impl WorkspacePathScope {
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Subtree => "subtree",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RememberedPermissionMatcher {
    ExactAction {
        action_digest: String,
    },
    ExecPrefix {
        tokens: Vec<String>,
    },
    WorkspacePath {
        tool_name: String,
        path: String,
        scope: WorkspacePathScope,
    },
    WebOrigin {
        origin: String,
    },
    McpTool {
        tool_name: String,
    },
}
