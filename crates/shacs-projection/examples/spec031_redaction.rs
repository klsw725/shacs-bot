use shacs_projection::*;
use std::{error::Error, io};

fn input(summary: &str) -> Result<Spec031EnvelopeInput, Spec031ConstructionError> {
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
            subject_ref: Spec031SubjectRef::try_new("subject:manual:redaction")?,
            parent_ref: Some(Spec031ParentRef::try_new("parent:manual:redaction")?),
            action_ref: Some(Spec031ActionRef::try_new("action:manual:redaction")?),
            digest: Some(Spec031Digest::try_new("sha256:manualredaction")?),
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

fn sentinel_values() -> [&'static str; 12] {
    [
        "ghp_spec031_token_sentinel",
        "spec031-credential-url-pass",
        "spec031-host-path-secret",
        "sk-spec031-env-value-sentinel",
        "spec031-process-handle-sentinel",
        "spec031-provider-payload",
        "spec031-tool-payload",
        "spec031-stdout-sentinel",
        "spec031-stderr-sentinel",
        "spec031-prompt-bytes",
        "spec031-media-bytes",
        "sk-spec031-stale-state-token",
    ]
}

fn contains_any_sentinel(value: &str) -> bool {
    sentinel_values()
        .iter()
        .any(|sentinel| value.contains(sentinel))
}

fn fail(message: &'static str) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message))
}

fn main() -> Result<(), Box<dyn Error>> {
    let safe = Spec031Envelope::try_new(input("manual safe degraded summary")?)?;
    let safe_output = serde_json::to_string(&safe)?;
    if contains_any_sentinel(&safe_output) {
        return Err(fail("safe envelope output contained a sentinel"));
    }
    println!(
        "safe_envelope bytes={} sentinel_absent=true",
        safe_output.len()
    );

    let unsafe_inputs = [
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

    for (label, summary) in unsafe_inputs {
        let rendered = match input(summary).and_then(Spec031Envelope::try_new) {
            Ok(envelope) => serde_json::to_string(&envelope)?,
            Err(error) => format!("rejected field={} kind={:?}", error.field(), error.kind()),
        };
        if contains_any_sentinel(&rendered) {
            return Err(fail("unsafe output contained a sentinel"));
        }
        println!("unsafe_case {label} sentinel_absent=true output={rendered}");
    }

    let mut stale_json = serde_json::to_value(&safe)?;
    stale_json["lineage"]["subject_ref"] =
        serde_json::json!("/Users/spec031-host-path-secret/session.json");
    stale_json["reason"]["safe_summary"] =
        serde_json::json!("OPENAI_API_KEY=sk-spec031-stale-state-token");
    let stale_output = match Spec031Envelope::from_json_value(stale_json) {
        Ok(envelope) => serde_json::to_string(&envelope)?,
        Err(error) => format!("rejected stale_json {error}"),
    };
    if contains_any_sentinel(&stale_output) {
        return Err(fail("stale-state output contained a sentinel"));
    }
    println!("stale_state sentinel_absent=true output={stale_output}");
    Ok(())
}
