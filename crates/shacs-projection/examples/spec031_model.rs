use shacs_projection::*;
use std::{error::Error, io};

fn base_input(
    capability: Spec031Capability,
) -> Result<Spec031EnvelopeInput, Spec031ConstructionError> {
    Ok(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Readiness,
        state: Spec031Availability::Ready,
        severity: Spec031Severity::Info,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Included,
            safe_summary: Spec031SafeSummary::try_new("qa safe summary")?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new("subject:qa:1")?,
            parent_ref: Some(Spec031ParentRef::try_new("parent:qa:1")?),
            action_ref: Some(Spec031ActionRef::try_new("action:qa:1")?),
            digest: Some(Spec031Digest::try_new("sha256:qa")?),
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Projection,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
            freshness: Spec031Freshness::Current,
        },
        capability,
        children: Vec::new(),
    })
}

fn base_envelope(capability: Spec031Capability) -> Result<Spec031Envelope, Box<dyn Error>> {
    Ok(Spec031Envelope::try_new(base_input(capability)?)?)
}

fn fail(message: &'static str) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message))
}

fn assert_json_rejected(serialized: &str, label: &str) -> Result<(), Box<dyn Error>> {
    match Spec031Envelope::parse_json(serialized) {
        Ok(_) => Err(fail("invalid Spec031 envelope parsed successfully")),
        Err(error) => {
            println!("parse_failure {label}={error}");
            Ok(())
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let availability = base_envelope(Spec031Capability::Readiness(Spec031ReadinessCapability {
        availability: Spec031Availability::Ready,
        component_count: None,
        queue_depth: None,
        queue_capacity: None,
        remediation: None,
    }))?;
    let approval = base_envelope(Spec031Capability::Approval(Spec031ApprovalCapability {
        state: Spec031ApprovalState::Pending,
    }))?;
    let mut inclusion_input = base_input(Spec031Capability::Context(Spec031ContextCapability {
        reason: Spec031InclusionReason::Skipped,
    }))?;
    inclusion_input.reason.code = Spec031ReasonCode::Skipped;
    let inclusion = Spec031Envelope::try_new(inclusion_input)?;
    let progress = base_envelope(Spec031Capability::Progress(
        Spec031ProgressCapability::delivery(Spec031ProgressDelivery::Coalesced),
    ))?;
    let mut severity_input = base_input(Spec031Capability::Diagnostics(
        Spec031DiagnosticsCapability {
            component_count: Some(Spec031Count::new(0)),
        },
    ))?;
    severity_input.severity = Spec031Severity::Critical;
    let severity = Spec031Envelope::try_new(severity_input)?;
    let mut freshness_input = base_input(Spec031Capability::Session(Spec031SessionCapability {
        active_turn_count: None,
    }))?;
    freshness_input.source.freshness = Spec031Freshness::Stale;
    let freshness = Spec031Envelope::try_new(freshness_input)?;
    let mut kind_input = base_input(Spec031Capability::Media(Spec031MediaCapability {
        reason: Spec031InclusionReason::ExtractionFailed,
    }))?;
    kind_input.kind = Spec031ProjectionKind::Media;
    let kind = Spec031Envelope::try_new(kind_input)?;
    let mut owner_input = base_input(Spec031Capability::ReleaseEvidence(
        Spec031ReleaseEvidenceCapability {
            blocker_count: Some(Spec031Count::new(1)),
        },
    ))?;
    owner_input.source.owner = Spec031SourceOwner::Spec029;
    let owner = Spec031Envelope::try_new(owner_input)?;

    for (label, envelope) in [
        ("availability", availability),
        ("approval", approval),
        ("inclusion_reason", inclusion),
        ("progress_delivery", progress),
        ("severity", severity),
        ("freshness", freshness),
        ("kind", kind),
        ("source_owner", owner),
    ] {
        let serialized = serde_json::to_string(&envelope)?;
        let parsed = Spec031Envelope::parse_json(&serialized)?;
        if parsed != envelope {
            return Err(fail("Spec031 roundtrip changed the envelope"));
        }
        println!(
            "valid_roundtrip {label} schema={}",
            parsed.schema_version().as_u32()
        );
    }

    match Spec031SchemaVersion::try_from_raw(2) {
        Err(Spec031VersionError::Unsupported { found }) => {
            println!("typed_failure version=unsupported found={found}");
        }
        Ok(_) => return Err(fail("unsupported Spec031 schema version was accepted")),
    }

    let invalid_base = serde_json::to_string(&base_envelope(Spec031Capability::Readiness(
        Spec031ReadinessCapability {
            availability: Spec031Availability::Ready,
            component_count: None,
            queue_depth: None,
            queue_capacity: None,
            remediation: None,
        },
    ))?)?;
    let unknown_version = invalid_base.replace("\"schema_version\":1", "\"schema_version\":2");
    assert_json_rejected(&unknown_version, "unknown_version")?;

    let unknown_state = invalid_base.replace("\"state\":\"ready\"", "\"state\":\"not_ready\"");
    assert_json_rejected(&unknown_state, "unknown_state")?;
    Ok(())
}
