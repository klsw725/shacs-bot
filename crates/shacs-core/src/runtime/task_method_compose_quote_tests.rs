use std::time::{Duration, SystemTime};

use super::*;
use crate::runtime::task_method_report::{SourceKind, SourceStatus};

const NOTES: &str = "Revenue grew 10%.\n";

fn read_source(path: &std::path::Path) -> ReportSource {
    ReportSource {
        seq: 1,
        call_id: "r1".to_owned(),
        selection_revision: 1,
        version: 1,
        carried: false,
        kind: SourceKind::File,
        locator: "notes.md".to_owned(),
        match_keys: Vec::new(),
        status: SourceStatus::Read,
        content_digest: None,
        file_digest: Some(digest(NOTES.as_bytes())),
        resolved_path: Some(path.display().to_string()),
        requested_path: None,
        read_at: String::new(),
        error: None,
    }
}

#[test]
fn only_a_file_unchanged_since_it_was_read_and_since_the_request_began_is_quoted(
) -> std::io::Result<()> {
    // Given
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("notes.md");
    std::fs::write(&path, NOTES)?;
    let sources = [read_source(&path)];
    let later = Some(SystemTime::now() + Duration::from_secs(60));
    let earlier = Some(SystemTime::now() - Duration::from_secs(60));
    // When / Then
    assert!(unchanged(&sources, 1, later).is_some_and(|file| file.quotes("Revenue grew 10%.")));
    assert!(unchanged(&sources, 2, later).is_none(), "no such source");
    assert!(unchanged(&sources, 1, None).is_none(), "no recorded start");
    // Written after the request began, as the model itself could have done.
    assert!(unchanged(&sources, 1, earlier).is_none());
    std::fs::write(&path, "Revenue grew 10%. The chart is blue.\n")?;
    assert!(
        unchanged(&sources, 1, later).is_none(),
        "the file changed after it was read"
    );
    Ok(())
}
