use crate::CliError;
use serde_json::Value;
use shacs_projection::Spec035MediaProjection;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;

mod channel_adapter;
mod context_builder;
mod runtime_format;
mod runtime_inspect;
mod tool_registry;

pub(crate) mod wiring {
    pub(crate) use super::runtime_format::format_runtime_inspect;
    pub(crate) use super::runtime_inspect::runtime_inspect_inner;
    pub(crate) use super::tool_registry::production_tool_registry;
}

#[cfg(test)]
const MAX_PROJECTION_BYTES: u64 = 64 * 1024;
#[cfg(test)]
const MAX_PROJECTION_FILES: usize = 64;

pub(crate) struct MediaProjectionPresentation {
    pub(crate) machine_json: String,
    pub(crate) human: String,
}

pub(crate) fn present_media_projection(
    projection: &Spec035MediaProjection,
) -> Result<MediaProjectionPresentation, CliError> {
    let machine_json = serde_json::to_string(projection).map_err(|_| {
        CliError::Runtime("Spec035 media projection serialization failed".to_owned())
    })?;
    let machine = serde_json::from_str(&machine_json).map_err(|_| invalid_projection())?;
    let state = required_label(&machine, "state")?;
    let reason = machine
        .get("reason")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .ok_or_else(invalid_projection)?;
    let summary = machine
        .get("reason")
        .and_then(|value| value.get("safe_summary"))
        .and_then(Value::as_str)
        .ok_or_else(invalid_projection)?;
    let freshness = required_label(&machine, "freshness")?;
    let human = format!(
        "Spec035 media: state={state} reason={reason} freshness={freshness} summary={summary}"
    );
    Ok(MediaProjectionPresentation {
        machine_json,
        human,
    })
}

#[cfg(test)]
pub(crate) fn read_media_projection_directory(
    root: &Path,
) -> Result<Vec<Spec035MediaProjection>, CliError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    if root.symlink_metadata()?.file_type().is_symlink() || !root.is_dir() {
        return Err(invalid_projection());
    }
    let mut paths = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    if paths.len() > MAX_PROJECTION_FILES {
        return Err(invalid_projection());
    }
    let mut projections = Vec::with_capacity(paths.len());
    for path in paths {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_PROJECTION_BYTES
        {
            return Err(invalid_projection());
        }
        let bytes = fs::read(path)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid_projection())?;
        projections
            .push(Spec035MediaProjection::parse_json(text).map_err(|_| invalid_projection())?);
    }
    Ok(projections)
}

fn required_label<'a>(value: &'a Value, key: &str) -> Result<&'a str, CliError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(invalid_projection)
}

fn invalid_projection() -> CliError {
    CliError::Runtime("Spec035 media projection unavailable: invalid canonical record".to_owned())
}

#[cfg(test)]
mod tests;
