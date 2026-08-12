use crate::ConfigContext;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLayoutEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLayoutOwner {
    UserConfig,
    CredentialStore,
    SessionStore,
    UserSkills,
    RuntimeProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLayoutCreation {
    ConfigWriter,
    CredentialWriter,
    SessionWriter,
    RuntimeStartup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLayoutMutation {
    UserManaged,
    OwnerAdmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLayoutCleanup {
    Preserve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLayoutEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: RuntimeLayoutEntryKind,
    pub owner: RuntimeLayoutOwner,
    pub marker_path: Option<PathBuf>,
    pub creation: RuntimeLayoutCreation,
    pub mutation: RuntimeLayoutMutation,
    pub cleanup: RuntimeLayoutCleanup,
}

pub fn runtime_layout(context: &ConfigContext) -> Vec<RuntimeLayoutEntry> {
    let marker = context.data_dir.join("runtime/ownership-marker.json");
    vec![
        entry(
            "config",
            context.config_path.clone(),
            RuntimeLayoutEntryKind::File,
            RuntimeLayoutOwner::UserConfig,
            None,
            RuntimeLayoutCreation::ConfigWriter,
            RuntimeLayoutMutation::UserManaged,
        ),
        entry(
            "auth",
            context.auth_path(),
            RuntimeLayoutEntryKind::File,
            RuntimeLayoutOwner::CredentialStore,
            None,
            RuntimeLayoutCreation::CredentialWriter,
            RuntimeLayoutMutation::UserManaged,
        ),
        entry(
            "sessions",
            context.workspace.join("sessions"),
            RuntimeLayoutEntryKind::Directory,
            RuntimeLayoutOwner::SessionStore,
            None,
            RuntimeLayoutCreation::SessionWriter,
            RuntimeLayoutMutation::UserManaged,
        ),
        runtime_entry("media", context.data_dir.join("media"), &marker),
        runtime_entry("logs", context.data_dir.join("logs"), &marker),
        runtime_entry("channels", context.data_dir.join("channels"), &marker),
        entry(
            "skills",
            context.data_dir.join("skills"),
            RuntimeLayoutEntryKind::Directory,
            RuntimeLayoutOwner::UserSkills,
            None,
            RuntimeLayoutCreation::RuntimeStartup,
            RuntimeLayoutMutation::UserManaged,
        ),
        runtime_entry("cache", context.data_dir.join("cache"), &marker),
        runtime_entry("tmp", context.data_dir.join("tmp"), &marker),
        runtime_entry("snapshots", context.data_dir.join("snapshots"), &marker),
    ]
}

fn runtime_entry(name: &str, path: PathBuf, marker: &Path) -> RuntimeLayoutEntry {
    entry(
        name,
        path,
        RuntimeLayoutEntryKind::Directory,
        RuntimeLayoutOwner::RuntimeProcess,
        Some(marker.to_path_buf()),
        RuntimeLayoutCreation::RuntimeStartup,
        RuntimeLayoutMutation::OwnerAdmitted,
    )
}

fn entry(
    name: &str,
    path: PathBuf,
    kind: RuntimeLayoutEntryKind,
    owner: RuntimeLayoutOwner,
    marker_path: Option<PathBuf>,
    creation: RuntimeLayoutCreation,
    mutation: RuntimeLayoutMutation,
) -> RuntimeLayoutEntry {
    RuntimeLayoutEntry {
        name: name.to_owned(),
        path,
        kind,
        owner,
        marker_path,
        creation,
        mutation,
        cleanup: RuntimeLayoutCleanup::Preserve,
    }
}
