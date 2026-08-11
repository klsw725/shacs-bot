use super::model::{Spec030CommandEvidenceMode, Spec030ReleaseRunnerMode};
use super::target_catalog::spec030_integration_targets;
use crate::{Spec031ReleaseCommandRecord, Spec031ReleaseGateKind};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) const OWNER_LIFECYCLE_ID: &str = "spec030-bwrap-owner-lifecycle";

pub(super) type CommandEvidenceMode = Spec030CommandEvidenceMode;

pub(super) struct LifecycleCommandSpec {
    pub id: &'static str,
    pub gate: Spec031ReleaseGateKind,
    pub package: &'static str,
    pub filter: &'static str,
    pub argv: &'static [&'static str],
    pub cwd: LifecycleCwdPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LifecycleCwdPolicy {
    ModeWorkspace,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LifecycleCwdRoots<'a> {
    pub runner_mode: Spec030ReleaseRunnerMode,
    pub evidence_root: &'a Path,
    pub repo_root: &'a Path,
}

pub(super) const LIFECYCLE_COMMAND: LifecycleCommandSpec = LifecycleCommandSpec {
    id: OWNER_LIFECYCLE_ID,
    gate: Spec031ReleaseGateKind::FocusedCargoTest,
    package: "shacs-projection",
    filter: "linux_bwrap_owner_lifecycle",
    argv: &[
        "env",
        "SHACS_REQUIRE_BWRAP=1",
        "SHACS_RUNTIME_PACKAGE=shacs-bot-official-container",
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "-p",
        "shacs-projection",
        "--lib",
        "linux_bwrap_owner_lifecycle",
    ],
    cwd: LifecycleCwdPolicy::ModeWorkspace,
};

impl CommandEvidenceMode {
    pub(super) const fn for_runner(mode: Spec030ReleaseRunnerMode) -> Self {
        match mode {
            Spec030ReleaseRunnerMode::SuccessFixture => Self::SuccessFixture,
            Spec030ReleaseRunnerMode::CurrentWorktree if cfg!(target_os = "linux") => {
                Self::LinuxCurrentWorktree
            }
            Spec030ReleaseRunnerMode::CurrentWorktree => Self::ExternalRecord,
        }
    }

    pub(super) const fn owner_lifecycle_id(self) -> Option<&'static str> {
        match self {
            Self::LinuxCurrentWorktree => Some(OWNER_LIFECYCLE_ID),
            Self::SuccessFixture | Self::ExternalRecord => None,
        }
    }
}

pub(super) fn lifecycle_record_matches(
    record: &Spec031ReleaseCommandRecord,
    roots: LifecycleCwdRoots<'_>,
) -> bool {
    record.id == LIFECYCLE_COMMAND.id
        && record.gate == LIFECYCLE_COMMAND.gate
        && record.package.as_deref() == Some(LIFECYCLE_COMMAND.package)
        && record.filter.as_deref() == Some(LIFECYCLE_COMMAND.filter)
        && record
            .argv
            .iter()
            .map(String::as_str)
            .eq(LIFECYCLE_COMMAND.argv.iter().copied())
        && expected_lifecycle_cwd(roots).is_some_and(|cwd| Path::new(&record.cwd) == cwd)
}

fn expected_lifecycle_cwd(roots: LifecycleCwdRoots<'_>) -> Option<PathBuf> {
    let path = match (LIFECYCLE_COMMAND.cwd, roots.runner_mode) {
        (LifecycleCwdPolicy::ModeWorkspace, Spec030ReleaseRunnerMode::SuccessFixture) => {
            roots.evidence_root.join("fixtures/success")
        }
        (LifecycleCwdPolicy::ModeWorkspace, Spec030ReleaseRunnerMode::CurrentWorktree) => {
            roots.repo_root.to_path_buf()
        }
    };
    path.canonicalize().ok()
}

pub(super) fn required_ids(mode: CommandEvidenceMode) -> Vec<&'static str> {
    spec030_integration_targets()
        .iter()
        .map(|target| target.command_id)
        .chain([
            "spec030-bwrap-active",
            "cargo-fmt",
            "cargo-clippy-workspace",
            "cargo-test-workspace",
            "surface-cli-json",
            "surface-cli-human",
            "surface-tui-no-session",
            "surface-tui-runtime",
            "surface-api-schema",
        ])
        .chain(mode.owner_lifecycle_id())
        .collect()
}

pub(super) fn validate_exact_ids<'a>(
    mode: CommandEvidenceMode,
    actual: impl IntoIterator<Item = &'a str>,
) -> bool {
    let required = required_ids(mode).into_iter().collect::<BTreeSet<_>>();
    let actual = actual.into_iter().collect::<Vec<_>>();
    let unique = actual.iter().copied().collect::<BTreeSet<_>>();
    actual.len() == unique.len() && unique == required
}
