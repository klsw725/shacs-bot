use std::path::Path;

pub(crate) use super::owner_facts::project as external_owner_facts;
use crate::{spec031_cli, CliError, RuntimeInspectOptions};

pub(crate) fn lines(config_path: &Path, workspace: &Path) -> Vec<String> {
    crate::runtime_inspect_inner(
        RuntimeInspectOptions {
            config_path: Some(config_path.to_path_buf()),
            workspace_override: Some(workspace.to_path_buf()),
        },
        false,
    )
    .and_then(|inspect| {
        spec031_cli::readiness::lines(&inspect).map_err(|error| {
            CliError::InvalidArguments(format!("readiness projection failed: {error}"))
        })
    })
    .unwrap_or_else(|error| {
        vec![format!(
            "Spec031 readiness: state=unavailable reason=missing detail={}",
            shacs_redaction::redact_string(&error.to_string())
        )]
    })
}
