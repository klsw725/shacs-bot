use crate::generated_media::GeneratedArtifactRef;
use crate::runtime::{RecentAutoModeDenial, RecentAutoModeRetryToken, RuntimeInterrupt, ToolEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub final_content: Option<String>,
    pub messages: Vec<Value>,
    pub tools_used: Vec<String>,
    pub usage: BTreeMap<String, u64>,
    pub stop_reason: String,
    pub error: Option<String>,
    pub error_message: Option<String>,
    pub interrupt: Option<RuntimeInterrupt>,
    pub tool_events: Vec<ToolEvent>,
    pub had_injections: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_artifacts: Vec<GeneratedArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_auto_mode_denials: Vec<RecentAutoModeDenial>,
    #[serde(skip, default)]
    pub recent_auto_mode_retry_tokens: Vec<RecentAutoModeRetryToken>,
}
