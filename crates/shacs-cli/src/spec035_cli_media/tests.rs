use super::{present_media_projection, read_media_projection_directory};
use crate::{
    format_runtime_inspect, runtime_inspect, ExternalTransportRuntimeContext, RuntimeInspectOptions,
};
use serde_json::{json, Value};
use shacs_projection::Spec035MediaProjection;
use std::error::Error;
use std::fs;

const STATES: [&str; 6] = [
    "included",
    "unsupported",
    "extraction_failed",
    "analyzer_missing",
    "truncated",
    "unavailable",
];

#[test]
fn cli_media_presentation_preserves_all_canonical_fields_for_six_states(
) -> Result<(), Box<dyn Error>> {
    for state in STATES {
        let projection = projection(state)?;

        let presented = present_media_projection(&projection)?;

        assert_eq!(
            serde_json::from_str::<Value>(&presented.machine_json)?,
            serde_json::to_value(&projection)?
        );
        assert_eq!(presented.machine_json, serde_json::to_string(&projection)?);
        assert_eq!(
            presented.human,
            format!(
                "Spec035 media: state={state} reason={state} freshness={} summary=bounded-{state}",
                if matches!(state, "analyzer_missing" | "unavailable") {
                    "unavailable"
                } else {
                    "current"
                }
            )
        );
    }
    Ok(())
}

#[test]
fn cli_media_directory_reads_canonical_records_in_stable_order() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for (index, state) in STATES.iter().rev().enumerate() {
        fs::write(
            root.path().join(format!("{index}-{state}.json")),
            serde_json::to_vec(&projection(state)?)?,
        )?;
    }

    let inspected = read_media_projection_directory(root.path())?;

    let states = inspected
        .iter()
        .map(|projection| serde_json::to_value(projection).map(|value| value["state"].clone()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        states,
        vec![
            json!("unavailable"),
            json!("truncated"),
            json!("analyzer_missing"),
            json!("extraction_failed"),
            json!("unsupported"),
            json!("included"),
        ]
    );
    Ok(())
}

#[test]
fn cli_media_directory_rejects_malformed_or_misleading_records() -> Result<(), Box<dyn Error>> {
    for invalid in [
        "{malformed}".to_owned(),
        invalid_projection("unknown_field", json!("secret-provider-body"))?,
        invalid_projection("state", json!("success"))?,
        stale_success()?,
    ] {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("projection.json"), invalid)?;

        assert!(read_media_projection_directory(root.path()).is_err());
    }
    Ok(())
}

#[test]
fn cli_media_output_excludes_forbidden_material() -> Result<(), Box<dyn Error>> {
    let mut output = String::new();
    for state in STATES {
        let presented = present_media_projection(&projection(state)?)?;
        output.push_str(&presented.machine_json);
        output.push_str(&presented.human);
    }

    for forbidden in [
        "https://",
        "data:image",
        "base64",
        "secret-token",
        "raw_provider_body",
        "/Users/",
    ] {
        assert!(!output.contains(forbidden), "forbidden output: {forbidden}");
    }
    assert!(output.len() < 24_000);
    Ok(())
}

#[test]
fn runtime_inspect_reads_the_runtime_owned_canonical_media_record() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("workspace");
    let mut config = shacs_config::Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().into_owned();
    crate::save_config_to_path(&config, &config_path)?;
    let expected = projection("included")?;
    shacs_core::runtime::Spec035MediaProjectionStore::new(root.path()).publish(&expected)?;

    let report = runtime_inspect(RuntimeInspectOptions {
        config_path: Some(config_path),
        workspace_override: None,
    })?;
    let diagnostics = crate::diagnostics_snapshot_from_runtime_inspect(&report);
    assert_eq!(
        diagnostics.runtime["spec035_media_projections"],
        json!(report.media_projections)
    );
    let output = format_runtime_inspect(report);

    assert!(output.contains("Spec035 media projections: 1"));
    assert!(output.contains("Spec035 media: state=included reason=included"));
    let machine = output
        .lines()
        .filter_map(|line| line.strip_prefix("Spec035 media JSON: "))
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(machine, [serde_json::to_value(expected)?]);
    for forbidden in ["https://", "base64", "secret-token", "/Users/"] {
        assert!(!output.contains(forbidden));
    }
    Ok(())
}

