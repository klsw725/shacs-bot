use super::model::Spec030ReleaseArtifactError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

pub const SPEC030_MANUAL_QA_SCHEMA: &str = "spec030.manual_qa.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ManualQaRecord {
    pub schema: String,
    pub source_digest: String,
    pub observed_commands: Vec<Spec030ManualCommand>,
    pub non_guarantees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ManualCommand {
    pub id: String,
    pub status: Spec030ManualCommandStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec030ManualCommandStatus {
    Passed,
    Failed,
}

pub fn parse_spec030_manual_qa(
    path: &Path,
    source_digest: &str,
) -> Result<Spec030ManualQaRecord, Spec030ReleaseArtifactError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidManualRecord)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Spec030ReleaseArtifactError::InvalidManualRecord);
    }
    let bytes =
        std::fs::read(path).map_err(|_| Spec030ReleaseArtifactError::InvalidManualRecord)?;
    let record = serde_json::from_slice::<Spec030ManualQaRecord>(&bytes)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidManualRecord)?;
    let commands = record
        .observed_commands
        .iter()
        .filter(|command| command.status == Spec030ManualCommandStatus::Passed)
        .map(|command| command.id.as_str())
        .collect::<BTreeSet<_>>();
    let guarantees = record
        .non_guarantees
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_commands = [
        "cli-json",
        "cli-human",
        "tui-no-session",
        "api-schema-1",
        "api-schema-2",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let required_guarantees = [
        "current_os_user_authority",
        "not_kernel_isolation",
        "optional_adapter_scoped_sandbox",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if record.schema != SPEC030_MANUAL_QA_SCHEMA
        || record.source_digest != source_digest
        || commands != required_commands
        || guarantees != required_guarantees
    {
        return Err(Spec030ReleaseArtifactError::InvalidManualRecord);
    }
    Ok(record)
}
