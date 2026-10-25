use serde_json::json;
use shacs_session::{Session, SessionManager};
use shacs_tui::{
    live_source::{RuntimeProjectionSource, SessionRuntimeSource},
    state::TuiState,
    view::render_lines,
};
use std::{error::Error, fs, path::Path};

const LEFTOVER_RECORD: &str = r#"{"schema_version":1,"kind":"media_capability","state":"extraction_failed","reason":{"code":"extraction_failed","safe_summary":"media extraction failed"},"lineage":{"artifact_ref":"spec034://media/artifact/7b1dfd5bb5d3258aa9d9c556171a55c5b2a8dc847bb042faa489277a53f627ee"},"freshness":"unavailable","disclosure":{"status":"unavailable"},"owner_facts":{"unavailable_reasons":["missing_analyzer_owner_ref"]}}"#;
const CORRUPT_RECORD: &str =
    r#"{"schema_version": 1, "kind": "media_capability", "state": "included""#;

#[test]
fn media_lines_stay_unavailable_with_leftover_records() -> Result<(), Box<dyn Error>> {
    for record in [LEFTOVER_RECORD, CORRUPT_RECORD] {
        // Given
        let workspace = tempfile::tempdir()?;
        let record_path = workspace
            .path()
            .join("media")
            .join("projections")
            .join("current.json");
        fs::create_dir_all(record_path.parent().ok_or("record parent")?)?;
        fs::write(&record_path, record)?;
        save_session_with_leftover_metadata(workspace.path())?;

        // When
        let rendered = render_workspace(workspace.path())?;

        // Then
        assert!(
            rendered.contains("media: state=unavailable reason=unavailable freshness=unavailable")
        );
        assert!(rendered.contains("media lineage: unavailable"));
        assert!(rendered.contains("media disclosure: unavailable"));
        assert!(!rendered.contains("media reason:"));
        assert!(!rendered.contains("extraction_failed"));
        assert!(!rendered.contains("state=included"));
        assert_eq!(fs::read_to_string(&record_path)?, record);
    }
    Ok(())
}

fn render_workspace(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let snapshot =
        SessionRuntimeSource::with_config(Some(workspace.join("config.json")), workspace).load()?;
    let state = TuiState::from_snapshot(snapshot, None);
    Ok(render_lines(&state).join("\n"))
}

fn save_session_with_leftover_metadata(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = Session::new("cli:direct");
    session
        .metadata
        .insert("media_capability".to_owned(), json!({"state": "included"}));
    session.add_message("user", "미디어 상태 확인", serde_json::Map::new());
    manager.save(&session)?;
    Ok(())
}