#[test]
fn external_channel_projects_the_runtime_owned_media_record() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let expected = projection("included")?;
    shacs_core::runtime::Spec035MediaProjectionStore::new(root.path()).publish(&expected)?;
    let mut context = ExternalTransportRuntimeContext::new(root.path().join("metadata"), 1);
    context.configure_durable_inbound(root.path().to_path_buf(), "owner:test".to_owned(), None);

    // When
    let channel = context
        .media_projection()?
        .ok_or("channel projection missing")?;

    // Then
    assert_eq!(channel.media_capability(), &expected);
    assert_eq!(
        channel.delivery_status(),
        shacs_channels::ChannelSpec035MediaDelivery::Pending
    );
    Ok(())
}

fn projection(state: &str) -> Result<Spec035MediaProjection, Box<dyn Error>> {
    Spec035MediaProjection::from_json_value(projection_value(state)?).map_err(Into::into)
}

fn projection_value(state: &str) -> Result<Value, Box<dyn Error>> {
    let mut value = included_value();
    value["state"] = json!(state);
    value["reason"] = json!({"code": state, "safe_summary": format!("bounded-{state}")});
    match state {
        "included" | "truncated" => {}
        "unsupported" | "extraction_failed" => {
            value["lineage"]
                .as_object_mut()
                .ok_or("lineage")?
                .remove("evidence_digest");
        }
        "analyzer_missing" | "unavailable" => {
            let lineage = value["lineage"].as_object_mut().ok_or("lineage")?;
            lineage.remove("analyzer_ref");
            lineage.remove("snapshot_ref");
            lineage.remove("evidence_digest");
            value["freshness"] = json!("unavailable");
            value["disclosure"] = json!({"status": "unavailable"});
            value["owner_facts"] = json!({
                "unavailable_reasons": ["missing_analyzer_owner_ref"]
            });
        }
        other => return Err(format!("unsupported test state {other}").into()),
    }
    Ok(value)
}

fn invalid_projection(key: &str, value: Value) -> Result<String, Box<dyn Error>> {
    let mut projection = included_value();
    projection[key] = value;
    Ok(serde_json::to_string(&projection)?)
}

fn stale_success() -> Result<String, Box<dyn Error>> {
    let mut projection = included_value();
    projection["freshness"] = json!("stale");
    Ok(serde_json::to_string(&projection)?)
}

fn included_value() -> Value {
    json!({
        "schema_version": 1,
        "kind": "media_capability",
        "state": "included",
        "reason": {"code": "included", "safe_summary": "bounded-included"},
        "lineage": {
            "artifact_ref": "spec034://media/artifact/cli-fixture",
            "analyzer_ref": "spec034://media/analyzer/cli-fixture",
            "snapshot_ref": "snapshot:034:cli_fixture",
            "evidence_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        },
        "freshness": "current",
        "disclosure": {
            "status": "recorded",
            "raw_content_possible": true,
            "surfaces": ["session", "trace"],
            "trace_status": "enabled"
        },
        "owner_facts": {
            "unavailable_reasons": [],
            "analyzer_source": {
                "analyzer_ref": "spec034://media/analyzer/cli-fixture",
                "source": "explicit",
                "activation": "explicit",
                "trust": "explicitOrTrustedWorkspace",
                "trusted_code_disclosure": "shown"
            },
            "sandbox": {
                "availability": "available",
                "status": "active",
                "fallback": "notApplicable",
                "appliedAdapters": ["genericExec"],
                "filesystemPolicy": "applied",
                "networkPolicy": "applied"
            },
            "credential": {
                "availability": "available",
                "status": "resolved",
                "source": "environment",
                "fingerprint": "current",
                "refreshSerialization": "active"
            },
            "snapshot": {
                "snapshot_ref": "snapshot:034:cli_fixture",
                "provenance_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        }
    })
}
