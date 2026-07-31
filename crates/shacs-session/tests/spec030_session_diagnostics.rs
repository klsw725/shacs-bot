use serde_json::json;
use shacs_session::{build_session_diagnostics_aggregate, SessionUxDiagnostics};
use std::error::Error;
use std::path::PathBuf;

#[test]
fn session_diagnostics_aggregate_uses_opaque_refs_not_absolute_paths() -> Result<(), Box<dyn Error>>
{
    let diagnostics = SessionUxDiagnostics {
        key: "session-with-/Users/spec030/raw-key".to_owned(),
        path: PathBuf::from("/Users/spec030/raw/session.jsonl"),
        exists: true,
        message_count: 3,
        last_consolidated: 1,
        metadata_keys: vec![
            "runtime_diagnostics".to_owned(),
            "provider_payload".to_owned(),
        ],
        recovery_markers: vec!["runtime_diagnostics".to_owned()],
        checkpoint_phase: Some("ready".to_owned()),
        diagnostics_refs: vec![
            "diagnostics:policy-safety".to_owned(),
            "diagnostics:sha256:safe-ref".to_owned(),
        ],
        runtime_workflow: None,
        runtime_execution: None,
        legal_start: 1,
    };

    let aggregate = build_session_diagnostics_aggregate(&diagnostics)?;
    let serialized = serde_json::to_string(&aggregate)?;

    assert!(serialized.contains("session:sha256:"));
    assert!(serialized.contains("diagnostics_ref_count"));
    assert!(!serialized.contains("/Users/spec030"));
    assert!(!serialized.contains("raw-key"));
    assert_eq!(aggregate.message_count, 3);
    assert_eq!(aggregate.diagnostics_ref_count, 2);
    Ok(())
}

#[test]
fn session_diagnostics_aggregate_rejects_raw_diagnostics_refs_before_hashing() {
    let diagnostics = SessionUxDiagnostics {
        key: "session-safe".to_owned(),
        path: PathBuf::from("relative/session.jsonl"),
        exists: true,
        message_count: 0,
        last_consolidated: 0,
        metadata_keys: Vec::new(),
        recovery_markers: Vec::new(),
        checkpoint_phase: None,
        diagnostics_refs: vec![
            "stdout_ref carries raw stdout content".to_owned(),
            "C:\\Users\\spec030\\secret.txt".to_owned(),
        ],
        runtime_workflow: None,
        runtime_execution: None,
        legal_start: 0,
    };

    let result = build_session_diagnostics_aggregate(&diagnostics);

    assert!(result.is_err());
    let error_text = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(!error_text.contains("raw stdout content"));
    assert!(!error_text.contains("C:\\Users"));
    assert_eq!(error_text, "raw diagnostic material rejected");
}

#[test]
fn session_diagnostics_aggregate_direct_serialization_redacts_mutated_raw_fields(
) -> Result<(), Box<dyn Error>> {
    let diagnostics = SessionUxDiagnostics {
        key: "session-safe".to_owned(),
        path: PathBuf::from("relative/session.jsonl"),
        exists: true,
        message_count: 0,
        last_consolidated: 0,
        metadata_keys: Vec::new(),
        recovery_markers: Vec::new(),
        checkpoint_phase: Some("ready".to_owned()),
        diagnostics_refs: vec!["diagnostics:sha256:safe-ref".to_owned()],
        runtime_workflow: None,
        runtime_execution: None,
        legal_start: 0,
    };
    let mut aggregate = build_session_diagnostics_aggregate(&diagnostics)?;
    aggregate.session_ref = "raw stderr content hidden in ref".to_owned();
    aggregate.diagnostics_refs = vec!["C:\\Users\\spec030\\secret.txt".to_owned()];

    let serialized = serde_json::to_string(&aggregate)?;

    assert!(!serialized.contains("raw stderr content"));
    assert!(!serialized.contains("C:\\Users"));
    assert!(serialized.contains("[REDACTED]"));
    Ok(())
}

#[test]
fn session_diagnostics_aggregate_rejects_raw_metadata_names_before_serialization() {
    let diagnostics = SessionUxDiagnostics {
        key: "session-safe".to_owned(),
        path: PathBuf::from("relative/session.jsonl"),
        exists: true,
        message_count: 0,
        last_consolidated: 0,
        metadata_keys: vec!["sk-spec030-raw-token".to_owned()],
        recovery_markers: Vec::new(),
        checkpoint_phase: None,
        diagnostics_refs: Vec::new(),
        runtime_workflow: None,
        runtime_execution: None,
        legal_start: 0,
    };

    let result = build_session_diagnostics_aggregate(&diagnostics);

    assert!(result.is_err());
    let error_text = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(!error_text.contains("sk-spec030-raw-token"));
    assert_eq!(
        json!({"error": error_text})["error"],
        "raw diagnostic material rejected"
    );
}
