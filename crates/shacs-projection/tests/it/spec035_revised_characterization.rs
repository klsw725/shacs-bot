use serde_json::json;
use shacs_projection::{
    DataDisclosureProjection, DataSurface, ProcessAdapterKind, SandboxFallback,
    SandboxFilesystemPolicy, SandboxNetworkPolicy, SandboxStatus, SandboxStatusProjection,
    Spec030Availability, Spec031ApprovalState, Spec031ProgressCapability, Spec031ProgressDelivery,
    TraceDisclosureProjection, TraceStatus,
};

#[test]
fn spec031_shipped_approval_and_delivery_identifiers_remain_canonical() {
    // Given: the shipped Spec031 durable approval and delivery vocabularies.
    let approval = Spec031ApprovalState::RetryConsumed;
    let delivery = Spec031ProgressCapability::delivery(Spec031ProgressDelivery::FinalDelivered);

    // When: the adapter-facing values are serialized.
    let approval_json = serde_json::to_value(approval).expect("approval fixture serializes");
    let delivery_json = serde_json::to_value(delivery).expect("delivery fixture serializes");

    // Then: their existing identifiers remain unchanged.
    assert_eq!(approval_json, json!("retry_consumed"));
    assert_eq!(delivery_json["delivery"], "final_delivered");
    assert!(delivery_json.get("dropped").is_none());
}

#[test]
fn spec030_owner_facts_preserve_adapter_scope_fallback_and_raw_content_disclosure() {
    // Given: existing owner facts for an adapter-scoped sandbox and possible raw content.
    let sandbox = SandboxStatusProjection {
        availability: Spec030Availability::Degraded,
        status: SandboxStatus::Active,
        fallback: SandboxFallback::TrustedNativeFallback,
        applied_adapters: vec![ProcessAdapterKind::GenericExec],
        filesystem_policy: SandboxFilesystemPolicy::Applied,
        network_policy: SandboxNetworkPolicy::NotApplied,
    };
    let disclosure = DataDisclosureProjection {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Session, DataSurface::ToolOutput],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Disabled,
            preview: None,
        },
    };

    // When: the current owner facts cross their serialization boundary.
    let sandbox_json = serde_json::to_value(sandbox).expect("sandbox fixture serializes");
    let disclosure_json = serde_json::to_value(disclosure).expect("disclosure fixture serializes");

    // Then: scope, fallback, and raw-content possibility remain explicit.
    assert_eq!(sandbox_json["status"], "active");
    assert_eq!(sandbox_json["fallback"], "trustedNativeFallback");
    assert_eq!(sandbox_json["appliedAdapters"], json!(["genericExec"]));
    assert_eq!(disclosure_json["rawContentPossible"], true);
    assert_eq!(
        disclosure_json["surfaces"],
        json!(["session", "toolOutput"])
    );
}
