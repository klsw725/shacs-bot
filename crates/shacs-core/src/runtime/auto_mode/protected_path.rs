//! Paths whose writes auto mode never approves by an allow rule or the
//! workspace fast path; the classifier reviews them (Spec 048 PRD 000 scope 7).

use super::{real_path, shell_command};
use crate::runtime::context::BOOTSTRAP_FILES;
use crate::runtime::permission_rules::classify_target_value;
use crate::runtime::DEFAULT_CONTEXT_FILE_NAMES;
use std::path::{Component, Path, PathBuf};

const PROTECTED_FOLDERS: &[&str] = &[
    ".git",
    ".vscode",
    ".idea",
    ".husky",
    ".cargo",
    ".devcontainer",
    ".yarn",
    ".mvn",
];
const PROTECTED_FILES: &[&str] = &[
    ".gitconfig",
    ".gitmodules",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_aliases",
    ".bash_logout",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".zlogout",
    ".profile",
    ".envrc",
    ".npmrc",
    ".yarnrc",
    ".yarnrc.yml",
    ".pnp.cjs",
    ".pnp.loader.mjs",
    ".pnpmfile.cjs",
    "bunfig.toml",
    ".bunfig.toml",
    ".bazelrc",
    ".bazelversion",
    ".bazeliskrc",
    ".pre-commit-config.yaml",
    "lefthook.yml",
    "lefthook.yaml",
    ".lefthook.yml",
    ".lefthook.yaml",
    "gradle-wrapper.properties",
    "maven-wrapper.properties",
    ".devcontainer.json",
    ".ripgreprc",
    "pyrightconfig.json",
    ".mcp.json",
];
const WORKSPACE_TREES: &[&str] = &[
    "sessions",
    "skills",
    ".shacs-bot/skills",
    ".nanobot/skills",
    "builtin_skills",
    "workflows",
    "agents",
    ".shacs-bot/plugins",
];
const DATA_TREES: &[&str] = &["runtime", "plugins"];
const REMEMBERED_PERMISSIONS_FILE: &str = "permissions.json";

#[derive(Debug, Clone, Copy)]
pub struct ProtectedPathsInput<'a> {
    pub workspace: &'a Path,
    pub data_folder: &'a Path,
    pub config_file: &'a Path,
    pub extra_skill_roots: &'a [PathBuf],
    pub extra_context_files: &'a [PathBuf],
    pub configured_targets: &'a [String],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtectedPaths {
    workspaces: Vec<PathBuf>,
    trees: Vec<PathBuf>,
    files: Vec<PathBuf>,
    configured_targets: Vec<String>,
}

impl ProtectedPaths {
    pub fn new(input: ProtectedPathsInput<'_>) -> Self {
        let workspaces = spellings(input.workspace);
        let mut trees = Vec::new();
        for workspace in &workspaces {
            trees.extend(WORKSPACE_TREES.iter().map(|tree| workspace.join(tree)));
        }
        for data in spellings(input.data_folder) {
            trees.extend(DATA_TREES.iter().map(|tree| data.join(tree)));
        }
        trees.extend(
            input
                .extra_skill_roots
                .iter()
                .flat_map(|root| spellings(root)),
        );
        let mut files = spellings(input.config_file);
        files.extend(spellings(
            &input.data_folder.join(REMEMBERED_PERMISSIONS_FILE),
        ));
        files.extend(
            input
                .extra_context_files
                .iter()
                .flat_map(|file| spellings(file)),
        );
        Self {
            workspaces,
            trees,
            files,
            configured_targets: input.configured_targets.to_vec(),
        }
    }

    pub(crate) fn contains(&self, path: &Path) -> bool {
        let parts = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>();
        let named = parts.iter().any(|part| PROTECTED_FOLDERS.contains(part))
            || parts.windows(2).any(|pair| pair == [".config", "git"])
            || parts
                .last()
                .is_some_and(|name| PROTECTED_FILES.contains(name));
        named
            || classify_target_value(&path.display().to_string(), &self.configured_targets)
                .is_some()
            || self.files.iter().any(|file| file == path)
            || self.trees.iter().any(|tree| path.starts_with(tree))
            || self.instruction_file(path, &parts)
    }

    fn instruction_file(&self, path: &Path, parts: &[&str]) -> bool {
        let Some(name) = parts.last() else {
            return false;
        };
        self.workspaces.iter().any(|workspace| {
            (path.parent() == Some(workspace.as_path()) && BOOTSTRAP_FILES.contains(name))
                || (path.starts_with(workspace) && DEFAULT_CONTEXT_FILE_NAMES.contains(name))
        })
    }
}

/// The path as written and with symlinks followed.
fn spellings(path: &Path) -> Vec<PathBuf> {
    let mut spellings = vec![shell_command::normalize(path)];
    let real = real_path::real_path(path);
    if !spellings.contains(&real) {
        spellings.push(real);
    }
    spellings
}

#[cfg(test)]
#[path = "protected_path_tests.rs"]
mod tests;
