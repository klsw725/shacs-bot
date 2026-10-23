use serde_json::{json, Value};
use shacs_projection::*;
use shacs_session::{Session, SessionManager};
use shacs_tui::{
    live_source::{RuntimeProjectionSource, SessionRuntimeSource},
    state::TuiState,
    view::render_lines,
};
use std::{error::Error, path::Path};

#[test]
fn canonical_media_states_render_consistently() -> Result<(), Box<dyn Error>> {
    for state in [
        Spec035MediaState::Included,
        Spec035MediaState::Unsupported,
        Spec035MediaState::Unavailable,
        Spec035MediaState::ExtractionFailed,
        Spec035MediaState::AnalyzerMissing,
        Spec035MediaState::Truncated,
    ] {
        // Given
        let workspace = tempfile::tempdir()?;
        save_session(workspace.path(), canonical_projection(state)?)?;

        // When
        let rendered = render_workspace(workspace.path())?;

        // Then
        let label = state_label(state);
        assert!(rendered.contains(&format!(
            "media: state={label} reason={label} freshness={}",
            expected_freshness(state)
        )));
        assert!(rendered.contains("media reason: bounded media projection"));
        assert!(rendered.contains("media lineage: artifact=spec034://media/artifact/fixture"));
        match state {
            Spec035MediaState::Included | Spec035MediaState::Truncated => {
                assert!(rendered.contains("media evidence: sha256:bbbb"));
                assert!(rendered.contains("media disclosure: recorded raw_content_possible=true surfaces=session,trace trace=enabled"));
            }
            Spec035MediaState::Unsupported | Spec035MediaState::ExtractionFailed => {
                assert!(rendered.contains("media evidence: unavailable"));
                assert!(rendered.contains("media disclosure: recorded raw_content_possible=true surfaces=session,trace trace=enabled"));
            }
            Spec035MediaState::AnalyzerMissing | Spec035MediaState::Unavailable => {
                assert!(
                    rendered.contains("media lineage: analyzer=unavailable snapshot=unavailable")
                );
                assert!(rendered.contains("media evidence: unavailable"));
                assert!(rendered.contains("media disclosure: unavailable"));
            }
        }
    }
    Ok(())
}

#[test]
fn malformed_media_falls_back_to_unavailable_without_echoing_untrusted_content(
) -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    save_session(
        workspace.path(),
        json!({
            "state": "included",
            "raw_url": "https://user:secret@example.test/media",
            "base64": "cHJvdmlkZXIgYm9keQ==",
            "provider_body": "provider body token-123",
            "absolute_path": "/Users/private/media.png"
        }),
    )?;

    // When
    let rendered = render_workspace(workspace.path())?;

    // Then
    assert!(rendered.contains("media: state=unavailable reason=unavailable freshness=unavailable"));
    assert!(rendered.contains("media lineage: unavailable"));
    assert!(rendered.contains("media disclosure: unavailable"));
    for forbidden in [
        "https://",
        "cHJvdmlkZXIgYm9keQ==",
        "provider body",
        "token-123",
        "/Users/private",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "leaked forbidden text: {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn misleading_stale_success_falls_back_to_unavailable() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let mut media = canonical_projection(Spec035MediaState::Included)?;
    media["freshness"] = json!("stale");
    save_session(workspace.path(), media)?;

    // When
    let rendered = render_workspace(workspace.path())?;

    // Then
    assert!(rendered.contains("media: state=unavailable reason=unavailable freshness=unavailable"));
    assert!(!rendered.contains("media: state=included"));
    Ok(())
}

