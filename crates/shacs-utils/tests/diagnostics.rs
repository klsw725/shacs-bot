use serde_json::json;
use shacs_redaction::REDACTED;
use shacs_utils::diagnostics::{
    CrashEvidence, DiagnosticsCorrelation, DiagnosticsKind, DiagnosticsRecord, DiagnosticsSeverity,
    DiagnosticsSnapshot, OperationalLogRecord, RecoveryEvidence, TraceRecord,
};

#[test]
fn diagnostics_redaction_recursively_masks_raw_process_and_provider_fields() {
    let snapshot = DiagnosticsSnapshot {
        generated_at_ms: 1,
        runtime: json!({
            "stdout": "raw stdout content",
            "StdErr": "raw stderr content",
            "process_handle": "process_handle_123",
            "args": ["--token", "plain-arg"],
            "env": {"SAFE_FLAG": "true", "TOKEN": "plain-token"},
            "provider_payload": {"body": "raw provider response"},
            "absolute_path": "/Users/spec030/workspace/file.txt",
            "safe_artifact_ref": "artifact:sha256:abc",
            "diagnostics_state": "redacted",
            "diagnostics_count": 1
        }),
        operational_logs: vec![OperationalLogRecord {
            timestamp_ms: 1,
            severity: DiagnosticsSeverity::Info,
            kind: DiagnosticsKind::Runtime,
            message: "ordinary diagnostic summary mentions stdout as a label".to_owned(),
            correlation: DiagnosticsCorrelation::default(),
            fields: json!({
                "nested": [{
                    "std_out": "misleading stdout content",
                    "stdErr": "misleading stderr content",
                    "processHandle": "misleading process handle",
                    "providerPayload": "misleading provider payload",
                    "argv": ["run", "--unsafe"],
                    "environment": {"HOME": "/Users/spec030"}
                }]
            }),
        }],
        traces: Vec::<TraceRecord>::new(),
        diagnostics: Vec::<DiagnosticsRecord>::new(),
        crash_evidence: Vec::<CrashEvidence>::new(),
        recovery_evidence: Vec::<RecoveryEvidence>::new(),
        provider_progress: vec![json!({"response_payload": "raw provider payload in array"})],
        tool_progress: Vec::new(),
        subagent_progress: Vec::new(),
    };

    let redacted = snapshot.redacted_value();
    let serialized = serde_json::to_string(&redacted).unwrap_or_default();

    for raw_marker in raw_material_markers() {
        assert!(
            !serialized.contains(raw_marker),
            "raw marker leaked: {raw_marker}"
        );
    }
    assert_eq!(redacted["runtime"]["stdout"], REDACTED);
    assert_eq!(redacted["runtime"]["StdErr"], REDACTED);
    assert_eq!(redacted["runtime"]["process_handle"], REDACTED);
    assert_eq!(redacted["runtime"]["args"], REDACTED);
    assert_eq!(redacted["runtime"]["env"], REDACTED);
    assert_eq!(redacted["runtime"]["provider_payload"], REDACTED);
    assert_eq!(redacted["runtime"]["absolute_path"], REDACTED);
    assert_eq!(
        redacted["runtime"]["safe_artifact_ref"],
        "artifact:sha256:abc"
    );
    assert_eq!(redacted["runtime"]["diagnostics_state"], "redacted");
    assert_eq!(redacted["runtime"]["diagnostics_count"], 1);
    assert!(serialized.contains("ordinary diagnostic summary mentions stdout as a label"));
}

