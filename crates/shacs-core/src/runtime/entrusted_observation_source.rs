use std::path::{Path, PathBuf};

use super::{
    IntegerObservationSummary, ObservationBlock, ObservationResult, ObservationSource,
    ObservationWake,
};
use crate::tools::{JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolResult};

pub struct BoundedObservationTool {
    workspace: PathBuf,
    wake: ObservationWake,
}

impl BoundedObservationTool {
    pub fn new(workspace: &Path, wake: &ObservationWake) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            wake: wake.clone(),
        }
    }
}

impl Tool for BoundedObservationTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read bounded entrusted local evidence with an opt-in integer summary"
    }
    fn parameters(&self) -> serde_json::Value {
        ToolParameters::new()
            .property("path", StringSchema::new("Workspace-relative file"))
            .required(["path"])
            .to_json_schema()
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        let ObservationSource::LocalFile { relative_path } = &self.wake.plan.source;
        let result = if params.get("path").and_then(serde_json::Value::as_str)
            != Some(relative_path)
            || self.wake.plan.validate().is_err()
        {
            ObservationResult::Blocked {
                reason: ObservationBlock::UnsupportedSource,
            }
        } else {
            match read_bounded_file(&self.workspace, &self.wake) {
                Ok(result) => result,
                Err(reason) => ObservationResult::Unavailable { reason },
            }
        };
        match serde_json::to_string(&result) {
            Ok(text) => text.into(),
            Err(_) => "Error: observation serialization failed".into(),
        }
    }
}

#[cfg(unix)]
fn read_bounded_file(
    workspace: &Path,
    wake: &ObservationWake,
) -> Result<ObservationResult, ObservationBlock> {
    use super::MAX_OBSERVATION_BYTES;
    use rustix::fs::{open, openat, Mode, OFlags};
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;

    let ObservationSource::LocalFile {
        relative_path: relative,
    } = &wake.plan.source;
    let root = open(
        workspace,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ObservationBlock::ReadFailed)?;
    let root_file = std::fs::File::from(root);
    let root_uid = root_file
        .metadata()
        .map_err(|_| ObservationBlock::ReadFailed)?
        .uid();
    if root_uid != rustix::process::geteuid().as_raw() {
        return Err(ObservationBlock::UnsupportedSource);
    }
    let mut directory = root_file;
    let mut parts = Path::new(relative).components().peekable();
    while let Some(part) = parts.next() {
        let mut flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
        if parts.peek().is_some() {
            flags |= OFlags::DIRECTORY;
        }
        let descriptor = openat(&directory, part.as_os_str(), flags, Mode::empty()).map_err(
            |error| match error {
                rustix::io::Errno::NOENT => ObservationBlock::SourceMissing,
                rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR => {
                    ObservationBlock::UnsupportedSource
                }
                _ => ObservationBlock::ReadFailed,
            },
        )?;
        directory = std::fs::File::from(descriptor);
    }
    let metadata = directory
        .metadata()
        .map_err(|_| ObservationBlock::ReadFailed)?;
    if !metadata.is_file() || metadata.uid() != root_uid {
        return Err(ObservationBlock::UnsupportedSource);
    }
    if metadata.mode() & 0o444 == 0 {
        return Err(ObservationBlock::ReadFailed);
    }
    if metadata.len() > MAX_OBSERVATION_BYTES {
        return Err(ObservationBlock::SourceTooLarge);
    }
    let mut bytes = Vec::new();
    directory
        .take(MAX_OBSERVATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ObservationBlock::ReadFailed)?;
    let byte_count = u64::try_from(bytes.len()).map_err(|_| ObservationBlock::SourceTooLarge)?;
    if byte_count > MAX_OBSERVATION_BYTES {
        return Err(ObservationBlock::SourceTooLarge);
    }
    let summary = match &wake.plan.comparison {
        Some(_) => serde_json::from_slice::<IntegerObservationSummary>(&bytes).ok(),
        None => None,
    };
    Ok(ObservationResult::Observed {
        evidence_ref: format!("sha256:{:x}", Sha256::digest(&bytes)),
        byte_count,
        summary,
    })
}

#[cfg(not(unix))]
fn read_bounded_file(
    _workspace: &Path,
    _wake: &ObservationWake,
) -> Result<ObservationResult, ObservationBlock> {
    Err(ObservationBlock::UnsupportedSource)
}
