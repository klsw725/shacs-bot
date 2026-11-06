//! What the permission decision needs to know about the user's folders and
//! rules (Spec 048 PRD 000 scopes 4, 5, 7 and 8).

use super::classifier_slots::{AutoModeSlots, SlotOverrides};
use super::critical_path::CriticalFolders;
use super::path_pattern::PathBases;
use super::permission_rule::{RawRuleLists, UserPermissionRules};
use super::protected_path::{ProtectedPaths, ProtectedPathsInput};
use super::rule_match::{self, PathPair};
use super::secret_file::SecretFiles;
use super::secret_values::SecretValues;
use super::user_only_file::UserOnlyFiles;
use super::{real_path, shell_command};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct PermissionEnvironmentInput<'a> {
    pub workspace: &'a Path,
    pub data_folder: &'a Path,
    pub config_file: &'a Path,
    pub home: &'a Path,
    pub rules: RawRuleLists<'a>,
    pub mcp_servers: &'a [String],
    pub extra_skill_roots: &'a [PathBuf],
    pub extra_context_files: &'a [PathBuf],
    pub configured_targets: &'a [String],
    pub classify_all_shell: bool,
    pub slots: SlotOverrides<'a>,
    pub classifier_model: Option<&'a str>,
    /// The `.env.json` values to mask (Spec 048 PRD 010 scope 10), and
    /// whether they were read for this turn rather than kept from an earlier
    /// read.
    pub env_file: &'a shacs_config::EnvFile,
    pub env_file_current: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionEnvironment {
    pub user_rules: UserPermissionRules,
    pub(crate) secret_files: SecretFiles,
    pub(crate) user_only_files: UserOnlyFiles,
    pub(crate) protected_paths: ProtectedPaths,
    pub(crate) critical_folders: CriticalFolders,
    pub(crate) home: PathBuf,
    pub(crate) workspace: PathBuf,
    pub(crate) real_workspace: PathBuf,
    pub(crate) config_folder: PathBuf,
    pub(crate) classify_all_shell: bool,
    pub slots: AutoModeSlots,
    pub classifier_model: Option<String>,
    pub(crate) secret_values: SecretValues,
    /// The classifier is not called when the values could not be read for
    /// this turn (Spec 048 PRD 010 scope 10).
    pub(crate) secret_values_current: bool,
}

impl PermissionEnvironment {
    pub fn new(input: PermissionEnvironmentInput<'_>) -> Self {
        let config_folder = input
            .config_file
            .parent()
            .map_or_else(|| input.data_folder.to_path_buf(), Path::to_path_buf);
        Self {
            user_rules: UserPermissionRules::parse(input.rules, input.mcp_servers),
            secret_files: SecretFiles::new(input.data_folder, input.config_file),
            user_only_files: UserOnlyFiles::new(input.data_folder),
            protected_paths: ProtectedPaths::new(ProtectedPathsInput {
                workspace: input.workspace,
                data_folder: input.data_folder,
                config_file: input.config_file,
                extra_skill_roots: input.extra_skill_roots,
                extra_context_files: input.extra_context_files,
                configured_targets: input.configured_targets,
            }),
            critical_folders: CriticalFolders::new(input.home, input.workspace, input.data_folder),
            home: input.home.to_path_buf(),
            workspace: shell_command::normalize(input.workspace),
            real_workspace: real_path::real_path(input.workspace),
            config_folder,
            classify_all_shell: input.classify_all_shell,
            slots: AutoModeSlots::resolve(input.slots),
            classifier_model: input.classifier_model.map(str::to_owned),
            secret_values: SecretValues::new(input.env_file),
            secret_values_current: input.env_file_current,
        }
    }

    /// The `.env.json` values this environment masks.
    pub fn secret_values(&self) -> &SecretValues {
        &self.secret_values
    }

    /// Whether the values were read for this turn; the classifier is not
    /// called otherwise.
    pub fn secret_values_current(&self) -> bool {
        self.secret_values_current
    }

    pub(crate) fn bases(&self) -> PathBases<'_> {
        PathBases {
            home: &self.home,
            config_folder: &self.config_folder,
            workspace: &self.workspace,
        }
    }

    /// Whether a reader working for the user must leave `path` unread, as
    /// folder scans do (Spec 048 PRD 050 scope 7).
    pub fn withholds_read(&self, path: &Path) -> bool {
        self.hidden_from_scans(path)
    }

    /// Whether folder scans leave `path` out: the auth store and the paths a
    /// `read` deny or ask rule covers (Spec 048 PRD 000 scopes 4 and 5).
    pub(crate) fn hidden_from_scans(&self, path: &Path) -> bool {
        if self.secret_files.contains(path) {
            return true;
        }
        let pair = PathPair {
            requested: shell_command::normalize(path),
            real: real_path::real_path(path),
        };
        rule_match::read_rule_hides(&self.user_rules, &pair, self.bases())
    }

    pub(crate) fn inside_workspace(&self, requested: &Path, real: &Path) -> bool {
        (requested.starts_with(&self.workspace) || requested.starts_with(&self.real_workspace))
            && real.starts_with(&self.real_workspace)
    }
}
