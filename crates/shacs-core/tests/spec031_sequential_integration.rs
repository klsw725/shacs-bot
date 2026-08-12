use serde_json::json;
use shacs_core::runtime::{
    ActivationReason, ActivationRecord, ActivationRecordInput, ActivationReplay, ActivationSource,
    ActivationStatus, ActivationStore, ExecutionSnapshot, ExecutionSnapshotInput,
    ResourceIdentitySnapshot, WorkspaceTrustRef,
};
use std::error::Error;
use std::fs;

#[path = "spec031_activation_execution/support.rs"]
mod support;

#[test]
fn migration_layout_live_context_activation_and_replay_form_one_flow() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "agents": {"defaults": {"sessionTtlMinutes": 5}},
            "profiles": {"selection": {"context": "default"}}
        }))?,
    )?;
    let migration = shacs_config::apply_config_migration(&config_path)?;
    let bundle = shacs_config::load_config_with_env(
        shacs_config::LoadOptions {
            config_path: Some(config_path),
            workspace_override: Some(root.path().join("workspace")),
            resolve_env: false,
            write_back_migrations: false,
        },
        &std::collections::BTreeMap::<String, String>::new(),
    )?;
    let layout = shacs_config::runtime_layout(&bundle.context);
    let activation = ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:skill:formatter:v1".to_owned(),
        source: ActivationSource::TrustedWorkspace,
        workspace_trust_ref: WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        resource_ref: "resource:skill:formatter".to_owned(),
        source_identity: "source:project:formatter".to_owned(),
        content_digest: "sha256:content-a".to_owned(),
        dependency_manifest_digest: "sha256:deps-a".to_owned(),
        status: ActivationStatus::Active,
        reason: ActivationReason::Activated,
        recorded_at_unix_ms: 31_005,
    });
    let store = ActivationStore::new(root.path().join("activations.json"));
    store.put(activation.clone())?;

    // When
    let mut input: ExecutionSnapshotInput = support::input("execution:integrated", 31_006);
    input.selected_resources = vec![ResourceIdentitySnapshot {
        identity: activation.resource_ref().to_owned(),
        content_digest: Some(activation.content_digest().to_owned()),
        activation_ref: None,
    }];
    input.attach_activation_refs(std::slice::from_ref(&activation));
    let snapshot = ExecutionSnapshot::create(input)?;
    let replay = ActivationReplay::diagnostic(&snapshot);

    // Then
    assert!(migration.changed);
    assert!(layout.iter().any(|entry| entry.name == "snapshots"));
    assert_eq!(snapshot.context_sources[0].source_ref, "context:system");
    assert_eq!(replay.activation_refs(), ["activation:skill:formatter:v1"]);
    assert_eq!(replay.counters().total(), 0);
    Ok(())
}
