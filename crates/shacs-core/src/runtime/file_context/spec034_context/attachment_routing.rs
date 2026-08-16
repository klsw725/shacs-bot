use super::super::{AudioContextAnalyzer, StoredAttachmentNote};
use super::attachment_dispatch::{routed_stored_attachment, StoredAttachmentAnalyzers};
use crate::runtime::video_analyzer_runtime::{AnalyzerInvocation, SupervisedVideoAnalyzer};
use crate::runtime::video_analyzer_spec035::VideoAnalyzerSpec035Publisher;
#[cfg(test)]
use crate::runtime::{CancellationToken, VideoContextAnalyzer};
use serde_json::Value;
use shacs_utils::attachments::AttachmentHandoffStatus;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MediaRootRouting {
    Routed(Vec<Value>),
    IgnoredMediaRoot,
    OutsideMediaRoots,
}

#[cfg(test)]
pub(crate) fn route_stored_attachment_with_native_image_support(
    path: &Path,
    media_roots: &[PathBuf],
    native_image_input_supported: bool,
) -> MediaRootRouting {
    route_stored_attachment_with_audio_analyzer(
        path,
        media_roots,
        native_image_input_supported,
        None,
    )
}

#[cfg(test)]
pub(crate) fn route_stored_attachment_with_audio_analyzer(
    path: &Path,
    media_roots: &[PathBuf],
    native_image_input_supported: bool,
    audio_analyzer: Option<&dyn AudioContextAnalyzer>,
) -> MediaRootRouting {
    route_stored_attachment_with_analyzers(
        path,
        media_roots,
        native_image_input_supported,
        audio_analyzer,
        None,
    )
}

#[cfg(test)]
pub(crate) fn route_stored_attachment_with_analyzers(
    path: &Path,
    media_roots: &[PathBuf],
    native_image_input_supported: bool,
    audio_analyzer: Option<&dyn AudioContextAnalyzer>,
    video_analyzer: Option<&dyn VideoContextAnalyzer>,
) -> MediaRootRouting {
    let invocation = AnalyzerInvocation::new(
        std::env::temp_dir().join("shacs-video-analyzer-staging"),
        CancellationToken::new(),
    );
    route_stored_attachment(
        path,
        media_roots,
        native_image_input_supported,
        audio_analyzer,
        video_analyzer.map(super::video_route::VideoAnalyzer::direct),
        &invocation,
        None,
    )
}

pub(crate) fn route_stored_attachment_with_analyzer_invocation(
    path: &Path,
    media_roots: &[PathBuf],
    native_image_input_supported: bool,
    audio_analyzer: Option<&dyn AudioContextAnalyzer>,
    video_analyzer: Option<Arc<SupervisedVideoAnalyzer>>,
    analyzer_invocation: &AnalyzerInvocation,
    video_projection_publisher: Option<&VideoAnalyzerSpec035Publisher>,
) -> MediaRootRouting {
    route_stored_attachment(
        path,
        media_roots,
        native_image_input_supported,
        audio_analyzer,
        video_analyzer.map(super::video_route::VideoAnalyzer::supervised),
        analyzer_invocation,
        video_projection_publisher,
    )
}

fn route_stored_attachment(
    path: &Path,
    media_roots: &[PathBuf],
    native_image_input_supported: bool,
    audio_analyzer: Option<&dyn AudioContextAnalyzer>,
    video_analyzer: Option<super::video_route::VideoAnalyzer<'_>>,
    analyzer_invocation: &AnalyzerInvocation,
    video_projection_publisher: Option<&VideoAnalyzerSpec035Publisher>,
) -> MediaRootRouting {
    if let Some(routing) = route_original_symlink_leaf(path, media_roots) {
        return routing;
    }
    if let Some(routing) = route_original_symlink_parent(path, media_roots) {
        return routing;
    }
    let Ok(candidate) = fs::canonicalize(path) else {
        return route_missing_lexical_stored_attachment(path, media_roots)
            .unwrap_or(MediaRootRouting::OutsideMediaRoots);
    };
    for media_root in media_roots {
        let Ok(canonical_root) = fs::canonicalize(media_root) else {
            continue;
        };
        if !candidate.starts_with(&canonical_root) {
            continue;
        }
        let Ok(relative) = candidate.strip_prefix(&canonical_root) else {
            return MediaRootRouting::IgnoredMediaRoot;
        };
        let Some((channel, attachment_path)) = stored_attachment_relative_path(relative) else {
            return MediaRootRouting::IgnoredMediaRoot;
        };
        return routed_stored_attachment(
            &candidate,
            &channel,
            attachment_path,
            native_image_input_supported,
            StoredAttachmentAnalyzers {
                audio: audio_analyzer,
                video: video_analyzer,
                invocation: analyzer_invocation,
                publisher: video_projection_publisher,
            },
        );
    }
    MediaRootRouting::OutsideMediaRoots
}

