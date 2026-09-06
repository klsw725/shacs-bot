use super::super::{
    note_block, sniff_video_duration_seconds, AudioContextAnalyzer, StoredAttachmentNote,
};
use super::video_blocks::video_analysis_blocks;
use crate::runtime::video_analyzer_runtime::{
    run_supervised_video_analyzer, AnalyzerInvocation, SupervisedVideoAnalyzer,
    SupervisedVideoAnalyzerOutcome,
};
use crate::runtime::video_analyzer_spec035::{
    VideoAnalyzerSpec035PublicationStatus, VideoAnalyzerSpec035Publisher,
};
use crate::runtime::{VideoAnalysisPolicy, VideoContextError, VideoContextRequest};
use serde_json::Value;
use shacs_utils::attachments::{AttachmentHandoffStatus, MimeDetectionMetadata};
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub(super) struct VideoRoutingAnalyzers<'a> {
    pub(super) audio: Option<&'a dyn AudioContextAnalyzer>,
    pub(super) video: Option<VideoAnalyzer<'a>>,
    pub(super) invocation: &'a AnalyzerInvocation,
    pub(super) publisher: Option<&'a VideoAnalyzerSpec035Publisher>,
}

pub(super) struct VideoAnalyzer<'a> {
    supervised: Option<Arc<SupervisedVideoAnalyzer>>,
    direct: Option<&'a dyn crate::runtime::VideoContextAnalyzer>,
}

impl<'a> VideoAnalyzer<'a> {
    pub(super) fn supervised(analyzer: Arc<SupervisedVideoAnalyzer>) -> Self {
        Self {
            supervised: Some(analyzer),
            direct: None,
        }
    }

    #[cfg(test)]
    pub(super) fn direct(analyzer: &'a dyn crate::runtime::VideoContextAnalyzer) -> Self {
        Self {
            supervised: None,
            direct: Some(analyzer),
        }
    }
}

pub(super) fn route_video_attachment(
    path: &Path,
    bytes: &[u8],
    mime: &MimeDetectionMetadata,
    byte_length: u64,
    base_note: StoredAttachmentNote,
    analyzers: VideoRoutingAnalyzers<'_>,
) -> Vec<Value> {
    let policy = VideoAnalysisPolicy::default();
    if byte_length > policy.max_byte_length {
        return unsupported(
            base_note,
            format!(
                "video exceeds configured byte limit: bytes={byte_length} limit_bytes={}",
                policy.max_byte_length
            ),
        );
    }
    let duration_seconds = sniff_video_duration_seconds(bytes, mime.detected_mime.as_deref());
    if duration_seconds.is_some_and(|duration| duration > policy.max_duration_seconds) {
        return unsupported(
            base_note,
            format!(
                "video duration exceeds configured limit: duration_seconds={} limit_seconds={}",
                duration_seconds.unwrap_or_default(),
                policy.max_duration_seconds
            ),
        );
    }
    if duration_seconds.is_none() && !policy.allow_unknown_duration {
        return unsupported(
            base_note,
            "video duration could not be determined".to_owned(),
        );
    }
    let Some(analyzer) = analyzers.video else {
        return unsupported(base_note, "video analyzer is not configured".to_owned());
    };
    let request = VideoContextRequest {
        file_path: path.to_path_buf(),
        detected_mime: mime
            .detected_mime
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        byte_length,
        duration_seconds,
        policy,
    };
    let result = match (analyzer.supervised, analyzer.direct) {
        (Some(analyzer), None) => {
            let completion =
                run_supervised_video_analyzer(analyzer, analyzers.invocation.child(), request);
            match &completion {
                SupervisedVideoAnalyzerOutcome::Completed(completed) => completed.result().clone(),
                SupervisedVideoAnalyzerOutcome::Busy => Err(VideoContextError::Failed(
                    "video analyzer is busy".to_owned(),
                )),
                SupervisedVideoAnalyzerOutcome::Cancelled => Err(VideoContextError::Cancelled),
                SupervisedVideoAnalyzerOutcome::TimedOut => Err(VideoContextError::TimedOut),
                SupervisedVideoAnalyzerOutcome::Failed => Err(VideoContextError::Failed(
                    "video analyzer worker failed".to_owned(),
                )),
            }
        }
        (None, Some(analyzer)) => analyzer.analyze(analyzers.invocation, request),
        (None, None) | (Some(_), Some(_)) => Err(VideoContextError::Failed(
            "video analyzer routing is invalid".to_owned(),
        )),
    };
    if let Some(publisher) = analyzers.publisher {
        match publisher.publish_result(bytes, duration_seconds, policy, &result) {
            Ok(status) => {
                if let Some(blocks) = publication_status_blocks(status, base_note.clone()) {
                    return blocks;
                }
            }
            Err(_) => {
                return vec![note_block(
                    AttachmentHandoffStatus::ExtractionFailed,
                    base_note.with_reason("video projection publication failed"),
                )];
            }
        }
    }
    match result {
        Ok(analysis) => video_analysis_blocks(base_note, path, analysis, policy, analyzers.audio),
        Err(VideoContextError::Unsupported(reason)) => unsupported(
            base_note,
            super::super::safe_video_error_reason(&reason, "video format is not supported"),
        ),
        Err(VideoContextError::Failed(reason)) => vec![note_block(
            AttachmentHandoffStatus::ExtractionFailed,
            base_note.with_reason(super::super::safe_video_error_reason(
                &reason,
                "video analyzer failed",
            )),
        )],
        Err(VideoContextError::Cancelled) => vec![note_block(
            AttachmentHandoffStatus::Cancelled,
            base_note.with_reason("video analyzer cancelled"),
        )],
        Err(VideoContextError::TimedOut) => vec![note_block(
            AttachmentHandoffStatus::TimedOut,
            base_note.with_reason("video analyzer timed out"),
        )],
    }
}

fn publication_status_blocks(
    status: VideoAnalyzerSpec035PublicationStatus,
    base_note: StoredAttachmentNote,
) -> Option<Vec<Value>> {
    match status {
        VideoAnalyzerSpec035PublicationStatus::Published
        | VideoAnalyzerSpec035PublicationStatus::Reconciled => None,
        VideoAnalyzerSpec035PublicationStatus::CommitStatusUnknown => Some(vec![note_block(
            AttachmentHandoffStatus::Deferred,
            base_note.with_reason("video projection commit status unknown"),
        )]),
    }
}

fn unsupported(base_note: StoredAttachmentNote, reason: String) -> Vec<Value> {
    vec![note_block(
        AttachmentHandoffStatus::Unsupported,
        base_note.with_reason(reason),
    )]
}
