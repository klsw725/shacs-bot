use crate::text::{detect_image_mime, safe_filename};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_redaction::redact_string;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;
pub const DEFAULT_MAX_BYTES_PER_FILE: u64 = 10 * 1024 * 1024;
pub const DEFAULT_MAX_BYTES_PER_TURN: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAttachmentIntakeRequest {
    pub session_key: String,
    pub turn_id: Option<String>,
    pub channel: String,
    pub external_message_id: Option<String>,
    pub source_display_name: Option<String>,
    pub original_filename: Option<String>,
    pub declared_mime: Option<String>,
    pub declared_byte_length: Option<u64>,
    pub bytes: Vec<u8>,
    pub received_at_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentSourceKind {
    PlatformDownload,
    InlineBytes,
    DataUrl,
    MimePart,
    BridgeMediaHandle,
    LocalMultipart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAttachmentAdapterFailureReason {
    MalformedDataUrl,
    UnsupportedDataUrlMimeType,
    PayloadTooLargeBeforeStorage,
    MissingCredential,
    PlatformDownloadFailed,
    MimePartDecodeFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAttachmentAdapterDiagnostic {
    pub source_kind: AttachmentSourceKind,
    pub reason: ChannelAttachmentAdapterFailureReason,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAttachmentAdapterFailure {
    pub item_index: usize,
    pub diagnostic: ChannelAttachmentAdapterDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelAttachmentAdapterResult {
    pub requests: Vec<ChannelAttachmentIntakeRequest>,
    pub failures: Vec<ChannelAttachmentAdapterFailure>,
}

impl ChannelAttachmentIntakeRequest {
    pub fn from_bytes(
        session_key: impl Into<String>,
        channel: impl Into<String>,
        filename: Option<String>,
        declared_mime: Option<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            session_key: session_key.into(),
            turn_id: None,
            channel: channel.into(),
            external_message_id: None,
            source_display_name: filename.clone(),
            original_filename: filename,
            declared_mime,
            declared_byte_length: Some(bytes.len() as u64),
            bytes,
            received_at_ms: now_millis(),
        }
    }
}

pub fn normalize_channel_attachment_data_url(
    session_key: impl Into<String>,
    channel: impl Into<String>,
    source_display_name: Option<String>,
    original_filename: Option<String>,
    data_url: &str,
    max_bytes_per_file: u64,
) -> ChannelAttachmentAdapterResult {
    match channel_attachment_request_from_data_url(
        session_key,
        channel,
        source_display_name,
        original_filename,
        data_url,
        max_bytes_per_file,
    ) {
        Ok(request) => ChannelAttachmentAdapterResult {
            requests: vec![request],
            failures: Vec::new(),
        },
        Err(failure) => ChannelAttachmentAdapterResult {
            requests: Vec::new(),
            failures: vec![failure],
        },
    }
}

pub fn channel_attachment_request_from_data_url(
    session_key: impl Into<String>,
    channel: impl Into<String>,
    source_display_name: Option<String>,
    original_filename: Option<String>,
    data_url: &str,
    max_bytes_per_file: u64,
) -> Result<ChannelAttachmentIntakeRequest, ChannelAttachmentAdapterFailure> {
    let Some(data_url_body) = data_url.strip_prefix("data:") else {
        return Err(adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::MalformedDataUrl,
            "data url is malformed",
        ));
    };
    let Some((header, payload)) = data_url_body.split_once(',') else {
        return Err(adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::MalformedDataUrl,
            "data url is malformed",
        ));
    };
    let Some((mime_type, encoding)) = header.rsplit_once(';') else {
        return Err(adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::MalformedDataUrl,
            "data url is malformed",
        ));
    };
    if !encoding.eq_ignore_ascii_case("base64") {
        return Err(adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::MalformedDataUrl,
            "data url is malformed",
        ));
    }
    if !is_supported_data_url_mime_type(mime_type) {
        return Err(adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::UnsupportedDataUrlMimeType,
            format!("unsupported data url mime type: {mime_type}"),
        ));
    }
    let bytes = STANDARD.decode(payload).map_err(|_| {
        adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::MalformedDataUrl,
            "data url payload is not valid base64",
        )
    })?;
    if (bytes.len() as u64) > max_bytes_per_file {
        return Err(adapter_failure(
            0,
            AttachmentSourceKind::DataUrl,
            ChannelAttachmentAdapterFailureReason::PayloadTooLargeBeforeStorage,
            format!(
                "data url payload exceeds storage limit before intake: bytes={} limit={}",
                bytes.len(),
                max_bytes_per_file
            ),
        ));
    }

    Ok(ChannelAttachmentIntakeRequest {
        session_key: session_key.into(),
        turn_id: None,
        channel: channel.into(),
        external_message_id: None,
        source_display_name,
        original_filename,
        declared_mime: Some(mime_type.to_owned()),
        declared_byte_length: Some(bytes.len() as u64),
        bytes,
        received_at_ms: now_millis(),
    })
}

fn adapter_failure(
    item_index: usize,
    source_kind: AttachmentSourceKind,
    reason: ChannelAttachmentAdapterFailureReason,
    message: impl Into<String>,
) -> ChannelAttachmentAdapterFailure {
    ChannelAttachmentAdapterFailure {
        item_index,
        diagnostic: ChannelAttachmentAdapterDiagnostic {
            source_kind,
            reason,
            message: message.into(),
        },
    }
}

