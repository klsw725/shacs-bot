use shacs_projection::*;
use shacs_session::{Session, SessionManager};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: spec034_media_workspace <workspace>")?;
    let projection = Spec035MediaProjection::try_new(Spec035MediaProjectionInput {
        state: Spec035MediaState::Included,
        reason: Spec035MediaReason {
            code: Spec035MediaReasonCode::Included,
            safe_summary: Spec031SafeSummary::try_new("bounded analyzer evidence included")?,
        },
        lineage: Spec035MediaLineage {
            artifact_ref: Spec031ExternalOwnerRef::try_new("spec034://media/artifact/tui-fixture")?,
            analyzer_ref: Some(Spec031ExternalOwnerRef::try_new(
                "spec034://media/analyzer/tui-fixture",
            )?),
            snapshot_ref: Some(Spec035MediaOpaqueRef::try_new("snapshot:034:tui-fixture")?),
            evidence_digest: Some(Spec035MediaDigest::try_new(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )?),
        },
        owner_facts: Spec035MediaOwnerFactsInput {
            freshness: Spec031Freshness::Current,
            unavailable_reasons: Vec::new(),
            facts: vec![
                Spec035MediaOwnerFactInput::AnalyzerSource {
                    analyzer_ref: Spec031ExternalOwnerRef::try_new(
                        "spec034://media/analyzer/tui-fixture",
                    )?,
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
                    snapshot_ref: Spec035MediaOpaqueRef::try_new("snapshot:034:tui-fixture")?,
                    provenance_digest: Spec035MediaDigest::try_new(
                        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )?,
                },
            ],
        },
    })?;
    let mut session = Session::new("cli:direct");
    session.metadata.insert(
        "media_capability".to_owned(),
        serde_json::to_value(projection)?,
    );
    session.add_message("user", "미디어 상태 확인", serde_json::Map::new());
    let mut manager = SessionManager::new(workspace)?;
    manager.save(&session)?;
    let mut cjk_session = Session::new("cli:한국어");
    cjk_session.add_message("user", "정렬 및 잘림 확인", serde_json::Map::new());
    manager.save(&cjk_session)?;
    Ok(())
}
