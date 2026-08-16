mod attachment_dispatch;
mod attachment_routing;
mod video_blocks;
mod video_route;
mod video_types;

pub(crate) use attachment_routing::{
    route_stored_attachment_with_analyzer_invocation, MediaRootRouting,
};
#[cfg(test)]
pub(crate) use attachment_routing::{
    route_stored_attachment_with_analyzers, route_stored_attachment_with_audio_analyzer,
    route_stored_attachment_with_native_image_support,
};
pub use video_types::*;

pub(super) fn handoff_status_name(
    status: shacs_utils::attachments::AttachmentHandoffStatus,
) -> &'static str {
    use shacs_utils::attachments::AttachmentHandoffStatus;
    match status {
        AttachmentHandoffStatus::Pending => "pending",
        AttachmentHandoffStatus::IncludedNative => "included_native",
        AttachmentHandoffStatus::IncludedText => "included_text",
        AttachmentHandoffStatus::Truncated => "truncated",
        AttachmentHandoffStatus::Unsupported => "unsupported",
        AttachmentHandoffStatus::ExtractionFailed => "extraction_failed",
        AttachmentHandoffStatus::Cancelled => "cancelled",
        AttachmentHandoffStatus::TimedOut => "timeout",
        AttachmentHandoffStatus::Deferred => "deferred",
        AttachmentHandoffStatus::Blocked => "blocked",
    }
}