fn is_supported_data_url_mime_type(mime_type: &str) -> bool {
    matches!(
        mime_type.to_ascii_lowercase().as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "text/plain"
            | "application/json"
            | "application/pdf"
            | "audio/mpeg"
            | "audio/mp3"
            | "audio/wav"
            | "audio/ogg"
            | "audio/mp4"
            | "video/mp4"
            | "video/webm"
            | "video/quicktime"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAttachment {
    pub attachment_id: String,
    pub session_key: String,
    pub channel: String,
    pub source_display_name: Option<String>,
    pub original_filename: Option<String>,
    pub sanitized_filename: String,
    pub media_root_relative_path: String,
    pub declared_mime: Option<String>,
    pub detected_mime: Option<String>,
    pub mime_detection_source: MimeDetectionSource,
    pub mime_mismatch: bool,
    pub byte_length: u64,
    pub sha256: String,
    pub content_family: AttachmentContentFamily,
    pub intake_status: AttachmentIntakeStatus,
    pub diagnostic_reason: Option<String>,
    pub created_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentIntakeItem {
    pub attachment_id: String,
    pub channel: String,
    pub display_name: String,
    pub original_filename: Option<String>,
    pub sanitized_filename: Option<String>,
    pub media_root_relative_path: Option<String>,
    pub declared_mime: Option<String>,
    pub detected_mime: Option<String>,
    pub mime_detection_source: MimeDetectionSource,
    pub mime_mismatch: bool,
    pub byte_length: u64,
    pub sha256: Option<String>,
    pub content_family: AttachmentContentFamily,
    pub intake_status: AttachmentIntakeStatus,
    pub diagnostic_reason: Option<String>,
    pub stored: Option<StoredAttachment>,
}

impl AttachmentIntakeItem {
    pub fn diagnostic_summary(&self) -> AttachmentDiagnosticSummary {
        AttachmentDiagnosticSummary {
            attachment_id: self.attachment_id.clone(),
            display_name: diagnostic_display_name(self),
            media_root_relative_path: diagnostic_relative_path(self),
            byte_length: self.byte_length,
            detected_mime: self.detected_mime.clone(),
            mime_detection_source: self.mime_detection_source,
            mime_mismatch: self.mime_mismatch,
            status: self.intake_status,
            reason: self.diagnostic_reason.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDiagnosticSummary {
    pub attachment_id: String,
    pub display_name: String,
    pub media_root_relative_path: Option<String>,
    pub byte_length: u64,
    pub detected_mime: Option<String>,
    pub mime_detection_source: MimeDetectionSource,
    pub mime_mismatch: bool,
    pub status: AttachmentIntakeStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentIntakeBatch {
    pub items: Vec<AttachmentIntakeItem>,
}

impl AttachmentIntakeBatch {
    pub fn stored_relative_paths(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|item| item.media_root_relative_path.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentIntakeStatus {
    Stored,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentHandoffStatus {
    Pending,
    IncludedNative,
    IncludedText,
    Truncated,
    Unsupported,
    ExtractionFailed,
    Deferred,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentContentFamily {
    Image,
    Text,
    Pdf,
    Office,
    Audio,
    Video,
    UnsupportedBinary,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MimeDetectionSource {
    Magic,
    Extension,
    Declared,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MimeDetectionMetadata {
    pub declared_mime: Option<String>,
    pub detected_mime: Option<String>,
    pub detection_source: MimeDetectionSource,
    pub mismatch: bool,
    pub content_family: AttachmentContentFamily,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentLimitPolicy {
    pub max_attachments_per_message: usize,
    pub max_bytes_per_file: u64,
    pub max_bytes_per_turn: u64,
}

impl Default for AttachmentLimitPolicy {
    fn default() -> Self {
        Self {
            max_attachments_per_message: DEFAULT_MAX_ATTACHMENTS_PER_MESSAGE,
            max_bytes_per_file: DEFAULT_MAX_BYTES_PER_FILE,
            max_bytes_per_turn: DEFAULT_MAX_BYTES_PER_TURN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentIntakeError {
    MediaRoot(String),
}

impl std::fmt::Display for AttachmentIntakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MediaRoot(error) => write!(formatter, "attachment media root failed: {error}"),
        }
    }
}

impl std::error::Error for AttachmentIntakeError {}

#[derive(Debug, Clone)]
pub struct AttachmentIntakeService {
    media_root: PathBuf,
    policy: AttachmentLimitPolicy,
}

impl AttachmentIntakeService {
    pub fn new(media_root: impl Into<PathBuf>, policy: AttachmentLimitPolicy) -> Self {
        Self {
            media_root: media_root.into(),
            policy,
        }
    }

    pub fn media_root(&self) -> &Path {
        &self.media_root
    }

    pub fn intake(
        &self,
        requests: Vec<ChannelAttachmentIntakeRequest>,
    ) -> Result<AttachmentIntakeBatch, AttachmentIntakeError> {
        prepare_media_root(&self.media_root)?;
        let media_root = canonicalize_existing(&self.media_root)?;
        let mut total_bytes = 0_u64;
        let mut items = Vec::with_capacity(requests.len());

        for (index, request) in requests.into_iter().enumerate() {
            let attachment_id = attachment_id(index);
            if index >= self.policy.max_attachments_per_message {
                items.push(skipped_item(
                    request,
                    attachment_id,
                    "attachment_count_exceeded",
                ));
                continue;
            }
            let byte_length = request.bytes.len() as u64;
            let accounted_byte_length = request
                .declared_byte_length
                .unwrap_or(byte_length)
                .max(byte_length);
            if accounted_byte_length > self.policy.max_bytes_per_file {
                items.push(blocked_item(
                    request,
                    attachment_id,
                    accounted_byte_length,
                    "file_size_exceeded",
                ));
                continue;
            }
            if total_bytes.saturating_add(accounted_byte_length) > self.policy.max_bytes_per_turn {
                items.push(skipped_item_with_length(
                    request,
                    attachment_id,
                    accounted_byte_length,
                    "turn_byte_limit_exceeded",
                ));
                continue;
            }
            total_bytes += accounted_byte_length;
            items.push(self.store_one(&media_root, request, attachment_id));
        }
        Ok(AttachmentIntakeBatch { items })
    }

    fn store_one(
        &self,
        media_root: &Path,
        request: ChannelAttachmentIntakeRequest,
        attachment_id: String,
    ) -> AttachmentIntakeItem {
        let byte_length = request.bytes.len() as u64;
        let sanitized_channel = sanitize_storage_component(&request.channel, "channel");
        let sanitized_filename = sanitize_attachment_filename(
            request
                .original_filename
                .as_deref()
                .or(request.source_display_name.as_deref()),
        );
        let mime = detect_attachment_mime(
            &request.bytes,
            request.declared_mime.as_deref(),
            Some(&sanitized_filename),
        );
        let sha256 = sha256_hex(&request.bytes);
        let relative = PathBuf::from("attachments")
            .join(&sanitized_channel)
            .join(format!("{attachment_id}-{sanitized_filename}"));
        let target = media_root.join(&relative);
        let display_name = display_name(&request, &sanitized_filename);

        if let Err(error) = write_attachment(media_root, &target, &request.bytes) {
            return AttachmentIntakeItem {
                attachment_id,
                channel: request.channel,
                display_name,
                original_filename: request.original_filename,
                sanitized_filename: Some(sanitized_filename),
                media_root_relative_path: None,
                declared_mime: request.declared_mime,
                detected_mime: mime.detected_mime,
                mime_detection_source: mime.detection_source,
                mime_mismatch: mime.mismatch,
                byte_length,
                sha256: None,
                content_family: mime.content_family,
                intake_status: AttachmentIntakeStatus::Blocked,
                diagnostic_reason: Some(error),
                stored: None,
            };
        }

        let relative_text = path_to_forward_slash(&relative);
        let diagnostic_reason = if mime.mismatch {
            Some("mime_mismatch".to_owned())
        } else if mime.detection_source == MimeDetectionSource::Unknown {
            Some("mime_unknown".to_owned())
        } else {
            None
        };
        let stored = StoredAttachment {
            attachment_id: attachment_id.clone(),
            session_key: request.session_key,
            channel: request.channel.clone(),
            source_display_name: request.source_display_name,
            original_filename: request.original_filename.clone(),
            sanitized_filename: sanitized_filename.clone(),
            media_root_relative_path: relative_text.clone(),
            declared_mime: request.declared_mime.clone(),
            detected_mime: mime.detected_mime.clone(),
            mime_detection_source: mime.detection_source,
            mime_mismatch: mime.mismatch,
            byte_length,
            sha256: sha256.clone(),
            content_family: mime.content_family,
            intake_status: AttachmentIntakeStatus::Stored,
            diagnostic_reason,
            created_at_ms: now_millis(),
        };
        AttachmentIntakeItem {
            attachment_id,
            channel: request.channel,
            display_name,
            original_filename: request.original_filename,
            sanitized_filename: Some(sanitized_filename),
            media_root_relative_path: Some(relative_text),
            declared_mime: request.declared_mime,
            detected_mime: mime.detected_mime,
            mime_detection_source: mime.detection_source,
            mime_mismatch: mime.mismatch,
            byte_length,
            sha256: Some(sha256),
            content_family: mime.content_family,
            intake_status: AttachmentIntakeStatus::Stored,
            diagnostic_reason: stored.diagnostic_reason.clone(),
            stored: Some(stored),
        }
    }
}

pub fn detect_attachment_mime(
    bytes: &[u8],
    declared_mime: Option<&str>,
    filename: Option<&str>,
) -> MimeDetectionMetadata {
    let magic = detect_magic_mime(bytes);
    let extension = filename.and_then(mime_from_extension);
    let declared = declared_mime
        .map(normalize_mime)
        .filter(|mime| !mime.is_empty());
    let extension_overrides_generic_magic =
        magic.zip(extension).is_some_and(|(magic, extension)| {
            (magic == "application/zip"
                && content_family_for_mime(extension) == AttachmentContentFamily::Office)
                || (magic == "text/plain" && extension == "application/json")
                || (magic == "text/plain"
                    && content_family_for_mime(extension) == AttachmentContentFamily::Audio)
        });
    let (detected_mime, detection_source) = if extension_overrides_generic_magic {
        (extension.map(str::to_owned), MimeDetectionSource::Extension)
    } else if let Some(mime) = magic {
        (Some(mime.to_owned()), MimeDetectionSource::Magic)
    } else if let Some(mime) = extension {
        (Some(mime.to_owned()), MimeDetectionSource::Extension)
    } else if let Some(mime) = declared.clone() {
        (Some(mime), MimeDetectionSource::Declared)
    } else {
        (None, MimeDetectionSource::Unknown)
    };
    let mismatch = declared
        .as_ref()
        .zip(detected_mime.as_ref())
        .is_some_and(|(declared, detected)| declared != detected);
    let content_family = detected_mime
        .as_deref()
        .map(content_family_for_mime)
        .unwrap_or(AttachmentContentFamily::Unknown);
    MimeDetectionMetadata {
        declared_mime: declared,
        detected_mime,
        detection_source,
        mismatch,
        content_family,
    }
}

pub fn sanitize_attachment_filename(name: Option<&str>) -> String {
    let raw = name.unwrap_or("upload.bin").trim();
    let sanitized = safe_filename(raw)
        .chars()
        .map(|character| {
            if character.is_control() || character == '/' || character == '\\' || character == '%' {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let sanitized = sanitized
        .trim_matches(|character| character == '.' || character == ' ')
        .to_owned();
    let mut sanitized = if sanitized.is_empty()
        || sanitized == "."
        || sanitized == ".."
        || is_reserved_device_name(&sanitized)
    {
        "upload.bin".to_owned()
    } else {
        sanitized
    };
    if sanitized.chars().count() > 120 {
        sanitized = sanitized.chars().take(120).collect();
    }
    sanitized
}

fn prepare_media_root(media_root: &Path) -> Result<(), AttachmentIntakeError> {
    if let Ok(metadata) = fs::symlink_metadata(media_root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(AttachmentIntakeError::MediaRoot(
                "media root is not an allowed directory".to_owned(),
            ));
        }
    }
    fs::create_dir_all(media_root)
        .map_err(|error| AttachmentIntakeError::MediaRoot(error.to_string()))?;
    Ok(())
}

fn write_attachment(media_root: &Path, target: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "attachment_target_has_no_parent".to_owned())?;
    let filename = target
        .file_name()
        .ok_or_else(|| "attachment_target_has_no_filename".to_owned())?;
    reject_existing_symlink_components(parent).map_err(|_| "symlink_path_component".to_owned())?;
    fs::create_dir_all(parent).map_err(|_| "media_directory_unavailable".to_owned())?;
    reject_existing_symlink_components(parent).map_err(|_| "symlink_path_component".to_owned())?;
    ensure_child_path(media_root, parent).map_err(|_| "media_path_escape".to_owned())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "media_directory_unavailable".to_owned())?;
    ensure_child_path(media_root, &canonical_parent).map_err(|_| "media_path_escape".to_owned())?;
    let target = canonical_parent.join(filename);
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() {
            return Err("symlink_leaf".to_owned());
        }
        return Err("attachment_target_exists".to_owned());
    }
    let mut target_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                "attachment_target_exists".to_owned()
            } else {
                "attachment_write_failed".to_owned()
            }
        })?;
    if target_file.write_all(bytes).is_err() {
        let _ = fs::remove_file(&target);
        return Err("attachment_write_failed".to_owned());
    }
    drop(target_file);
    ensure_child_path(media_root, &target).map_err(|_| "media_path_escape".to_owned())?;
    Ok(())
}

fn ensure_child_path(media_root: &Path, path: &Path) -> io::Result<()> {
    let canonical_root = media_root.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    if canonical_path.starts_with(canonical_root) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "attachment path escapes media root",
        ))
    }
}

fn reject_existing_symlink_components(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "parent traversal is not allowed",
                ));
            }
            Component::Normal(value) => current.push(value),
        }
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "symlink components are not allowed",
                ));
            }
        }
    }
    Ok(())
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf, AttachmentIntakeError> {
    path.canonicalize()
        .map_err(|error| AttachmentIntakeError::MediaRoot(error.to_string()))
}

