use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub tool_call: bool,
    pub reasoning: bool,
    pub temperature: bool,
    pub attachment: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelModalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelLimits {
    pub context: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider_id: String,
    pub api_id: String,
    pub capabilities: ModelCapabilities,
    pub modalities: ModelModalities,
    pub limits: ModelLimits,
    pub cost: Option<ModelCost>,
    #[serde(default)]
    pub options: Value,
}
