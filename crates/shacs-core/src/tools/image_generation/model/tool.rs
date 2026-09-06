use super::{
    ImageGenerateTool, IntegerSchema, JsonMap, SchemaFragment, StringSchema, Tool,
    ToolCallExecutionContext, ToolParameters, ToolResult, Value, ALLOWED_PARAMS,
    IMAGE_GENERATE_TOOL_NAME,
};
use std::collections::BTreeSet;

impl Tool for ImageGenerateTool {
    fn name(&self) -> &str {
        IMAGE_GENERATE_TOOL_NAME
    }
    fn description(&self) -> &str {
        "Generate images through the configured image generation provider and store local media artifact references."
    }
    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "prompt",
                StringSchema::new("Image generation prompt").min_length(1),
            )
            .property("size", StringSchema::new("Provider-supported image size"))
            .property(
                "quality",
                StringSchema::new("Provider-supported quality setting"),
            )
            .property(
                "format",
                StringSchema::new("Output image format").enum_values([
                    Value::String("png".to_owned()),
                    Value::String("jpeg".to_owned()),
                    Value::String("webp".to_owned()),
                ]),
            )
            .property(
                "background",
                StringSchema::new("Provider-supported background setting"),
            )
            .property(
                "count",
                IntegerSchema::new("Number of images to generate")
                    .minimum(1)
                    .maximum(i64::from(self.config.max_count)),
            )
            .required(["prompt"])
            .to_json_schema()
    }
    fn read_only(&self) -> bool {
        false
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        match self.execute_inner(params, None) {
            Ok(value) => ToolResult::Json(value),
            Err(error) => ToolResult::Text(error),
        }
    }
    fn execute_with_context(
        &self,
        params: JsonMap,
        context: &ToolCallExecutionContext,
    ) -> ToolResult {
        match self.execute_inner(params, context.provider_invocation()) {
            Ok(value) => ToolResult::Json(value),
            Err(error) => ToolResult::Text(error),
        }
    }
    fn validate_params(&self, params: &JsonMap) -> Vec<crate::tools::ValidationError> {
        let allowed: BTreeSet<&str> = ALLOWED_PARAMS.iter().copied().collect();
        let mut errors = super::super::super::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        );
        for key in params.keys() {
            if !allowed.contains(key.as_str()) {
                errors.push(crate::tools::ValidationError::new(
                    key,
                    "is not an allowed parameter",
                ));
            }
        }
        errors
    }
}