fn skipped_item(
    request: ChannelAttachmentIntakeRequest,
    attachment_id: String,
    reason: &str,
) -> AttachmentIntakeItem {
    let byte_length = request
        .declared_byte_length
        .unwrap_or(request.bytes.len() as u64);
    skipped_item_with_length(request, attachment_id, byte_length, reason)
}

fn skipped_item_with_length(
    request: ChannelAttachmentIntakeRequest,
    attachment_id: String,
    byte_length: u64,
    reason: &str,
) -> AttachmentIntakeItem {
    let sanitized_filename = sanitize_attachment_filename(
        request
            .original_filename
            .as_deref()
            .or(request.source_display_name.as_deref()),
    );
    let display_name = display_name(&request, &sanitized_filename);
    AttachmentIntakeItem {
        attachment_id,
        channel: request.channel,
        display_name,
        original_filename: request.original_filename,
        sanitized_filename: Some(sanitized_filename),
        media_root_relative_path: None,
        declared_mime: request.declared_mime,
        detected_mime: None,
        mime_detection_source: MimeDetectionSource::Unknown,
        mime_mismatch: false,
        byte_length,
        sha256: None,
        content_family: AttachmentContentFamily::Unknown,
        intake_status: AttachmentIntakeStatus::Skipped,
        diagnostic_reason: Some(reason.to_owned()),
        stored: None,
    }
}

