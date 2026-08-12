use shacs_core::runtime::{
    ActivationDigestObservation, ActivationMutation, ActivationMutationRequest, ActivationReason,
    ActivationRecord, ActivationRecordInput, ActivationSource, ActivationStatus, ActivationStore,
    ActivationStoreError, WorkspaceTrustRef,
};
use std::error::Error;

fn record(status: ActivationStatus, reason: ActivationReason) -> ActivationRecord {
    ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:skill:formatter:v1".to_owned(),
        source: ActivationSource::TrustedWorkspace,
        workspace_trust_ref: WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        resource_ref: "resource:skill:formatter".to_owned(),
        source_identity: "source:project:.shacs/skills/formatter".to_owned(),
        content_digest: "sha256:content-a".to_owned(),
        dependency_manifest_digest: "sha256:deps-a".to_owned(),
        status,
        reason,
        recorded_at_unix_ms: 31_004,
    })
}

#[test]
fn state_matrix_round_trips_without_overloading_discovery_activation() -> Result<(), Box<dyn Error>>
{
    for (status, reason) in [
        (ActivationStatus::Active, ActivationReason::Activated),
        (
            ActivationStatus::Stale,
            ActivationReason::ContentDigestMismatch,
        ),
        (ActivationStatus::Disabled, ActivationReason::UserDisabled),
        (ActivationStatus::Revoked, ActivationReason::UserRevoked),
        (ActivationStatus::Removed, ActivationReason::SourceRemoved),
    ] {
        // Given
        let workspace = tempfile::tempdir()?;
        let store = ActivationStore::new(workspace.path().join("activation-records.json"));
        let expected = record(status, reason);

        // When
        store.put(expected.clone())?;

        // Then
        assert_eq!(
            store.inspect(expected.activation_ref(), expected.workspace_trust_ref())?,
            expected
        );
    }
    Ok(())
}

#[test]
fn legacy_schema_migrates_identity_status_and_reason() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let path = workspace.path().join("activation-records.json");
    std::fs::write(&path, include_str!("fixtures/spec031_activation_v0.json"))?;

    // When
    let store = ActivationStore::new(&path);
    let migrated = store.inspect(
        "activation:skill:formatter:v1",
        &WorkspaceTrustRef::new("workspace:sha256:owner-a"),
    )?;

    // Then
    assert_eq!(migrated.resource_ref(), "resource:skill:formatter");
    assert_eq!(migrated.status(), ActivationStatus::Disabled);
    assert_eq!(migrated.reason(), ActivationReason::UserDisabled);
    assert_eq!(migrated.schema_version(), 1);
    Ok(())
}

#[test]
fn future_schema_is_rejected_without_mutation() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let path = workspace.path().join("activation-records.json");
    let original = r#"{"schemaVersion":2,"records":[],"receipts":[]}"#;
    std::fs::write(&path, original)?;

    // When
    let result = ActivationStore::new(&path).inspect(
        "activation:skill:formatter:v1",
        &WorkspaceTrustRef::new("workspace:sha256:owner-a"),
    );

    // Then
    assert!(matches!(
        result,
        Err(ActivationStoreError::UnknownSchema(2))
    ));
    assert_eq!(std::fs::read_to_string(path)?, original);
    Ok(())
}

#[test]
fn digest_mismatch_diagnostics_distinguish_content_and_dependencies() {
    // Given
    let activation = record(ActivationStatus::Active, ActivationReason::Activated);

    // When
    let diagnostics = activation.diagnose(&ActivationDigestObservation {
        content_digest: "sha256:content-b".to_owned(),
        dependency_manifest_digest: "sha256:deps-b".to_owned(),
    });

    // Then
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].activation_ref, activation.activation_ref());
    assert_eq!(
        diagnostics[0].reason,
        ActivationReason::ContentDigestMismatch
    );
    assert_eq!(
        diagnostics[1].reason,
        ActivationReason::DependencyManifestDigestMismatch
    );
}

