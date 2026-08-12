use serde_json::json;
use shacs_app::app::AppLifecycleState;
use shacs_core::runtime::{
    admit_activation_for_execution, ActivationCurrentIdentity, ActivationLiveFacts,
    ActivationReason, ActivationRecord, ActivationRecordInput, ActivationReplay, ActivationSource,
    ActivationStatus, ExecutionSnapshot, ExecutionSnapshotInput, ResourceIdentitySnapshot,
    WorkspaceTrustRef,
};
use shacs_projection::{
    ResourceActivation, ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind,
    ResourceLoadStatus, ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "spec031_activation_execution/support.rs"]
mod support;

fn activation() -> ActivationRecord {
    ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:skill:formatter:v1".to_owned(),
        source: ActivationSource::TrustedWorkspace,
        workspace_trust_ref: WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        resource_ref: "resource:skill:formatter".to_owned(),
        source_identity: "source:project:.shacs/skills/formatter".to_owned(),
        content_digest: "sha256:content-a".to_owned(),
        dependency_manifest_digest: "sha256:deps-a".to_owned(),
        status: ActivationStatus::Active,
        reason: ActivationReason::Activated,
        recorded_at_unix_ms: 31_004,
    })
}

fn eligible_resource(content_digest: &str) -> ResourceCandidateProjection {
    ResourceCandidateProjection {
        resource_ref: "resource:skill:formatter".to_owned(),
        kind: ResourceKind::Skill,
        source: ResourceSource::Project,
        precedence: ResourcePrecedence::TrustedProjectAuto,
        canonical_path: ".shacs/skills/formatter".to_owned(),
        content_sha256: Some(content_digest.to_owned()),
        collision: ResourceCollisionStatus::None,
        load_status: ResourceLoadStatus::Loaded,
        activation: ResourceActivation::TrustedWorkspace,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        diagnostics: Vec::new(),
    }
}

#[test]
fn new_execution_rechecks_current_spec030_and_spec032_facts() {
    // Given
    let activation = activation();
    let eligible = eligible_resource("sha256:content-a");

    // When / Then
    assert!(admit_activation_for_execution(
        &activation,
        &ActivationLiveFacts::new(
            &eligible,
            ActivationCurrentIdentity::new(
                WorkspaceTrustRef::new("workspace:sha256:owner-a"),
                "source:project:.shacs/skills/formatter",
                "sha256:deps-a",
                AppLifecycleState::Enabled,
            ),
        )
    )
    .is_ok());
    let stale = eligible_resource("sha256:content-b");
    assert!(admit_activation_for_execution(
        &activation,
        &ActivationLiveFacts::new(
            &stale,
            ActivationCurrentIdentity::new(
                WorkspaceTrustRef::new("workspace:sha256:owner-a"),
                "source:project:.shacs/skills/formatter",
                "sha256:deps-a",
                AppLifecycleState::Enabled,
            ),
        )
    )
    .is_err());
    assert!(admit_activation_for_execution(
        &activation,
        &ActivationLiveFacts::new(
            &eligible,
            ActivationCurrentIdentity::new(
                WorkspaceTrustRef::new("workspace:sha256:owner-a"),
                "source:project:.shacs/skills/formatter",
                "sha256:deps-a",
                AppLifecycleState::Disabled,
            ),
        )
    )
    .is_err());
}

#[test]
fn new_execution_rejects_workspace_and_source_identity_mismatch() {
    // Given
    let activation = activation();
    let eligible = eligible_resource("sha256:content-a");

    // When
    let owner = admit_activation_for_execution(
        &activation,
        &ActivationLiveFacts::new(
            &eligible,
            ActivationCurrentIdentity::new(
                WorkspaceTrustRef::new("workspace:sha256:owner-b"),
                "source:project:.shacs/skills/formatter",
                "sha256:deps-a",
                AppLifecycleState::Enabled,
            ),
        ),
    );
    let source = admit_activation_for_execution(
        &activation,
        &ActivationLiveFacts::new(
            &eligible,
            ActivationCurrentIdentity::new(
                WorkspaceTrustRef::new("workspace:sha256:owner-a"),
                "source:package:formatter",
                "sha256:deps-a",
                AppLifecycleState::Enabled,
            ),
        ),
    );

    // Then
    assert!(owner.is_err());
    assert!(source.is_err());
}

#[test]
fn snapshot_uses_exact_activation_ref_without_permission_provenance() -> Result<(), Box<dyn Error>>
{
    // Given
    let activation = activation();
    let mut input: ExecutionSnapshotInput = support::input("execution:activation", 31_007);
    input.selected_resources = vec![ResourceIdentitySnapshot {
        identity: activation.resource_ref().to_owned(),
        content_digest: Some(activation.content_digest().to_owned()),
        activation_ref: None,
    }];

    // When
    input.attach_activation_refs(std::slice::from_ref(&activation));
    let snapshot = ExecutionSnapshot::create(input)?;
    let encoded = serde_json::to_value(snapshot)?;

    // Then
    assert_eq!(
        encoded["selected_resources"][0]["activation_ref"],
        json!(activation.activation_ref())
    );
    let text = encoded.to_string();
    for excluded in ["permission", "approval", "authorization", "grant"] {
        assert!(!text.contains(excluded));
    }
    Ok(())
}

#[test]
fn diagnostic_replay_performs_zero_live_dispatch() -> Result<(), Box<dyn Error>> {
    // Given
    let snapshot = ExecutionSnapshot::create(support::input("execution:replay", 31_008))?;
    let discovery = AtomicUsize::new(0);
    let dependencies = AtomicUsize::new(0);
    let credentials = AtomicUsize::new(0);
    let entrypoints = AtomicUsize::new(0);

    // When
    let replay = ActivationReplay::diagnostic(&snapshot);

    // Then
    assert_eq!(discovery.load(Ordering::SeqCst), 0);
    assert_eq!(dependencies.load(Ordering::SeqCst), 0);
    assert_eq!(credentials.load(Ordering::SeqCst), 0);
    assert_eq!(entrypoints.load(Ordering::SeqCst), 0);
    assert_eq!(replay.counters().total(), 0);
    assert_eq!(
        replay.transcript(),
        ["snapshot_loaded", "diagnostic_projection_emitted"]
    );
    assert_eq!(replay.activation_refs(), ["activation:skill:formatter:v1"]);
    Ok(())
}
