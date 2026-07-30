use shacs_config::{
    ConfigContext, RememberedPermissionEffect, RememberedPermissionMatcher,
    RememberedPermissionRule, WorkspacePermissionId,
};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct StoreFixture {
    _root: TempDir,
    pub data_dir: PathBuf,
    pub context: ConfigContext,
    pub workspace_id: WorkspacePermissionId,
}

impl StoreFixture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("config-data");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(&workspace)?;
        let canonical_workspace = workspace.canonicalize()?;
        let context = ConfigContext {
            config_path: data_dir.join("config.json"),
            data_dir: data_dir.clone(),
            workspace,
        };
        let workspace_id = WorkspacePermissionId::from_canonical_workspace_path(
            canonical_workspace.to_string_lossy().as_ref(),
        );
        Ok(Self {
            _root: root,
            data_dir,
            context,
            workspace_id,
        })
    }
}

pub fn exec_rule(command: &str, created_unix_ms: u64) -> RememberedPermissionRule {
    RememberedPermissionRule::new(
        RememberedPermissionEffect::Allow,
        RememberedPermissionMatcher::ExecPrefix {
            tokens: vec![command.to_owned()],
        },
        created_unix_ms,
    )
}
