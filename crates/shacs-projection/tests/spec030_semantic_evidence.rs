use shacs_projection::{
    parse_spec030_manual_qa, parse_spec030_surface_assertions, validate_spec030_cleanup_receipt,
    Spec030ReleaseArtifactError,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock follows epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("spec030-semantic-{label}-{nonce}"))
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

#[test]
fn manual_qa_rejects_placeholder_source_mismatch_and_missing_non_guarantees(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let path = temp_path("manual");
    let digest = format!("sha256:{}", "1".repeat(64));
    write_json(
        &path,
        &serde_json::json!({
            "schema": "spec030.manual_qa.v1",
            "source_digest": digest,
            "observed_commands": [
                {"id":"cli-json","status":"passed"},
                {"id":"cli-human","status":"passed"},
                {"id":"tui-no-session","status":"passed"},
                {"id":"api-schema-1","status":"passed"},
                {"id":"api-schema-2","status":"passed"}
            ],
            "non_guarantees": [
                "current_os_user_authority",
                "not_kernel_isolation",
                "optional_adapter_scoped_sandbox"
            ]
        }),
    )?;

    // When / Then
    assert!(parse_spec030_manual_qa(&path, &digest).is_ok());
    let mut missing: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    missing["non_guarantees"]
        .as_array_mut()
        .expect("non-guarantees array")
        .pop();
    write_json(&path, &missing)?;
    assert_eq!(
        parse_spec030_manual_qa(&path, &digest).expect_err("non-guarantee is mandatory"),
        Spec030ReleaseArtifactError::InvalidManualRecord
    );
    missing["non_guarantees"] = serde_json::json!([
        "current_os_user_authority",
        "not_kernel_isolation",
        "optional_adapter_scoped_sandbox"
    ]);
    write_json(&path, &missing)?;
    assert_eq!(
        parse_spec030_manual_qa(&path, &format!("sha256:{}", "2".repeat(64)))
            .expect_err("source substitution fails"),
        Spec030ReleaseArtifactError::InvalidManualRecord
    );
    write_json(&path, &serde_json::json!({"status":"provided","record":0}))?;
    assert_eq!(
        parse_spec030_manual_qa(&path, &digest).expect_err("placeholder fails"),
        Spec030ReleaseArtifactError::InvalidManualRecord
    );
    Ok(())
}

