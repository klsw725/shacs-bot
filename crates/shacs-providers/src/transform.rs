use crate::model::ModelInfo;
use crate::types::GenerationSettings;
use serde_json::Value;

pub trait RequestTransform: Send + Sync {
    fn transform_messages(&self, _model: &ModelInfo, messages: Vec<Value>) -> Vec<Value> {
        messages
    }

    fn transform_options(
        &self,
        _model: &ModelInfo,
        options: GenerationSettings,
    ) -> GenerationSettings {
        options
    }
}

pub trait ToolSchemaTransform: Send + Sync {
    fn transform_tool_schema(&self, _model: &ModelInfo, schema: Value) -> Value {
        schema
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityTransform;

impl RequestTransform for IdentityTransform {}

impl ToolSchemaTransform for IdentityTransform {}