#[test]
fn owner_safe_mutations_return_historical_receipts() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let store = ActivationStore::new(workspace.path().join("activation-records.json"));
    let activation = record(ActivationStatus::Active, ActivationReason::Activated);
    store.put(activation.clone())?;

    // When
    let disabled = store.mutate(ActivationMutationRequest {
        activation_ref: activation.activation_ref().to_owned(),
        workspace_trust_ref: activation.workspace_trust_ref().clone(),
        mutation: ActivationMutation::Disable,
        occurred_at_unix_ms: 31_005,
    })?;
    let revoked = store.mutate(ActivationMutationRequest {
        activation_ref: activation.activation_ref().to_owned(),
        workspace_trust_ref: activation.workspace_trust_ref().clone(),
        mutation: ActivationMutation::Revoke,
        occurred_at_unix_ms: 31_006,
    })?;

    // Then
    assert_eq!(disabled.previous_status, ActivationStatus::Active);
    assert_eq!(disabled.current_status, ActivationStatus::Disabled);
    assert_eq!(revoked.previous_status, ActivationStatus::Disabled);
    assert_eq!(revoked.current_status, ActivationStatus::Revoked);
    assert_ne!(disabled.receipt_ref, revoked.receipt_ref);
    assert_eq!(store.receipts(activation.activation_ref())?.len(), 2);
    let foreign = store.inspect(
        activation.activation_ref(),
        &WorkspaceTrustRef::new("workspace:sha256:owner-b"),
    );
    assert!(matches!(foreign, Err(ActivationStoreError::OwnerMismatch)));
    Ok(())
}

#[test]
fn terminal_states_reject_mutation_without_receipt() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let store = ActivationStore::new(workspace.path().join("activation-records.json"));
    let activation = record(ActivationStatus::Revoked, ActivationReason::UserRevoked);
    store.put(activation.clone())?;

    // When
    let result = store.mutate(ActivationMutationRequest {
        activation_ref: activation.activation_ref().to_owned(),
        workspace_trust_ref: activation.workspace_trust_ref().clone(),
        mutation: ActivationMutation::Disable,
        occurred_at_unix_ms: 31_007,
    });

    // Then
    assert!(matches!(
        result,
        Err(ActivationStoreError::InvalidTransition { .. })
    ));
    assert!(store.receipts(activation.activation_ref())?.is_empty());
    Ok(())
}

#[test]
fn put_rejects_cross_owner_and_source_collision_without_mutation() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let store = ActivationStore::new(workspace.path().join("activation-records.json"));
    let original = record(ActivationStatus::Active, ActivationReason::Activated);
    store.put(original.clone())?;
    let foreign_owner = record_with_identity(
        WorkspaceTrustRef::new("workspace:sha256:owner-b"),
        "source:project:.shacs/skills/formatter",
    );
    let foreign_source = record_with_identity(
        WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        "source:package:formatter",
    );

    // When
    let owner_result = store.put(foreign_owner);
    let source_result = store.put(foreign_source);

    // Then
    assert!(matches!(
        owner_result,
        Err(ActivationStoreError::OwnerMismatch)
    ));
    assert!(matches!(
        source_result,
        Err(ActivationStoreError::SourceMismatch)
    ));
    assert_eq!(
        store.inspect(original.activation_ref(), original.workspace_trust_ref())?,
        original
    );
    Ok(())
}

fn record_with_identity(owner: WorkspaceTrustRef, source_identity: &str) -> ActivationRecord {
    ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:skill:formatter:v1".to_owned(),
        source: ActivationSource::TrustedWorkspace,
        workspace_trust_ref: owner,
        resource_ref: "resource:skill:formatter".to_owned(),
        source_identity: source_identity.to_owned(),
        content_digest: "sha256:content-b".to_owned(),
        dependency_manifest_digest: "sha256:deps-b".to_owned(),
        status: ActivationStatus::Disabled,
        reason: ActivationReason::UserDisabled,
        recorded_at_unix_ms: 31_008,
    })
}
