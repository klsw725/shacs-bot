use shacs_projection::*;

pub fn current_owner_facts() -> Result<Spec035MediaOwnerFactsInput, Box<dyn std::error::Error>> {
    Ok(Spec035MediaOwnerFactsInput {
        freshness: Spec031Freshness::Current,
        unavailable_reasons: Vec::new(),
        facts: vec![
            Spec035MediaOwnerFactInput::AnalyzerSource {
                analyzer_ref: Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/fixture")?,
                source: ResourceSource::Explicit,
                activation: ResourceActivation::Explicit,
                trust: ResourceTrust::ExplicitOrTrustedWorkspace,
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
                snapshot_ref: Spec035MediaOpaqueRef::try_new("snapshot:034:fixture")?,
                provenance_digest: Spec035MediaDigest::try_new(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )?,
            },
        ],
    })
}

pub fn included_input() -> Result<Spec035MediaProjectionInput, Box<dyn std::error::Error>> {
    Ok(Spec035MediaProjectionInput {
        state: Spec035MediaState::Included,
        reason: Spec035MediaReason {
            code: Spec035MediaReasonCode::Included,
            safe_summary: Spec031SafeSummary::try_new("bounded analyzer evidence included")?,
        },
        lineage: Spec035MediaLineage {
            artifact_ref: Spec031ExternalOwnerRef::try_new("spec034://media/artifact/fixture")?,
            analyzer_ref: Some(Spec031ExternalOwnerRef::try_new(
                "spec034://media/analyzer/fixture",
            )?),
            snapshot_ref: Some(Spec035MediaOpaqueRef::try_new("snapshot:034:fixture")?),
            evidence_digest: Some(Spec035MediaDigest::try_new(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )?),
        },
        owner_facts: current_owner_facts()?,
    })
}