fn blocked_item(
    request: ChannelAttachmentIntakeRequest,
    attachment_id: String,
    byte_length: u64,
    reason: &str,
) -> AttachmentIntakeItem {
    let sanitized_filename = sanitize_attachment_filename(
        request
            .original_filename
            .as_deref()
            .or(request.source_display_name.as_deref()),
    );
    let display_name = display_name(&request, &sanitized_filename);
    AttachmentIntakeItem {
        attachment_id,
        channel: request.channel,
        display_name,
        original_filename: request.original_filename,
        sanitized_filename: Some(sanitized_filename),
        media_root_relative_path: None,
        declared_mime: request.declared_mime,
        detected_mime: None,
        mime_detection_source: MimeDetectionSource::Unknown,
        mime_mismatch: false,
        byte_length,
        sha256: None,
        content_family: AttachmentContentFamily::Unknown,
        intake_status: AttachmentIntakeStatus::Blocked,
        diagnostic_reason: Some(reason.to_owned()),
        stored: None,
    }
}

fn display_name(request: &ChannelAttachmentIntakeRequest, fallback: &str) -> String {
    request
        .source_display_name
        .as_deref()
        .or(request.original_filename.as_deref())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn diagnostic_display_name(item: &AttachmentIntakeItem) -> String {
    let source = item
        .original_filename
        .as_deref()
        .or(Some(item.display_name.as_str()))
        .unwrap_or_default();
    let without_query = source.split(['?', '#']).next().unwrap_or_default();
    let normalized = without_query.replace('\\', "/");
    let basename = normalized
        .rsplit('/')
        .find(|part| !part.trim().is_empty())
        .unwrap_or_default();
    let sanitized = sanitize_attachment_filename(Some(basename));
    if sanitized == "upload.bin" {
        item.attachment_id.clone()
    } else {
        redact_string(&sanitized)
    }
}

fn diagnostic_relative_path(item: &AttachmentIntakeItem) -> Option<String> {
    let relative = item.media_root_relative_path.as_deref()?;
    let mut parts = relative.split('/').collect::<Vec<_>>();
    if parts.is_empty() {
        return Some(item.attachment_id.clone());
    }
    let last = parts.len() - 1;
    parts[last] = item.attachment_id.as_str();
    Some(parts.join("/"))
}

fn sanitize_storage_component(value: &str, fallback: &str) -> String {
    let sanitized = sanitize_attachment_filename(Some(value));
    if sanitized == "upload.bin" {
        fallback.to_owned()
    } else {
        sanitized
    }
}

fn is_reserved_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn detect_magic_mime(bytes: &[u8]) -> Option<&'static str> {
    if let Some(mime) = detect_image_mime(bytes) {
        return Some(mime);
    }
    if bytes.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else if bytes.starts_with(b"PK\x03\x04") {
        Some("application/zip")
    } else if bytes.starts_with(b"ID3") || bytes.starts_with(&[0xff, 0xfb]) {
        Some("audio/mpeg")
    } else if bytes.starts_with(b"OggS") {
        Some("audio/ogg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        Some("audio/wav")
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Some(mp4_family_mime(bytes))
    } else if std::str::from_utf8(bytes).is_ok() {
        Some("text/plain")
    } else {
        None
    }
}

fn mp4_family_mime(bytes: &[u8]) -> &'static str {
    let brands_end = bytes.len().min(32);
    let major_brand = bytes.get(8..12).unwrap_or_default();
    let compatible_brands = bytes.get(16..brands_end).unwrap_or_default();
    if is_audio_mp4_brand(major_brand) || compatible_brands.chunks(4).any(is_audio_mp4_brand) {
        "audio/mp4"
    } else {
        "video/mp4"
    }
}

