use serde_json::json;
use shacs_providers::{
    build_codex_responses_request, GenerationSettings, ProviderConfig, ProviderRequest,
};
use std::error::Error;

#[test]
fn ordinary_chat_without_function_tools_strips_native_tool_extra_body() -> Result<(), Box<dyn Error>>
{
    // Given
    let request = ProviderRequest {
        messages: vec![json!({"role": "user", "content": "draw"})],
        tools: Vec::new(),
        model: "openai_codex/gpt-5.6".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    };
    let config = ProviderConfig {
        extra_body: Some(
            json!({
                "tools": [{"type": "image_generation"}],
                "tool_choice": {"type": "image_generation"}
            })
            .as_object()
            .ok_or("extra body must be an object")?
            .clone(),
        ),
        ..ProviderConfig::default()
    };

    // When
    let parts = build_codex_responses_request(&request, &config);

    // Then
    assert!(parts.body.get("tools").is_none(), "body: {}", parts.body);
    assert_eq!(parts.body["tool_choice"], "auto");
    Ok(())
}
