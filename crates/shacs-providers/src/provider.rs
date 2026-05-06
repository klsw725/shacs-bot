use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::model::ModelInfo;
use crate::types::{GenerationSettings, LlmResponse};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        delta: String,
    },
    ToolCallReady {
        id: String,
        name: String,
        input: Value,
    },
    Finish {
        usage: Value,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRequest {
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub model: String,
    pub settings: GenerationSettings,
    pub tool_choice: Option<Value>,
}

pub trait ProviderClient: Send + Sync {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError>;
    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError>;
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn config(&self) -> &ProviderConfig;
    fn default_model(&self) -> &str;
    fn supports_progress_deltas(&self) -> bool {
        false
    }
    fn model_info(&self, model: &str) -> Option<ModelInfo>;
}
