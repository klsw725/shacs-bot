use super::*;
use crate::runtime::video_analyzer_spec035::VideoAnalyzerSpec035PublicationStatus;

#[test]
fn unknown_projection_commit_is_deferred_instead_of_extraction_failed() {
    // Given
    let note = StoredAttachmentNote::new(
        "cli",
        Path::new("clip.mp4"),
        None,
        4,
        Some("abc123"),
        "pending",
    );

    // When
    let blocks = publication_status_blocks(
        VideoAnalyzerSpec035PublicationStatus::CommitStatusUnknown,
        note,
    )
    .unwrap_or_default();

    // Then
    let rendered = blocks[0]["text"].as_str().unwrap_or_default();
    assert!(rendered.contains("[attachment:deferred]"));
    assert!(rendered.contains("projection commit status unknown"));
    assert!(!rendered.contains("[attachment:extraction_failed]"));
}