fn is_audio_mp4_brand(brand: &[u8]) -> bool {
    matches!(brand, b"M4A " | b"M4B " | b"M4P ")
}

fn mime_from_extension(filename: &str) -> Option<&'static str> {
    match Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("txt") | Some("md") | Some("csv") | Some("log") => Some("text/plain"),
        Some("json") => Some("application/json"),
        Some("pdf") => Some("application/pdf"),
        Some("docx") => {
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        }
        Some("xlsx") => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        Some("pptx") => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
        }
        Some("mp3") => Some("audio/mpeg"),
        Some("wav") => Some("audio/wav"),
        Some("ogg") | Some("opus") => Some("audio/ogg"),
        Some("m4a") => Some("audio/mp4"),
        Some("mp4") | Some("m4v") => Some("video/mp4"),
        Some("mov") => Some("video/quicktime"),
        Some("webm") => Some("video/webm"),
        _ => None,
    }
}

fn content_family_for_mime(mime: &str) -> AttachmentContentFamily {
    if mime.starts_with("image/") {
        AttachmentContentFamily::Image
    } else if mime.starts_with("text/") || matches!(mime, "application/json") {
        AttachmentContentFamily::Text
    } else if mime == "application/pdf" {
        AttachmentContentFamily::Pdf
    } else if mime.contains("officedocument") {
        AttachmentContentFamily::Office
    } else if mime.starts_with("audio/") {
        AttachmentContentFamily::Audio
    } else if mime.starts_with("video/") {
        AttachmentContentFamily::Video
    } else if mime == "application/zip" || mime == "application/octet-stream" {
        AttachmentContentFamily::UnsupportedBinary
    } else {
        AttachmentContentFamily::Unknown
    }
}

