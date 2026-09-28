use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035EphemeralConfirmationState {
    ConfirmationRequired,
    ConfirmationAllowed,
    ConfirmationDenied,
    HeadlessConfirmationDenied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035HookDenialState {
    HookDenied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035SandboxRuntimeState {
    Active,
    Disabled,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035SandboxFallback {
    NativeFallback,
    NotApplicable,
    ExecutionDenied,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035AdapterScope {
    AdapterScoped,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035ContainmentGuarantee {
    NotGuaranteed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035DisclosureState {
    SafeSummary,
    RedactedForSurface,
    RawContentPossibleElsewhere,
    RemoteTraceOptIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035DisclosureBoundary {
    ProjectionSerializationOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035ProgressState {
    Live,
    Coalesced,
    Dropped,
    Reconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035FinalDeliveryState {
    FinalDelivered,
    FinalPending,
    FinalFailed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035OwnerSurface {
    Cli,
    Api,
    Websocket,
    Channel,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035DeliveryGuarantee {
    OwnerSurfaceObservationOnly,
}
