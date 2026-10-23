use shacs_projection::*;
use std::error::Error;

fn safe_ref(value: &str) -> Result<Spec031SubjectRef, Spec031ConstructionError> {
    Spec031SubjectRef::try_new(value)
}

fn safe_summary(value: &str) -> Result<Spec031SafeSummary, Spec031ConstructionError> {
    Spec031SafeSummary::try_new(value)
}

fn input(
    summary: Spec031SafeSummary,
    subject_ref: Spec031SubjectRef,
) -> Result<Spec031EnvelopeInput, Spec031ConstructionError> {
    Ok(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Diagnostics,
        state: Spec031Availability::Degraded,
        severity: Spec031Severity::Warning,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Degraded,
            safe_summary: summary,
        },
        lineage: Spec031Lineage {
            subject_ref,
            parent_ref: Some(Spec031ParentRef::try_new("parent:boundary:safe")?),
            action_ref: Some(Spec031ActionRef::try_new("action:boundary:safe")?),
            digest: Some(Spec031Digest::try_new("sha256:boundarysafe")?),
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

#[test]
fn spec031_boundary_nested_values_cannot_serialize_raw_from_public_bypass(
) -> Result<(), Box<dyn Error>> {
    let unsafe_reason = serde_json::json!({
        "code": "blocked",
        "safe_summary": "OPENAI_API_KEY=sk-spec031-public-bypass"
    });
    let reason: Spec031Reason = serde_json::from_value(unsafe_reason)?;
    let serialized = serde_json::to_string(&reason)?;

    assert!(!serialized.contains("sk-spec031-public-bypass"));
    assert!(Spec031SafeSummary::try_new("HOME=/Users/spec031-public-path-bypass").is_err());
    assert!(Spec031SubjectRef::try_new("/Users/spec031-public-ref-bypass").is_err());
    Ok(())
}

#[test]
fn spec031_boundary_public_parse_error_never_echoes_invalid_sentinel() {
    let raw = r#"{
        "schema_version": 1,
        "kind": "diagnostics",
        "state": "spec031_unknown_state_sentinel",
        "severity": "warning",
        "reason": {"code": "degraded", "safe_summary": "safe"},
        "lineage": {"subject_ref": "subject:boundary:safe"},
        "source": {"owner": "spec031", "freshness": "current"},
        "capability": {"kind": "diagnostics", "details": {"component_count": 1}}
    }"#;

    let error = Spec031Envelope::parse_json(raw).expect_err("invalid state must fail");
    let display = error.to_string();
    let debug = format!("{error:?}");

    assert!(!display.contains("spec031_unknown_state_sentinel"));
    assert!(!debug.contains("spec031_unknown_state_sentinel"));
}

#[test]
fn spec031_boundary_rejects_absolute_paths_after_env_or_label_separator(
) -> Result<(), Box<dyn Error>> {
    let cases = [
        "HOME=/Users/spec031-env-path-leak",
        "PATH:/usr/bin/spec031-colon-path-leak",
    ];

    for case in cases {
        let result = safe_summary(case)
            .and_then(|summary| input(summary, safe_ref("subject:path:safe")?))
            .and_then(Spec031Envelope::try_new);
        let output = match result {
            Ok(envelope) => serde_json::to_string(&envelope)?,
            Err(error) => error.to_string(),
        };
        assert!(!output.contains("spec031-env-path-leak"));
        assert!(!output.contains("spec031-colon-path-leak"));
    }
    Ok(())
}

#[test]
fn spec031_boundary_allows_safe_words_that_contain_pid_letters() -> Result<(), Box<dyn Error>> {
    let envelope = Spec031Envelope::try_new(input(
        safe_summary("rapid progress update")?,
        safe_ref("subject:rapid:safe")?,
    )?)?;
    let serialized = serde_json::to_string(&envelope)?;

    assert!(serialized.contains("rapid progress update"));
    Ok(())
}

#[test]
fn spec031_boundary_rejects_exact_pid_and_process_labels() {
    for value in [
        "pid",
        "pid=424242",
        "process_handle",
        "process_handle=424242",
    ] {
        assert!(Spec031SafeSummary::try_new(value).is_err(), "{value}");
    }
}