#[test]
fn canonical_reason_redacts_base64_tokens() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let mut media = canonical_projection(Spec035MediaState::Included)?;
    media["reason"]["safe_summary"] = json!("source cHJvdmlkZXIgYm9keQ== unavailable");
    save_session(workspace.path(), media)?;

    // When
    let rendered = render_workspace(workspace.path())?;

    // Then
    let reason_line = rendered
        .lines()
        .find(|line| line.starts_with("media reason:"))
        .ok_or("media reason line missing")?;
    assert_eq!(reason_line, "media reason: source [redacted] unavailable");
    assert!(!rendered.contains("cHJvdmlkZXIgYm9keQ=="));
    Ok(())
}

fn render_workspace(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let snapshot =
        SessionRuntimeSource::with_config(Some(workspace.join("config.json")), workspace).load()?;
    let state = TuiState::from_snapshot(snapshot, None);
    Ok(render_lines(&state).join("\n"))
}

fn save_session(workspace: &Path, media: Value) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = Session::new("cli:direct");
    session
        .metadata
        .insert("media_capability".to_owned(), media.clone());
    session.add_message("user", "미디어 상태 확인", serde_json::Map::new());
    manager.save(&session)?;
    if let Ok(projection) = Spec035MediaProjection::from_json_value(media) {
        shacs_core::runtime::Spec035MediaProjectionStore::new(workspace).publish(&projection)?;
    }
    Ok(())
}

fn canonical_projection(state: Spec035MediaState) -> Result<Value, Box<dyn Error>> {
    let unavailable = matches!(state, Spec035MediaState::Unavailable);
    let analyzer_missing = matches!(state, Spec035MediaState::AnalyzerMissing);
    let current = !unavailable && !analyzer_missing;
    let analyzer_ref = current
        .then(|| Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/fixture"))
        .transpose()?;
    let snapshot_ref = current
        .then(|| Spec035MediaOpaqueRef::try_new("snapshot:034:fixture"))
        .transpose()?;
    let evidence_digest = matches!(
        state,
        Spec035MediaState::Included | Spec035MediaState::Truncated
    )
    .then(|| {
        Spec035MediaDigest::try_new(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
    })
    .transpose()?;
    let owner_facts = if current {
        current_owner_facts()?
    } else {
        Spec035MediaOwnerFactsInput {
            freshness: if analyzer_missing {
                Spec031Freshness::Unavailable
            } else {
                Spec031Freshness::Stale
            },
            unavailable_reasons: vec![if analyzer_missing {
                Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef
            } else {
                Spec035MediaOwnerUnavailableReason::StaleOwnerFacts
            }],
            facts: Vec::new(),
        }
    };
    let projection = Spec035MediaProjection::try_new(Spec035MediaProjectionInput {
        state,
        reason: Spec035MediaReason {
            code: state.into(),
            safe_summary: Spec031SafeSummary::try_new("bounded media projection")?,
        },
        lineage: Spec035MediaLineage {
            artifact_ref: Spec031ExternalOwnerRef::try_new("spec034://media/artifact/fixture")?,
            analyzer_ref,
            snapshot_ref,
            evidence_digest,
        },
        owner_facts,
    })?;
    Ok(serde_json::to_value(projection)?)
}

fn current_owner_facts() -> Result<Spec035MediaOwnerFactsInput, Box<dyn Error>> {
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

const fn state_label(state: Spec035MediaState) -> &'static str {
    match state {
        Spec035MediaState::Included => "included",
        Spec035MediaState::Unsupported => "unsupported",
        Spec035MediaState::ExtractionFailed => "extraction_failed",
        Spec035MediaState::AnalyzerMissing => "analyzer_missing",
        Spec035MediaState::Truncated => "truncated",
        Spec035MediaState::Unavailable => "unavailable",
    }
}

const fn expected_freshness(state: Spec035MediaState) -> &'static str {
    match state {
        Spec035MediaState::Included
        | Spec035MediaState::Unsupported
        | Spec035MediaState::ExtractionFailed
        | Spec035MediaState::Truncated => "current",
        Spec035MediaState::AnalyzerMissing => "unavailable",
        Spec035MediaState::Unavailable => "stale",
    }
}