fn normalize_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn path_to_forward_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn attachment_id(index: usize) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("att-{nanos:032x}-{:x}-{index}", std::process::id())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_attachment_with_safe_relative_path_and_digest(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("store");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let request = ChannelAttachmentIntakeRequest::from_bytes(
            "session-a",
            "api",
            Some("../CON.txt".to_owned()),
            Some("text/plain".to_owned()),
            b"hello".to_vec(),
        );

        let batch = service.intake(vec![request])?;

        assert_eq!(batch.items.len(), 1);
        let item = &batch.items[0];
        assert_eq!(item.intake_status, AttachmentIntakeStatus::Stored);
        assert_eq!(item.sanitized_filename.as_deref(), Some("_CON.txt"));
        assert_eq!(item.content_family, AttachmentContentFamily::Text);
        assert_eq!(item.byte_length, 5);
        assert_eq!(item.sha256.as_deref().map(str::len), Some(64));
        let relative = item
            .media_root_relative_path
            .as_deref()
            .expect("relative path");
        assert!(relative.starts_with("attachments/api/att-"));
        assert!(!relative.starts_with('/'));
        assert!(!relative.contains(".."));
        assert_eq!(fs::read(media_root.join(relative))?, b"hello");
        Ok(())
    }

    #[test]
    fn sanitizes_percent_encoded_traversal_markers() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("encoded-traversal");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());

        let batch = service.intake(vec![request("..%2fsecret.txt", b"ok")])?;

        let item = &batch.items[0];
        assert_eq!(item.intake_status, AttachmentIntakeStatus::Stored);
        assert_eq!(item.sanitized_filename.as_deref(), Some("_2fsecret.txt"));
        let relative = item
            .media_root_relative_path
            .as_deref()
            .expect("relative path");
        assert!(!relative.contains(".."));
        assert!(!relative.contains("%2f"));
        Ok(())
    }

    #[test]
    fn preserves_limit_failures_without_silent_drop() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("limits");
        let service = AttachmentIntakeService::new(
            &media_root,
            AttachmentLimitPolicy {
                max_attachments_per_message: 2,
                max_bytes_per_file: 3,
                max_bytes_per_turn: 5,
            },
        );
        let batch = service.intake(vec![
            request("a.txt", b"ab"),
            request("b.txt", b"abcd"),
            request("c.txt", b"c"),
        ])?;

        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.items[0].intake_status, AttachmentIntakeStatus::Stored);
        assert_eq!(
            batch.items[1].intake_status,
            AttachmentIntakeStatus::Blocked
        );
        assert_eq!(
            batch.items[1].diagnostic_reason.as_deref(),
            Some("file_size_exceeded")
        );
        assert_eq!(
            batch.items[2].intake_status,
            AttachmentIntakeStatus::Skipped
        );
        assert_eq!(
            batch.items[2].diagnostic_reason.as_deref(),
            Some("attachment_count_exceeded")
        );
        Ok(())
    }

    #[test]
    fn records_mime_mismatch_and_content_family() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("mime");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let batch = service.intake(vec![ChannelAttachmentIntakeRequest::from_bytes(
            "session-a",
            "api",
            Some("image.txt".to_owned()),
            Some("text/plain".to_owned()),
            png,
        )])?;

        let item = &batch.items[0];
        assert_eq!(item.detected_mime.as_deref(), Some("image/png"));
        assert_eq!(item.mime_detection_source, MimeDetectionSource::Magic);
        assert!(item.mime_mismatch);
        assert_eq!(item.content_family, AttachmentContentFamily::Image);
        assert_eq!(item.diagnostic_reason.as_deref(), Some("mime_mismatch"));
        let stored = item.stored.as_ref().expect("stored attachment");
        assert_eq!(stored.mime_detection_source, MimeDetectionSource::Magic);
        assert!(stored.mime_mismatch);
        let summary = item.diagnostic_summary();
        assert_eq!(summary.mime_detection_source, MimeDetectionSource::Magic);
        assert!(summary.mime_mismatch);
        Ok(())
    }

    #[test]
    fn extension_overrides_generic_magic_for_office_and_json(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("mime-precedence");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let batch = service.intake(vec![
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("report.docx".to_owned()),
                None,
                b"PK\x03\x04rest".to_vec(),
            ),
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("data.json".to_owned()),
                None,
                br#"{"ok":true}"#.to_vec(),
            ),
        ])?;

        let office = &batch.items[0];
        assert_eq!(
            office.detected_mime.as_deref(),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        );
        assert_eq!(office.mime_detection_source, MimeDetectionSource::Extension);
        assert_eq!(office.content_family, AttachmentContentFamily::Office);

        let json = &batch.items[1];
        assert_eq!(json.detected_mime.as_deref(), Some("application/json"));
        assert_eq!(json.mime_detection_source, MimeDetectionSource::Extension);
        assert_eq!(json.content_family, AttachmentContentFamily::Text);
        Ok(())
    }

    #[test]
    fn detects_audio_content_family_from_magic_extension_and_declared_mime(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("audio-family");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let batch = service.intake(vec![
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("magic.bin".to_owned()),
                None,
                b"ID3".to_vec(),
            ),
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("voice.m4a".to_owned()),
                None,
                b"not-magic".to_vec(),
            ),
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("declared.bin".to_owned()),
                Some("audio/ogg".to_owned()),
                vec![0xff, 0x00, 0x01],
            ),
        ])?;

        assert_eq!(batch.items[0].detected_mime.as_deref(), Some("audio/mpeg"));
        assert_eq!(
            batch.items[0].content_family,
            AttachmentContentFamily::Audio
        );
        assert_eq!(batch.items[1].detected_mime.as_deref(), Some("audio/mp4"));
        assert_eq!(
            batch.items[1].content_family,
            AttachmentContentFamily::Audio
        );
        assert_eq!(batch.items[2].detected_mime.as_deref(), Some("audio/ogg"));
        assert_eq!(
            batch.items[2].content_family,
            AttachmentContentFamily::Audio
        );
        Ok(())
    }

    #[test]
    fn detects_mp4_family_audio_and_video_brands() {
        let m4a = detect_attachment_mime(b"\0\0\0\x18ftypM4A \0\0\0\0M4A ", None, None);
        assert_eq!(m4a.detected_mime.as_deref(), Some("audio/mp4"));
        assert_eq!(m4a.content_family, AttachmentContentFamily::Audio);

        let video = detect_attachment_mime(b"\0\0\0\x18ftypisom\0\0\0\0mp42", None, None);
        assert_eq!(video.detected_mime.as_deref(), Some("video/mp4"));
        assert_eq!(video.content_family, AttachmentContentFamily::Video);
    }

    #[test]
    fn blocks_declared_size_over_file_cap_before_write() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("declared-size");
        let service = AttachmentIntakeService::new(
            &media_root,
            AttachmentLimitPolicy {
                max_attachments_per_message: 10,
                max_bytes_per_file: 3,
                max_bytes_per_turn: 10,
            },
        );
        let mut request = request("tiny.txt", b"ok");
        request.declared_byte_length = Some(4);

        let batch = service.intake(vec![request])?;

        assert_eq!(
            batch.items[0].intake_status,
            AttachmentIntakeStatus::Blocked
        );
        assert_eq!(batch.items[0].byte_length, 4);
        assert_eq!(
            batch.items[0].diagnostic_reason.as_deref(),
            Some("file_size_exceeded")
        );
        assert!(batch.items[0].media_root_relative_path.is_none());
        Ok(())
    }

    #[test]
    fn turn_byte_cap_uses_actual_bytes_when_declared_size_is_lower(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("turn-actual-size");
        let service = AttachmentIntakeService::new(
            &media_root,
            AttachmentLimitPolicy {
                max_attachments_per_message: 10,
                max_bytes_per_file: 10,
                max_bytes_per_turn: 5,
            },
        );
        let mut first = request("one.txt", b"abc");
        first.declared_byte_length = Some(1);
        let mut second = request("two.txt", b"def");
        second.declared_byte_length = Some(1);

        let batch = service.intake(vec![first, second])?;

        assert_eq!(batch.items[0].intake_status, AttachmentIntakeStatus::Stored);
        assert_eq!(
            batch.items[1].intake_status,
            AttachmentIntakeStatus::Skipped
        );
        assert_eq!(batch.items[1].byte_length, 3);
        assert_eq!(
            batch.items[1].diagnostic_reason.as_deref(),
            Some("turn_byte_limit_exceeded")
        );
        Ok(())
    }

    #[test]
    fn records_unknown_mime_without_marking_failure() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("unknown-mime");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let batch = service.intake(vec![ChannelAttachmentIntakeRequest::from_bytes(
            "session-a",
            "api",
            Some("blob".to_owned()),
            None,
            vec![0, 159, 146, 150],
        )])?;

        let item = &batch.items[0];
        assert_eq!(item.intake_status, AttachmentIntakeStatus::Stored);
        assert_eq!(item.detected_mime, None);
        assert_eq!(item.content_family, AttachmentContentFamily::Unknown);
        assert_eq!(item.diagnostic_reason.as_deref(), Some("mime_unknown"));
        Ok(())
    }

    #[test]
    fn normalizes_valid_data_url_into_single_request() {
        let result = normalize_channel_attachment_data_url(
            "session-a",
            "api",
            Some("inline.png".to_owned()),
            Some("inline.png".to_owned()),
            "data:image/png;base64,aGk=",
            10,
        );

        assert_eq!(result.requests.len(), 1);
        assert!(result.failures.is_empty());
        let request = &result.requests[0];
        assert_eq!(request.channel, "api");
        assert_eq!(request.source_display_name.as_deref(), Some("inline.png"));
        assert_eq!(request.original_filename.as_deref(), Some("inline.png"));
        assert_eq!(request.declared_mime.as_deref(), Some("image/png"));
        assert_eq!(request.bytes, b"hi");
        assert_eq!(request.declared_byte_length, Some(2));
    }

    #[test]
    fn normalizes_audio_data_url_into_single_request() {
        let result = normalize_channel_attachment_data_url(
            "session-a",
            "api",
            Some("voice.mp3".to_owned()),
            Some("voice.mp3".to_owned()),
            "data:audio/mpeg;base64,SUQz",
            10,
        );

        assert_eq!(result.requests.len(), 1);
        assert!(result.failures.is_empty());
        let request = &result.requests[0];
        assert_eq!(request.declared_mime.as_deref(), Some("audio/mpeg"));
        assert_eq!(request.bytes, b"ID3");
        assert_eq!(request.declared_byte_length, Some(3));
    }

    #[test]
    fn normalizes_video_data_url_into_single_request() {
        let result = normalize_channel_attachment_data_url(
            "session-a",
            "api",
            Some("clip.mp4".to_owned()),
            Some("clip.mp4".to_owned()),
            "data:video/mp4;base64,AAAAHGZ0eXBpc29tAAAAAG1wNDI=",
            64,
        );

        assert_eq!(result.requests.len(), 1);
        assert!(result.failures.is_empty());
        let request = &result.requests[0];
        assert_eq!(request.declared_mime.as_deref(), Some("video/mp4"));
        assert_eq!(request.bytes, b"\0\0\0\x1cftypisom\0\0\0\0mp42");
        assert_eq!(request.declared_byte_length, Some(20));
    }

    #[test]
    fn detects_video_content_family_from_extension_and_declared_mime(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("video-family");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let batch = service.intake(vec![
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("clip.webm".to_owned()),
                None,
                vec![0xff, 0x00, 0x01],
            ),
            ChannelAttachmentIntakeRequest::from_bytes(
                "session-a",
                "api",
                Some("declared.bin".to_owned()),
                Some("video/quicktime".to_owned()),
                vec![0xff, 0x00, 0x01],
            ),
        ])?;

        assert_eq!(batch.items[0].detected_mime.as_deref(), Some("video/webm"));
        assert_eq!(
            batch.items[0].content_family,
            AttachmentContentFamily::Video
        );
        assert_eq!(
            batch.items[1].detected_mime.as_deref(),
            Some("video/quicktime")
        );
        assert_eq!(
            batch.items[1].content_family,
            AttachmentContentFamily::Video
        );
        Ok(())
    }

    #[test]
    fn reports_malformed_data_url_as_item_failure_without_leaking_body() {
        let payload = "aGk=";
        let result = normalize_channel_attachment_data_url(
            "session-a",
            "api",
            Some("inline.txt".to_owned()),
            Some("inline.txt".to_owned()),
            &format!("data:text/plain;base64,{payload}%%%"),
            10,
        );

        assert!(result.requests.is_empty());
        assert_eq!(result.failures.len(), 1);
        let failure = &result.failures[0];
        assert_eq!(failure.item_index, 0);
        assert_eq!(
            failure.diagnostic.source_kind,
            AttachmentSourceKind::DataUrl
        );
        assert_eq!(
            failure.diagnostic.reason,
            ChannelAttachmentAdapterFailureReason::MalformedDataUrl
        );
        assert!(!failure.diagnostic.message.contains(payload));
        assert!(!format!("{:?}", failure).contains(payload));
    }

    #[test]
    fn reports_unsupported_and_oversized_data_urls() {
        let unsupported = channel_attachment_request_from_data_url(
            "session-a",
            "api",
            None,
            None,
            "data:application/x-sh;base64,aGk=",
            10,
        )
        .expect_err("unsupported type");
        assert_eq!(
            unsupported.diagnostic.reason,
            ChannelAttachmentAdapterFailureReason::UnsupportedDataUrlMimeType
        );
        assert!(unsupported.diagnostic.message.contains("application/x-sh"));

        let oversized = channel_attachment_request_from_data_url(
            "session-a",
            "api",
            None,
            None,
            "data:text/plain;base64,aGVsbG8=",
            3,
        )
        .expect_err("oversized payload");
        assert_eq!(
            oversized.diagnostic.reason,
            ChannelAttachmentAdapterFailureReason::PayloadTooLargeBeforeStorage
        );
        assert!(oversized.diagnostic.message.contains("bytes=5"));
        assert!(oversized.diagnostic.message.contains("limit=3"));
    }

    #[test]
    fn serializes_source_kinds_with_snake_case_names() {
        let cases = [
            (AttachmentSourceKind::PlatformDownload, "platform_download"),
            (AttachmentSourceKind::InlineBytes, "inline_bytes"),
            (AttachmentSourceKind::DataUrl, "data_url"),
            (AttachmentSourceKind::MimePart, "mime_part"),
            (
                AttachmentSourceKind::BridgeMediaHandle,
                "bridge_media_handle",
            ),
            (AttachmentSourceKind::LocalMultipart, "local_multipart"),
        ];

        for (kind, expected) in cases {
            let serialized = serde_json::to_string(&kind).expect("serialize source kind");
            assert_eq!(serialized, format!("\"{expected}\""));
        }
    }

    #[test]
    fn diagnostic_summary_keeps_only_relative_path() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("diagnostic");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let batch = service.intake(vec![request("/Users/alice/secrets/report.txt", b"ok")])?;
        let summary = batch.items[0].diagnostic_summary();

        let relative = summary.media_root_relative_path.expect("relative path");
        assert!(relative.starts_with("attachments/api/"));
        assert!(!relative.contains(media_root.to_string_lossy().as_ref()));
        assert_eq!(summary.display_name, "report.txt");
        assert!(!summary.display_name.contains("alice"));
        Ok(())
    }

    #[test]
    fn diagnostic_summary_redacts_secret_like_filename() {
        let item = AttachmentIntakeItem {
            attachment_id: "att-secret".to_owned(),
            channel: "api".to_owned(),
            display_name: "OPENAI_API_KEY=sk-secret-token.txt".to_owned(),
            original_filename: Some("OPENAI_API_KEY=sk-secret-token.txt".to_owned()),
            sanitized_filename: Some("OPENAI_API_KEY=sk-secret-token.txt".to_owned()),
            media_root_relative_path: Some(
                "attachments/api/att-secret-OPENAI_API_KEY=sk-secret-token.txt".to_owned(),
            ),
            declared_mime: None,
            detected_mime: None,
            mime_detection_source: MimeDetectionSource::Unknown,
            mime_mismatch: false,
            byte_length: 0,
            sha256: None,
            content_family: AttachmentContentFamily::Unknown,
            intake_status: AttachmentIntakeStatus::Blocked,
            diagnostic_reason: Some("file_size_exceeded".to_owned()),
            stored: None,
        };

        let summary = item.diagnostic_summary();

        assert!(summary.display_name.contains(shacs_redaction::REDACTED));
        assert!(!summary.display_name.contains("sk-secret-token"));
        let summary_path = summary.media_root_relative_path.expect("diagnostic path");
        assert_eq!(summary_path, "attachments/api/att-secret");
        assert!(!summary_path.contains("sk-secret-token"));
    }

    #[cfg(unix)]
    #[test]
    fn blocks_existing_symlink_channel_directory() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let media_root = temp_dir("symlink");
        let outside = temp_dir("outside");
        fs::create_dir_all(media_root.join("attachments"))?;
        symlink(&outside, media_root.join("attachments").join("api"))?;
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());
        let batch = service.intake(vec![request("report.txt", b"ok")])?;

        assert_eq!(
            batch.items[0].intake_status,
            AttachmentIntakeStatus::Blocked
        );
        assert_eq!(
            batch.items[0].diagnostic_reason.as_deref(),
            Some("symlink_path_component")
        );
        Ok(())
    }

    #[test]
    fn refuses_to_overwrite_existing_target_file() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("existing-target").canonicalize()?;
        let target = media_root
            .join("attachments")
            .join("api")
            .join("att-existing-report.txt");
        fs::create_dir_all(target.parent().expect("target parent"))?;
        fs::write(&target, b"original")?;

        let error = write_attachment(&media_root, &target, b"replacement").expect_err("blocked");

        assert_eq!(error, "attachment_target_exists");
        assert_eq!(fs::read(&target)?, b"original");
        Ok(())
    }

    #[test]
    fn stores_same_filename_across_rapid_intakes() -> Result<(), Box<dyn std::error::Error>> {
        let media_root = temp_dir("rapid-intakes");
        let service = AttachmentIntakeService::new(&media_root, AttachmentLimitPolicy::default());

        let first = service.intake(vec![request("same.txt", b"one")])?;
        let second = service.intake(vec![request("same.txt", b"two")])?;

        assert_eq!(first.items[0].intake_status, AttachmentIntakeStatus::Stored);
        assert_eq!(
            second.items[0].intake_status,
            AttachmentIntakeStatus::Stored
        );
        assert_ne!(
            first.items[0].media_root_relative_path,
            second.items[0].media_root_relative_path
        );
        Ok(())
    }

    fn request(filename: &str, bytes: &[u8]) -> ChannelAttachmentIntakeRequest {
        ChannelAttachmentIntakeRequest::from_bytes(
            "session-a",
            "api",
            Some(filename.to_owned()),
            Some("text/plain".to_owned()),
            bytes.to_vec(),
        )
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "shacs-utils-attachments-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}