fn route_missing_lexical_stored_attachment(
    path: &Path,
    media_roots: &[PathBuf],
) -> Option<MediaRootRouting> {
    for media_root in media_roots {
        if let Some(routing) = route_missing_under_root(path, media_root) {
            return Some(routing);
        }
        let Ok(canonical_root) = fs::canonicalize(media_root) else {
            continue;
        };
        if let Some(routing) = route_missing_under_root(path, &canonical_root) {
            return Some(routing);
        }
    }
    None
}

fn route_missing_under_root(path: &Path, root: &Path) -> Option<MediaRootRouting> {
    if !path.starts_with(root) {
        return None;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return Some(MediaRootRouting::IgnoredMediaRoot);
    };
    let Some((channel, attachment_path)) = stored_attachment_relative_path(relative) else {
        return Some(MediaRootRouting::IgnoredMediaRoot);
    };
    Some(MediaRootRouting::Routed(vec![super::super::note_block(
        AttachmentHandoffStatus::ExtractionFailed,
        StoredAttachmentNote::new(
            &channel,
            &attachment_path,
            None,
            0,
            None,
            "stored attachment could not be resolved",
        ),
    )]))
}

fn route_original_symlink_leaf(path: &Path, media_roots: &[PathBuf]) -> Option<MediaRootRouting> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_symlink() {
        return None;
    }
    let parent = path.parent()?.canonicalize().ok()?;
    let candidate = parent.join(path.file_name()?);
    route_blocked_symlink(
        &candidate,
        media_roots,
        "stored attachment symlink leaf is not allowed",
    )
}

fn route_original_symlink_parent(path: &Path, media_roots: &[PathBuf]) -> Option<MediaRootRouting> {
    for media_root in media_roots {
        let Ok(canonical_root) = fs::canonicalize(media_root) else {
            continue;
        };
        let root = if path.starts_with(media_root) {
            media_root.as_path()
        } else if path.starts_with(&canonical_root) {
            canonical_root.as_path()
        } else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            return Some(MediaRootRouting::IgnoredMediaRoot);
        };
        if !has_symlink_parent(root, relative) {
            return None;
        }
        return route_blocked_relative(relative, "stored attachment symlink parent is not allowed");
    }
    None
}

fn route_blocked_symlink(
    candidate: &Path,
    media_roots: &[PathBuf],
    reason: &str,
) -> Option<MediaRootRouting> {
    for media_root in media_roots {
        let Ok(root) = fs::canonicalize(media_root) else {
            continue;
        };
        if candidate.starts_with(&root) {
            return route_blocked_relative(candidate.strip_prefix(root).ok()?, reason);
        }
    }
    None
}

fn route_blocked_relative(relative: &Path, reason: &str) -> Option<MediaRootRouting> {
    let (channel, attachment_path) = stored_attachment_relative_path(relative)?;
    Some(MediaRootRouting::Routed(vec![super::super::note_block(
        AttachmentHandoffStatus::Blocked,
        StoredAttachmentNote::new(&channel, &attachment_path, None, 0, None, reason),
    )]))
}

fn has_symlink_parent(root: &Path, relative: &Path) -> bool {
    let Some(parent) = relative.parent() else {
        return false;
    };
    let mut current = root.to_path_buf();
    parent.components().any(|component| {
        current.push(component.as_os_str());
        fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink())
    })
}

fn stored_attachment_relative_path(relative: &Path) -> Option<(String, PathBuf)> {
    let mut components = relative.components();
    let attachments = components.next()?.as_os_str().to_str()?;
    let channel = components.next()?.as_os_str().to_str()?;
    if attachments != "attachments" || channel.is_empty() {
        return None;
    }
    let attachment_path = components.collect::<PathBuf>();
    (!attachment_path.as_os_str().is_empty()).then(|| (channel.to_owned(), attachment_path))
}
