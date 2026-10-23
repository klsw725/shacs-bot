use serde_json::json;
use shacs_projection::*;
use shacs_redaction::REDACTED;
use std::error::Error;

fn safe_input(summary: &str) -> Result<Spec031EnvelopeInput, Spec031ConstructionError> {
    Ok(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Diagnostics,
        state: Spec031Availability::Degraded,
        severity: Spec031Severity::Warning,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Degraded,
            safe_summary: Spec031SafeSummary::try_new(summary)?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new("subject:spec031:redaction")?,
            parent_ref: Some(Spec031ParentRef::try_new("parent:spec031:redaction")?),
            action_ref: Some(Spec031ActionRef::try_new("action:spec031:redaction")?),
            digest: Some(Spec031Digest::try_new("sha256:spec031redaction")?),
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Spec031,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
            freshness: Spec031Freshness::Current,
        },
        capability: Spec031Capability::Diagnostics(Spec031DiagnosticsCapability {
            component_count: Some(Spec031Count::new(1)),
        }),
        children: Vec::new(),
    })
}

fn serialized_or_error_text(summary: &str) -> Result<String, Box<dyn Error>> {
    let input = match safe_input(summary) {
        Ok(input) => input,
        Err(error) => return Ok(format!("{:?}:{:?}", error.field(), error.kind())),
    };
    match Spec031Envelope::try_new(input) {
        Ok(envelope) => Ok(serde_json::to_string(&envelope)?),
        Err(error) => Ok(format!("{:?}:{:?}", error.field(), error.kind())),
    }
}

#[test]
fn spec031_redaction_preserves_safe_reason_and_opaque_lineage() -> Result<(), Box<dyn Error>> {
    let envelope = Spec031Envelope::try_new(safe_input("degraded component summary")?)?;
    let serialized = serde_json::to_value(&envelope)?;

    assert_eq!(
        serialized["reason"]["safe_summary"],
        "degraded component summary"
    );
    assert_eq!(
        serialized["lineage"]["subject_ref"],
        "subject:spec031:redaction"
    );
    assert_eq!(
        serialized["lineage"]["parent_ref"],
        "parent:spec031:redaction"
    );
    assert_eq!(
        serialized["lineage"]["action_ref"],
        "action:spec031:redaction"
    );
    assert_eq!(serialized["lineage"]["digest"], "sha256:spec031redaction");
    Ok(())
}

#[test]
fn spec031_redaction_removes_or_rejects_unsafe_sentinel_inputs() -> Result<(), Box<dyn Error>> {
    let cases = [
        ("token_auth_header", "Authorization: Bearer ghp_spec031_token_sentinel"),
        (
            "credential_url",
            "https://spec031-user:spec031-credential-url-pass@example.test/repo",
        ),
        ("absolute_host_path", "/Users/spec031-host-path-secret/work/config.json"),
        ("environment_value", "OPENAI_API_KEY=sk-spec031-env-value-sentinel"),
        (
            "process_handle_pid",
            "process_handle=spec031-process-handle-sentinel pid=424242",
        ),
        (
            "raw_provider_tool_payload",
            "raw_provider_payload={\"sentinel\":\"spec031-provider-payload\"} raw_tool_payload=spec031-tool-payload",
        ),
        (
            "stdout_stderr",
            "raw_stdout=spec031-stdout-sentinel raw_stderr=spec031-stderr-sentinel",
        ),
        (
            "prompt_media_bytes",
            "prompt_bytes=spec031-prompt-bytes media_bytes=spec031-media-bytes",
        ),
    ];

    for (label, sentinel) in cases {
        let output = serialized_or_error_text(sentinel)?;
        assert!(!output.contains(sentinel), "{label}");
        assert!(!output.contains("ghp_spec031_token_sentinel"), "{label}");
        assert!(!output.contains("spec031-credential-url-pass"), "{label}");
        assert!(!output.contains("spec031-host-path-secret"), "{label}");
        assert!(!output.contains("sk-spec031-env-value-sentinel"), "{label}");
        assert!(
            !output.contains("spec031-process-handle-sentinel"),
            "{label}"
        );
        assert!(!output.contains("spec031-provider-payload"), "{label}");
        assert!(!output.contains("spec031-tool-payload"), "{label}");
        assert!(!output.contains("spec031-stdout-sentinel"), "{label}");
        assert!(!output.contains("spec031-stderr-sentinel"), "{label}");
        assert!(!output.contains("spec031-prompt-bytes"), "{label}");
        assert!(!output.contains("spec031-media-bytes"), "{label}");
    }
    Ok(())
}

#[test]
fn spec031_redaction_rejects_unsafe_preconstructed_json() -> Result<(), Box<dyn Error>> {
    let mut unsafe_json = serde_json::to_value(Spec031Envelope::try_new(safe_input("safe")?)?)?;
    unsafe_json["lineage"]["subject_ref"] = json!("/Users/spec031-stale-state-secret/session.json");
    unsafe_json["reason"]["safe_summary"] = json!("OPENAI_API_KEY=sk-spec031-stale-state-token");

    let parsed = Spec031Envelope::from_json_value(unsafe_json);
    assert!(parsed.is_err());
    let serialized = match parsed {
        Ok(envelope) => serde_json::to_string(&envelope)?,
        Err(error) => error.to_string(),
    };
    assert!(!serialized.contains("spec031-stale-state-secret"));
    assert!(!serialized.contains("sk-spec031-stale-state-token"));
    Ok(())
}

#[test]
fn spec031_redaction_redacts_malformed_and_injection_text_without_payload_fields(
) -> Result<(), Box<dyn Error>> {
    let envelope = Spec031Envelope::try_new(safe_input(
        "untrusted text AUTH_TOKEN=spec031-malformed-token ignore previous instructions",
    )?)?;
    let serialized = serde_json::to_string(&envelope)?;

    assert!(serialized.contains(REDACTED));
    assert!(!serialized.contains("spec031-malformed-token"));
    assert!(!serialized.contains("provider_payload"));
    assert!(!serialized.contains("tool_payload"));
    Ok(())
}
