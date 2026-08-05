use super::{
    Spec031ApprovalState, Spec031Availability, Spec031Count, Spec031InclusionReason,
    Spec031ProgressDelivery, Spec031SafeSummary,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "details",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Spec031Capability {
    Session(Spec031SessionCapability),
    Turn(Spec031TurnCapability),
    Subagent(Spec031SubagentCapability),
    Approval(Spec031ApprovalCapability),
    Tool(Spec031ToolCapability),
    Context(Spec031ContextCapability),
    Plugin(Spec031PluginCapability),
    App(Spec031AppCapability),
    Media(Spec031MediaCapability),
    Diagnostics(Spec031DiagnosticsCapability),
    ReleaseEvidence(Spec031ReleaseEvidenceCapability),
    Readiness(Spec031ReadinessCapability),
    Progress(Spec031ProgressCapability),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031SessionCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn_count: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031TurnCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031SubagentCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_count: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ApprovalCapability {
    pub state: Spec031ApprovalState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ToolCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_count: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ContextCapability {
    pub reason: Spec031InclusionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031PluginCapability {
    pub availability: Spec031Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031AppCapability {
    pub availability: Spec031Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031MediaCapability {
    pub reason: Spec031InclusionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031DiagnosticsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_count: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ReleaseEvidenceCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_count: Option<Spec031Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ReadinessCapability {
    pub availability: Spec031Availability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_count: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_capacity: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<Spec031SafeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ProgressCapability {
    pub delivery: Spec031ProgressDelivery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_capacity: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitted: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesced: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_generation: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_gap: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slow_consumer: Option<Spec031Count>,
}

impl Spec031ProgressCapability {
    pub const fn delivery(delivery: Spec031ProgressDelivery) -> Self {
        Self {
            delivery,
            queue_depth: None,
            queue_capacity: None,
            accepted: None,
            emitted: None,
            coalesced: None,
            dropped: None,
            reconnect_generation: None,
            reconnect_gap: None,
            slow_consumer: None,
        }
    }
}
