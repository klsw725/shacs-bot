use super::types::{
    ContextFileDigest, ContextFileProjection, ContextFileReadStatus, ContextFileSource,
};
use crate::runtime::context_safety::protected_context_path_reason;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) fn read_context_file(
    order: usize,
    path: PathBuf,
    filename: String,
    source: ContextFileSource,
    source_directory_depth: usize,
    workspace_root: &Path,
    max_bytes: usize,
) -> ContextFileProjection {
    if !path.exists() {
        return projection_with_reason(
            ProjectionInput::new(order, path, filename, source, source_directory_depth),
            ContextFileReadStatus::SkippedMissing,
            "context file candidate is missing",
        );
    }
    let Ok(canonical) = path.canonicalize() else {
        return projection_with_reason(
            ProjectionInput::new(order, path, filename, source, source_directory_depth),
            ContextFileReadStatus::ParseError,
            "context file path could not be canonicalized",
        );
    };
    let input = ProjectionInput::new(
        order,
        canonical.clone(),
        filename,
        source,
        source_directory_depth,
    );
    if !canonical.starts_with(workspace_root) {
        return projection_with_reason(
            ProjectionInput { path, ..input },
            ContextFileReadStatus::DeniedBoundary,
            "context file escapes workspace boundary",
        );
    }
    if let Some(reason) = protected_context_path_reason(&canonical) {
        return projection_with_reason(input, ContextFileReadStatus::DeniedBoundary, reason);
    }
    let Ok(metadata) = fs::metadata(&canonical) else {
        return projection_with_reason(
            input,
            ContextFileReadStatus::ParseError,
            "context file metadata could not be read",
        );
    };
    if !metadata.is_file() {
        return projection_with_reason(
            input,
            ContextFileReadStatus::ParseError,
            "context file candidate is not a regular file",
        );
    }
    read_file(input, &canonical, max_bytes)
}

struct ProjectionInput {
    order: usize,
    path: PathBuf,
    filename: String,
    source: ContextFileSource,
    source_directory_depth: usize,
}

impl ProjectionInput {
    fn new(
        order: usize,
        path: PathBuf,
        filename: String,
        source: ContextFileSource,
        source_directory_depth: usize,
    ) -> Self {
        Self {
            order,
            path,
            filename,
            source,
            source_directory_depth,
        }
    }
}

fn read_file(input: ProjectionInput, canonical: &Path, max_bytes: usize) -> ContextFileProjection {
    let mut bytes = Vec::new();
    let read_result = File::open(canonical).and_then(|mut file| {
        file.by_ref()
            .take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
    });
    if read_result.is_err() {
        return projection_with_reason(
            input,
            ContextFileReadStatus::ParseError,
            "context file content could not be read",
        );
    }
    let truncated = bytes.len() > max_bytes;
    if truncated {
        bytes.truncate(max_bytes);
    }
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let mut entry = projection(
        input,
        if truncated {
            ContextFileReadStatus::Truncated
        } else {
            ContextFileReadStatus::Included
        },
    );
    entry.reason = truncated.then(|| "context file exceeded max byte limit".to_owned());
    entry.digest = Some(ContextFileDigest {
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        byte_count: bytes.len(),
        token_estimate: content.split_whitespace().count().max(content.len() / 4),
    });
    entry.content = Some(content);
    entry
}

fn projection_with_reason(
    input: ProjectionInput,
    status: ContextFileReadStatus,
    reason: &str,
) -> ContextFileProjection {
    let mut entry = projection(input, status);
    entry.reason = Some(reason.to_owned());
    entry
}

fn projection(input: ProjectionInput, status: ContextFileReadStatus) -> ContextFileProjection {
    ContextFileProjection {
        order: input.order,
        path: input.path,
        filename: input.filename,
        source: input.source,
        source_directory_depth: input.source_directory_depth,
        status,
        reason: None,
        digest: None,
        content: None,
    }
}
