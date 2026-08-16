use super::super::{
    extract_plain_text, extract_text, is_truncated_text, note_block, route_audio_attachment,
    sha256_prefix, AudioContextAnalyzer, ExtractedText, StoredAttachmentNote,
    MAX_STORED_ATTACHMENT_BYTES, MAX_TEXT_LENGTH,
};
use super::attachment_routing::MediaRootRouting;
use super::video_route::{route_video_attachment, VideoAnalyzer, VideoRoutingAnalyzers};
use crate::runtime::video_analyzer_runtime::AnalyzerInvocation;
use crate::runtime::video_analyzer_spec035::VideoAnalyzerSpec035Publisher;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use shacs_utils::attachments::{
    detect_attachment_mime, AttachmentContentFamily, AttachmentHandoffStatus,
};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct StoredAttachmentAnalyzers<'a> {
    pub(super) audio: Option<&'a dyn AudioContextAnalyzer>,
    pub(super) video: Option<VideoAnalyzer<'a>>,
    pub(super) invocation: &'a AnalyzerInvocation,
    pub(super) publisher: Option<&'a VideoAnalyzerSpec035Publisher>,
}

pub(super) fn routed_stored_attachment(
    path: &Path,
    channel: &str,
    attachment_path: PathBuf,
    native_image_input_supported: bool,
    analyzers: StoredAttachmentAnalyzers<'_>,
) -> MediaRootRouting {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return blocked(
                channel,
                &attachment_path,
                0,
                "stored attachment is not an allowed regular file",
            );
        }
        Ok(metadata) if metadata.len() > MAX_STORED_ATTACHMENT_BYTES => {
            return blocked(
                channel,
                &attachment_path,
                metadata.len(),
                "stored attachment exceeds context routing byte limit",
            );
        }
        Ok(metadata) => metadata,
        Err(_) => {
            return failed(
                channel,
                &attachment_path,
                0,
                "stored attachment metadata could not be read",
            );
        }
    };
    let Ok(bytes) = fs::read(path) else {
        return failed(
            channel,
            &attachment_path,
            metadata.len(),
            "stored attachment could not be read",
        );
    };
    let filename = attachment_path.file_name().and_then(|value| value.to_str());
    let mime = detect_attachment_mime(&bytes, None, filename);
    let family = mime.content_family;
    let digest_prefix = sha256_prefix(&bytes);
    let base_note = StoredAttachmentNote::new(
        channel,
        &attachment_path,
        Some(&mime),
        bytes.len() as u64,
        Some(digest_prefix.as_str()),
        "stored attachment routed",
    );
    let blocks = match family {
        AttachmentContentFamily::Image if native_image_input_supported => {
            let detected_mime = mime
                .detected_mime
                .as_deref()
                .unwrap_or("application/octet-stream");
            vec![json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{detected_mime};base64,{}", STANDARD.encode(bytes))},
            })]
        }
        AttachmentContentFamily::Image => vec![note_block(
            AttachmentHandoffStatus::Unsupported,
            base_note.with_reason("native image input is not supported by provider/model"),
        )],
        AttachmentContentFamily::Text => text_blocks(path, base_note),
        AttachmentContentFamily::Pdf | AttachmentContentFamily::Office => {
            document_blocks(path, base_note)
        }
        AttachmentContentFamily::Audio => route_audio_attachment(
            path,
            &bytes,
            &mime,
            metadata.len(),
            base_note.clone(),
            analyzers.audio,
        ),
        AttachmentContentFamily::Video => route_video_attachment(
            path,
            &bytes,
            &mime,
            metadata.len(),
            base_note.clone(),
            VideoRoutingAnalyzers {
                audio: analyzers.audio,
                video: analyzers.video,
                invocation: analyzers.invocation,
                publisher: analyzers.publisher,
            },
        ),
        AttachmentContentFamily::UnsupportedBinary | AttachmentContentFamily::Unknown => {
            vec![note_block(
                AttachmentHandoffStatus::Unsupported,
                base_note.with_reason(
                    "stored attachment content family is unsupported for text context",
                ),
            )]
        }
    };
    MediaRootRouting::Routed(blocks)
}

fn text_blocks(path: &Path, base_note: StoredAttachmentNote) -> Vec<serde_json::Value> {
    match extract_plain_text(path, MAX_TEXT_LENGTH) {
        Some(extracted) => extracted_blocks(extracted, base_note),
        None => vec![note_block(
            AttachmentHandoffStatus::ExtractionFailed,
            base_note.with_reason("stored attachment text extraction failed"),
        )],
    }
}

fn document_blocks(path: &Path, base_note: StoredAttachmentNote) -> Vec<serde_json::Value> {
    let extracted = extract_text(path, MAX_TEXT_LENGTH)
        .ok()
        .flatten()
        .filter(|text| !text.starts_with("[error:"))
        .map(|text| ExtractedText {
            truncated: is_truncated_text(&text),
            original_chars: text.chars().count(),
            text,
        });
    match extracted {
        Some(extracted) => extracted_blocks(extracted, base_note),
        None => vec![note_block(
            AttachmentHandoffStatus::ExtractionFailed,
            base_note.with_reason("stored attachment text extraction failed"),
        )],
    }
}

fn extracted_blocks(
    extracted: ExtractedText,
    base_note: StoredAttachmentNote,
) -> Vec<serde_json::Value> {
    let status = if extracted.truncated {
        AttachmentHandoffStatus::Truncated
    } else {
        AttachmentHandoffStatus::IncludedText
    };
    vec![
        note_block(status, base_note),
        json!({"type": "text", "text": extracted.text}),
    ]
}

fn blocked(channel: &str, path: &Path, bytes: u64, reason: &str) -> MediaRootRouting {
    MediaRootRouting::Routed(vec![note_block(
        AttachmentHandoffStatus::Blocked,
        StoredAttachmentNote::new(channel, path, None, bytes, None, reason),
    )])
}

fn failed(channel: &str, path: &Path, bytes: u64, reason: &str) -> MediaRootRouting {
    MediaRootRouting::Routed(vec![note_block(
        AttachmentHandoffStatus::ExtractionFailed,
        StoredAttachmentNote::new(channel, path, None, bytes, None, reason),
    )])
}
