//! A refusal's identity, independent of its provider call id (Spec 053).

use super::auto_mode::{real_path, shell_command, shell_paths};
use super::{PermissionedAction, ToolExecutionContext};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RefusalKind {
    Workspace,
    WorkingDirectory,
    FileRange,
    DenyPattern,
    InvalidDenyPattern,
    AllowPattern,
    InternalUrl,
    ToolPreflight,
}

impl RefusalKind {
    pub(crate) fn from_text(text: &str) -> Option<Self> {
        let text = text.to_ascii_lowercase();
        if text.contains("outside allowed directory") {
            Some(Self::FileRange)
        } else if text.contains("working_dir could not be resolved") {
            Some(Self::WorkingDirectory)
        } else if [
            "outside the configured workspace",
            "working_dir is outside",
            "path traversal detected",
            "path outside working dir",
        ]
        .iter()
        .any(|marker| text.contains(marker))
        {
            Some(Self::Workspace)
        } else if text.contains("blocked by safety guard") {
            Some(if text.contains("invalid deny pattern") {
                Self::InvalidDenyPattern
            } else if text.contains("dangerous pattern detected") {
                Self::DenyPattern
            } else if text.contains("not in allowlist") {
                Self::AllowPattern
            } else if text.contains("internal/private url detected") {
                Self::InternalUrl
            } else {
                Self::ToolPreflight
            })
        } else {
            None
        }
    }
}

/// Future refusal producers supply a kind and normalized targets here;
/// admission, counting and presentation remain shared.
pub(crate) struct ToolRefusal {
    pub(crate) tool: String,
    pub(crate) kind: RefusalKind,
    pub(crate) targets: Vec<PathBuf>,
}

impl ToolRefusal {
    pub(crate) fn for_action(
        action: &PermissionedAction,
        kind: RefusalKind,
        context: &ToolExecutionContext,
    ) -> Self {
        let arguments = action.decision_arguments();
        let workspace = real_path::real_path(
            context
                .active_workspace
                .as_deref()
                .unwrap_or(Path::new(".")),
        );
        let home = context
            .permission_environment
            .as_ref()
            .map(|environment| environment.home.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let mut targets = Vec::new();
        match kind {
            RefusalKind::FileRange | RefusalKind::ToolPreflight => {
                if let Some(path) = arguments.get("path").and_then(Value::as_str) {
                    targets.push(normalize(path, &workspace, &home));
                }
            }
            RefusalKind::Workspace | RefusalKind::WorkingDirectory => {
                let folder = arguments
                    .get("working_dir")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || workspace.to_path_buf(),
                        |path| normalize(path, &workspace, &home),
                    );
                if kind == RefusalKind::WorkingDirectory || !folder.starts_with(&workspace) {
                    targets.push(real_path::real_path(&folder));
                } else if let Some(command) = arguments.get("command").and_then(Value::as_str) {
                    let analysis = shell_command::analyze(command, &folder, &home);
                    for subcommand in &analysis.subcommands {
                        let paths = shell_paths::subcommand_paths(subcommand, &home);
                        let words = paths.reads.iter().chain(&paths.writes);
                        for word in words {
                            if word.word.text.starts_with(['/', '~'])
                                || word.word.text.contains("../")
                                || word.word.text.contains("..\\")
                            {
                                targets.extend(
                                    word.paths.iter().map(|path| real_path::real_path(path)),
                                );
                            }
                        }
                    }
                } else if let Some(path) = arguments.get("path").and_then(Value::as_str) {
                    targets.push(normalize(path, &workspace, &home));
                }
            }
            RefusalKind::DenyPattern
            | RefusalKind::InvalidDenyPattern
            | RefusalKind::AllowPattern
            | RefusalKind::InternalUrl => {}
        }
        targets.sort();
        targets.dedup();
        Self {
            tool: action.tool_name.clone(),
            kind,
            targets,
        }
    }

    pub(crate) fn key(&self) -> String {
        let value = serde_json::json!([self.tool, self.kind, self.targets]);
        format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
    }
}

fn normalize(path: &str, workspace: &Path, home: &Path) -> PathBuf {
    let path = if path == "~" {
        home.to_path_buf()
    } else if let Some(tail) = path.strip_prefix("~/") {
        home.join(tail)
    } else {
        workspace.join(path)
    };
    real_path::real_path(&path)
}
