use shacs_projection::*;

pub fn projection_for_state(
    state: Spec035MediaState,
) -> Result<Spec035MediaProjection, Box<dyn std::error::Error>> {
    let mut input = included_input()?;
    input.state = state;
    input.reason.code = state.into();
    input.reason.safe_summary = Spec031SafeSummary::try_new(match state {
        Spec035MediaState::Included => "media evidence included",
        Spec035MediaState::Unsupported => "media capability unsupported",
        Spec035MediaState::ExtractionFailed => "media extraction failed",
        Spec035MediaState::AnalyzerMissing => "media analyzer missing",
        Spec035MediaState::Truncated => "media evidence truncated",
        Spec035MediaState::Unavailable => "media evidence unavailable",
    })?;
    match state {
        Spec035MediaState::Included | Spec035MediaState::Truncated => {}
        Spec035MediaState::Unsupported | Spec035MediaState::ExtractionFailed => {
            input.lineage.evidence_digest = None;
        }
        Spec035MediaState::AnalyzerMissing => {
            make_unavailable(
                &mut input,
                Spec031Freshness::Unavailable,
                Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef,
            );
        }
        Spec035MediaState::Unavailable => {
            make_unavailable(
                &mut input,
                Spec031Freshness::Stale,
                Spec035MediaOwnerUnavailableReason::StaleOwnerFacts,
            );
        }
    }
    Ok(Spec035MediaProjection::try_new(input)?)
}

pub fn included_input() -> Result<Spec035MediaProjectionInput, Box<dyn std::error::Error>> {
    Ok(Spec035MediaProjectionInput {
        state: Spec035MediaState::Included,
        reason: Spec035MediaReason {
            code: Spec035MediaReasonCode::Included,
            safe_summary: Spec031SafeSummary::try_new("media evidence included")?,
        },
        lineage: Spec035MediaLineage {
            artifact_ref: Spec031ExternalOwnerRef::try_new("spec034://media/artifact/channel")?,
            analyzer_ref: Some(Spec031ExternalOwnerRef::try_new(
                "spec034://media/analyzer/channel",
            )?),
            snapshot_ref: Some(Spec035MediaOpaqueRef::try_new("snapshot:034:channel")?),
            evidence_digest: Some(Spec035MediaDigest::try_new(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )?),
        },
        owner_facts: current_owner_facts()?,
    })
}

pub fn current_owner_facts() -> Result<Spec035MediaOwnerFactsInput, Box<dyn std::error::Error>> {
    Ok(Spec035MediaOwnerFactsInput {
        freshness: Spec031Freshness::Current,
        unavailable_reasons: Vec::new(),
        facts: vec![
            Spec035MediaOwnerFactInput::AnalyzerSource {
                analyzer_ref: Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/channel")?,
                source: ResourceSource::Explicit,
                activation: ResourceActivation::Explicit,
                trust: ResourceTrust::Unknown,
                trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            },
            Spec035MediaOwnerFactInput::Sandbox(SandboxStatusProjection {
                availability: Spec030Availability::Available,
                status: SandboxStatus::Active,
                fallback: SandboxFallback::NotApplicable,
                applied_adapters: vec![ProcessAdapterKind::GenericExec],
                filesystem_policy: SandboxFilesystemPolicy::Applied,
                network_policy: SandboxNetworkPolicy::Applied,
            }),
            Spec035MediaOwnerFactInput::Credential(CredentialStatusProjection {
                availability: Spec030Availability::Available,
                status: CredentialStatus::Resolved,
                source: Some(CredentialSource::Environment),
                fingerprint: CredentialFingerprintStatus::Current,
                refresh_serialization: RefreshSerializationStatus::Active,
            }),
            Spec035MediaOwnerFactInput::Disclosure(Spec035MediaDisclosureFact {
                raw_content_possible: true,
                surfaces: vec![DataSurface::Session, DataSurface::Trace],
                trace_status: TraceStatus::Enabled,
            }),
            Spec035MediaOwnerFactInput::Snapshot {
                snapshot_ref: Spec035MediaOpaqueRef::try_new("snapshot:034:channel")?,
                provenance_digest: Spec035MediaDigest::try_new(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )?,
            },
        ],
    })
}

fn make_unavailable(
    input: &mut Spec035MediaProjectionInput,
    freshness: Spec031Freshness,
    reason: Spec035MediaOwnerUnavailableReason,
) {
    input.lineage.analyzer_ref = None;
    input.lineage.snapshot_ref = None;
    input.lineage.evidence_digest = None;
    input.owner_facts = Spec035MediaOwnerFactsInput {
        freshness,
        unavailable_reasons: vec![reason],
        facts: Vec::new(),
    };
}