#[test]
fn surface_assertions_reject_wrong_status_and_json_divergence(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = temp_path("surface");
    fs::create_dir_all(&root)?;
    let projection = serde_json::json!({
        "schemaVersion": 1,
        "availability": "available",
        "status": "degraded",
        "profile": {
            "availability":"available", "status":"active", "profile":"trustedLocalAgent",
            "executionAuthority":"currentOsUser", "workspaceTrust":"userAsserted",
            "resourceTrust":"explicitOrTrustedWorkspace", "defaultContainment":"none",
            "optionalSandbox":"adapterScoped"
        },
        "hooks": {"availability":"available","status":"active","registeredHandlers":1},
        "credential": {"availability":"available","status":"resolved","source":"environment","refreshSerialization":"inactive"},
        "sandbox": {"availability":"available","status":"active","fallback":"notApplicable"},
        "processAdapters": [{"support":"supported","capabilities":{"timeout":true,"abort":true,"cwd":true,"boundedOutput":true,"descendantCleanup":true,"startupReadiness":true}}],
        "resources": [{"status":"loaded"}],
        "disclosure": {"rawContentPossible":true,"surfaces":["session","log","trace","toolOutput","extensionData"],"trace":{"status":"disabled"}}
    });
    write_json(&root.join("cli.json"), &projection)?;
    let cli_human = concat!(
        "Trusted runtime: availability=degraded\n",
        "profile: status=active\n",
        "credential: availability=available status=resolved source=environment\n",
        "sandbox: availability=available status=active fallback=notApplicable\n",
        "disclosure: rawContentPossible=true surfaces=session,log,trace,toolOutput,extensionData\n",
        "trace: status=disabled"
    );
    let tui_no_session = concat!(
        "Trusted runtime: availability=degraded\n",
        "credential: availability=unavailable status=unavailable source=unavailable\n",
        "sandbox: availability=unavailable status=unknown fallback=unknown\n",
        "status: no sessions"
    );
    let tui_runtime = format!(
        "{cli_human}\nprocess: adapter=bash support=supported controlScope=controlledChild"
    );
    fs::write(root.join("cli.txt"), cli_human)?;
    fs::write(root.join("tui-no-session.txt"), tui_no_session)?;
    fs::write(root.join("tui-runtime.txt"), &tui_runtime)?;
    write_json(
        &root.join("api.json"),
        &serde_json::json!({
            "schema1":{"status":200,"body":projection},
            "schema2":{"status":400,"body":{"error":{"type":"invalid_request_error"}}}
        }),
    )?;

    // When / Then
    assert!(parse_spec030_surface_assertions(&root).is_ok());
    fs::write(root.join("tui-runtime.txt"), tui_no_session)?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("stale no-session replay fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    fs::write(root.join("tui-runtime.txt"), &tui_runtime)?;
    fs::write(root.join("tui-no-session.txt"), &tui_runtime)?;
    fs::write(root.join("tui-runtime.txt"), tui_no_session)?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("swapped TUI roles fail"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    fs::write(root.join("tui-no-session.txt"), tui_no_session)?;
    fs::write(root.join("tui-runtime.txt"), &tui_runtime)?;
    fs::write(
        root.join("tui-no-session.txt"),
        format!(
            "{tui_no_session}\ncredential: availability=available status=resolved source=environment"
        ),
    )?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("no-session contamination fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    fs::write(root.join("tui-no-session.txt"), tui_no_session)?;
    fs::write(
        root.join("tui-runtime.txt"),
        format!("{tui_runtime}\nstatus: no sessions"),
    )?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("runtime contamination fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    fs::write(root.join("tui-runtime.txt"), &tui_runtime)?;
    fs::remove_file(root.join("tui-runtime.txt"))?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("missing runtime TUI fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    fs::write(root.join("tui-runtime.txt"), &tui_runtime)?;
    fs::remove_file(root.join("tui-no-session.txt"))?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("missing pre-exercise TUI fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    fs::write(root.join("tui-no-session.txt"), tui_no_session)?;
    let mut api: serde_json::Value = serde_json::from_slice(&fs::read(root.join("api.json"))?)?;
    api["schema2"]["status"] = serde_json::json!(200);
    write_json(&root.join("api.json"), &api)?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("wrong status fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    api["schema2"]["status"] = serde_json::json!(400);
    api["schema1"]["body"]["credential"]["status"] = serde_json::json!("missing");
    write_json(&root.join("api.json"), &api)?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("JSON divergence fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    write_json(
        &root.join("api.json"),
        &serde_json::json!({
            "schema1":{"status":200,"body":projection},
            "schema2":{"status":400,"body":{"error":{"type":"invalid_request_error"}}}
        }),
    )?;
    let mut unavailable = projection.clone();
    unavailable["availability"] = serde_json::json!("unavailable");
    unavailable["status"] = serde_json::json!("unavailable");
    write_json(&root.join("cli.json"), &unavailable)?;
    write_json(
        &root.join("api.json"),
        &serde_json::json!({
            "schema1":{"status":200,"body":unavailable},
            "schema2":{"status":400,"body":{"error":{"type":"invalid_request_error"}}}
        }),
    )?;
    assert_eq!(
        parse_spec030_surface_assertions(&root).expect_err("unavailable active owner fails"),
        Spec030ReleaseArtifactError::InvalidSurfaceEvidence
    );
    for (pointer, replacement) in [
        ("/profile/executionAuthority", serde_json::json!("unknown")),
        ("/hooks/registeredHandlers", serde_json::json!(0)),
        (
            "/processAdapters/0/capabilities/abort",
            serde_json::json!(false),
        ),
        (
            "/credential/refreshSerialization",
            serde_json::json!("unavailable"),
        ),
        ("/credential/status", serde_json::json!("missing")),
        ("/sandbox/status", serde_json::json!("disabled")),
        ("/resources", serde_json::json!([])),
        ("/disclosure/rawContentPossible", serde_json::json!(false)),
    ] {
        let mut false_feature = projection.clone();
        *false_feature
            .pointer_mut(pointer)
            .expect("feature pointer exists") = replacement;
        write_json(&root.join("cli.json"), &false_feature)?;
        write_json(
            &root.join("api.json"),
            &serde_json::json!({
                "schema1":{"status":200,"body":false_feature},
                "schema2":{"status":400,"body":{"error":{"type":"invalid_request_error"}}}
            }),
        )?;
        assert_eq!(
            parse_spec030_surface_assertions(&root).expect_err("false feature value fails"),
            Spec030ReleaseArtifactError::InvalidSurfaceEvidence
        );
    }
    Ok(())
}

#[test]
fn cleanup_receipt_rejects_remaining_processes_and_temporaries(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let path = temp_path("cleanup");
    write_json(
        &path,
        &serde_json::json!({
            "schema":"spec030.cleanup.v1",
            "processes_started":7,
            "processes_remaining":0,
            "temporary_artifacts_removed":1,
            "temporary_artifacts_remaining":0
        }),
    )?;

    // When / Then
    assert_eq!(
        validate_spec030_cleanup_receipt(&path).expect_err("placeholder cleanup is unbound"),
        Spec030ReleaseArtifactError::InvalidCleanupRecord
    );
    let mut receipt: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    receipt["processes_remaining"] = serde_json::json!(1);
    write_json(&path, &receipt)?;
    assert_eq!(
        validate_spec030_cleanup_receipt(&path).expect_err("remaining process fails"),
        Spec030ReleaseArtifactError::InvalidCleanupRecord
    );
    Ok(())
}