#[test]
fn diagnostics_redaction_prioritizes_dangerous_concepts_over_safe_suffixes() {
    let snapshot = DiagnosticsSnapshot {
        generated_at_ms: 1,
        runtime: json!({
            "stdout_ref": "raw stdout content hidden in ref",
            "stderr-state": "raw stderr content hidden in state",
            "processHandleCount": "process handle hidden as count",
            "providerPayloadDigest": "raw provider payload hidden as digest",
            "standard_output": "standard output raw data",
            "note": "raw stdout content under misleading note",
            "win_path": "C:\\Users\\spec030\\secret.txt",
            "unc_path": "\\\\server\\share\\secret.txt",
            "safe_artifact_ref": "artifact:sha256:abcdef",
            "runtime_state": "blocked_external_surface",
            "event_count": 7,
            "bad_ref": "artifact:sha256:abc def",
            "bad_state": "redacted but has spaces",
            "bad_count": "7"
        }),
        operational_logs: Vec::new(),
        traces: Vec::<TraceRecord>::new(),
        diagnostics: Vec::<DiagnosticsRecord>::new(),
        crash_evidence: Vec::<CrashEvidence>::new(),
        recovery_evidence: Vec::<RecoveryEvidence>::new(),
        provider_progress: Vec::new(),
        tool_progress: Vec::new(),
        subagent_progress: Vec::new(),
    };

    let redacted = snapshot.redacted_value();
    let serialized = serde_json::to_string(&redacted).unwrap_or_default();

    for raw_marker in suffix_abuse_markers() {
        assert!(
            !serialized.contains(raw_marker),
            "raw marker leaked: {raw_marker}"
        );
    }
    assert_eq!(redacted["runtime"]["stdout_ref"], REDACTED);
    assert_eq!(redacted["runtime"]["stderr-state"], REDACTED);
    assert_eq!(redacted["runtime"]["processHandleCount"], REDACTED);
    assert_eq!(redacted["runtime"]["providerPayloadDigest"], REDACTED);
    assert_eq!(redacted["runtime"]["standard_output"], REDACTED);
    assert_eq!(redacted["runtime"]["note"], REDACTED);
    assert_eq!(redacted["runtime"]["win_path"], REDACTED);
    assert_eq!(redacted["runtime"]["unc_path"], REDACTED);
    assert_eq!(
        redacted["runtime"]["safe_artifact_ref"],
        "artifact:sha256:abcdef"
    );
    assert_eq!(
        redacted["runtime"]["runtime_state"],
        "blocked_external_surface"
    );
    assert_eq!(redacted["runtime"]["event_count"], 7);
    assert_eq!(redacted["runtime"]["bad_ref"], REDACTED);
    assert_eq!(redacted["runtime"]["bad_state"], REDACTED);
    assert_eq!(redacted["runtime"]["bad_count"], REDACTED);
}

#[test]
fn diagnostics_redaction_rejects_malformed_safe_suffix_shapes() {
    let snapshot = DiagnosticsSnapshot {
        generated_at_ms: 1,
        runtime: json!({
            "object_ref": {"id": "artifact:sha256:abcdef"},
            "array_refs": ["artifact:sha256:abcdef"],
            "object_state": {"state": "ready"},
            "null_ref": null,
            "bool_state": true,
            "float_count": 1.5,
            "nested": {
                "camelRef": {"id": "artifact:sha256:nested"},
                "hyphen-state": ["ready"],
                "pluralRefs": ["artifact:sha256:plural"],
                "eventCount": 3
            },
            "safe_artifact_ref": "artifact:sha256:abcdef",
            "runtime_state": "blocked_external_surface",
            "event_count": 7
        }),
        operational_logs: Vec::new(),
        traces: Vec::<TraceRecord>::new(),
        diagnostics: Vec::<DiagnosticsRecord>::new(),
        crash_evidence: Vec::<CrashEvidence>::new(),
        recovery_evidence: Vec::<RecoveryEvidence>::new(),
        provider_progress: Vec::new(),
        tool_progress: Vec::new(),
        subagent_progress: Vec::new(),
    };

    let redacted = snapshot.redacted_value();

    assert_eq!(redacted["runtime"]["object_ref"], REDACTED);
    assert_eq!(redacted["runtime"]["array_refs"], REDACTED);
    assert_eq!(redacted["runtime"]["object_state"], REDACTED);
    assert_eq!(redacted["runtime"]["null_ref"], REDACTED);
    assert_eq!(redacted["runtime"]["bool_state"], REDACTED);
    assert_eq!(redacted["runtime"]["float_count"], REDACTED);
    assert_eq!(redacted["runtime"]["nested"]["camelRef"], REDACTED);
    assert_eq!(redacted["runtime"]["nested"]["hyphen-state"], REDACTED);
    assert_eq!(redacted["runtime"]["nested"]["pluralRefs"], REDACTED);
    assert_eq!(redacted["runtime"]["nested"]["eventCount"], 3);
    assert_eq!(
        redacted["runtime"]["safe_artifact_ref"],
        "artifact:sha256:abcdef"
    );
    assert_eq!(
        redacted["runtime"]["runtime_state"],
        "blocked_external_surface"
    );
    assert_eq!(redacted["runtime"]["event_count"], 7);
}

fn raw_material_markers() -> [&'static str; 14] {
    [
        "raw stdout content",
        "raw stderr content",
        "process_handle_123",
        "plain-arg",
        "plain-token",
        "raw provider response",
        "/Users/spec030/workspace/file.txt",
        "misleading stdout content",
        "misleading stderr content",
        "misleading process handle",
        "misleading provider payload",
        "--unsafe",
        "/Users/spec030",
        "raw provider payload in array",
    ]
}

fn suffix_abuse_markers() -> [&'static str; 8] {
    [
        "raw stdout content hidden in ref",
        "raw stderr content hidden in state",
        "process handle hidden as count",
        "raw provider payload hidden as digest",
        "standard output raw data",
        "raw stdout content under misleading note",
        "C:\\Users\\spec030\\secret.txt",
        "\\\\server\\share\\secret.txt",
    ]
}
