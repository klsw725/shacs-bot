#[path = "spec034_codex_media/adversarial.rs"]
mod adversarial;
#[path = "spec034_codex_media/bounds.rs"]
mod bounds;
#[path = "spec034_codex_media/cancellation.rs"]
mod cancellation;
#[path = "spec034_codex_media/request_policy.rs"]
mod request_policy;
#[path = "spec034_codex_media/support.rs"]
mod support;

use base64::Engine;
use serde_json::json;
use shacs_providers::{
    build_codex_responses_request, chat_completions_tool, find_by_name,
    image_generation_client_from_config, parse_codex_media_stream, GenerationSettings,
    ImageGenerationClient, ImageOperationRequest, ImageOperationResult, ProviderConfig,
    ProviderEvent, ProviderMediaLifecycleStatus, ProviderRequest,
};
use std::error::Error;
use support::{capturing_client, image_request, recorded_fixture};

#[test]
fn ordinary_chat_cannot_admit_native_image_generation() -> Result<(), Box<dyn Error>> {
    // Given
    let request = ProviderRequest {
        messages: vec![json!({"role": "user", "content": "draw"})],
        tools: vec![chat_completions_tool(
            "image_generate",
            "Generate an image",
            json!({"type": "object"}),
        )],
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
    let rendered = parts.body.to_string();
    assert!(!rendered.contains("\"type\":\"image_generation\""));
    assert_eq!(parts.body["tools"][0]["name"], "image_generate");
    assert_eq!(parts.body["tool_choice"], "auto");
    Ok(())
}

#[test]
fn codex_native_adapter_is_resolved_only_as_image_generation_capability(
) -> Result<(), Box<dyn Error>> {
    // Given
    let spec = find_by_name("openai_codex").ok_or("Codex provider missing")?;

    // When
    let client = image_generation_client_from_config(
        "openai_codex",
        ProviderConfig {
            api_key: Some("fixture-token".to_owned()),
            ..ProviderConfig::default()
        },
    );

    // Then
    assert!(spec.supports_image_generation);
    assert!(client.is_ok());
    Ok(())
}

#[test]
fn approved_image_adapter_constructs_native_request() -> Result<(), Box<dyn Error>> {
    // Given
    let (client, captured) = capturing_client(recorded_fixture());
    let mut events = Vec::new();

    // When
    let result =
        client.generate_image_with_observer(image_request(), &mut |event| events.push(event))?;

    // Then
    let requests = captured.lock().map_err(|error| error.to_string())?;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body["tools"][0]["type"], "image_generation");
    assert_eq!(requests[0].body["tool_choice"]["type"], "image_generation");
    assert_eq!(result.images.len(), 1);
    assert_eq!(result.images[0].bytes, b"final-image");
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::MediaLifecycle(observation)
            if observation.status() == ProviderMediaLifecycleStatus::Final
    )));
    Ok(())
}

#[test]
fn codex_operation_dispatch_routes_generate_to_native_adapter() -> Result<(), Box<dyn Error>> {
    let (client, captured) = capturing_client(recorded_fixture());

    let result =
        client.execute_image_operation(ImageOperationRequest::Generate(image_request()))?;

    match result {
        ImageOperationResult::Generate(result) if result.images.len() == 1 => {}
        other => return Err(format!("Codex generate dispatch drifted: {other:?}").into()),
    }
    if captured.lock().map_err(|error| error.to_string())?.len() != 1 {
        return Err("Codex generate dispatch did not invoke native transport once".into());
    }
    Ok(())
}

#[test]
fn recorded_image_events_never_become_text_or_serialized_payload() -> Result<(), Box<dyn Error>> {
    // Given
    let mut events = Vec::new();

    // When
    let response = parse_codex_media_stream(recorded_fixture(), "gpt-5.6", &mut |event| {
        events.push(event)
    })?;

    // Then
    assert!(response.content.is_none());
    assert_eq!(response.media_candidates.len(), 1);
    assert!(!events
        .iter()
        .any(|event| matches!(event, ProviderEvent::TextDelta { .. })));
    let statuses = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::MediaLifecycle(observation) => Some(observation.status()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![
            ProviderMediaLifecycleStatus::Started,
            ProviderMediaLifecycleStatus::Partial,
            ProviderMediaLifecycleStatus::Final,
        ]
    );
    let serialized = serde_json::to_string(&response)?;
    let debug = format!("{response:?} {events:?}");
    for forbidden in [
        "ZmluYWwtaW1hZ2U=",
        "cGFydGlhbA==",
        "final-image",
        "partial",
        "AKIAIOSFODNN7EXAMPLE",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized leak: {serialized}"
        );
        assert!(!debug.contains(forbidden), "debug leak: {debug}");
    }
    Ok(())
}

#[test]
fn malformed_final_payload_fails_without_echoing_raw_data() {
    // Given
    let raw = "not-valid-base64-secret";
    let body = format!(
        "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_bad\",\"status\":\"completed\",\"result\":\"{raw}\"}}}}\n\n"
    );

    // When
    let error = parse_codex_media_stream(&body, "gpt-5.6", &mut |_| {})
        .expect_err("malformed payload must fail");

    // Then
    assert!(!error.to_string().contains(raw));
}

#[test]
fn duplicate_and_stale_final_events_produce_one_candidate() -> Result<(), Box<dyn Error>> {
    // Given
    let final_data = base64::engine::general_purpose::STANDARD.encode(b"one-image");
    let body = format!(
        concat!(
            "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"sequence_number\":10,\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_dedupe\"}}}}\n\n",
            "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":12,\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_dedupe\",\"status\":\"completed\",\"result\":\"{final_data}\"}}}}\n\n",
            "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":11,\"item\":{{\"type\":\"image_generation_call\",\"id\":\"ig_dedupe\",\"status\":\"completed\",\"result\":\"{final_data}\"}}}}\n\n",
            "event: response.completed\ndata: {{\"type\":\"response.completed\",\"sequence_number\":13,\"response\":{{\"status\":\"completed\",\"output\":[{{\"type\":\"image_generation_call\",\"id\":\"ig_dedupe\",\"status\":\"completed\",\"result\":\"{final_data}\"}}]}}}}\n\n"
        ),
        final_data = final_data,
    );
    let mut events = Vec::new();

    // When
    let response = parse_codex_media_stream(&body, "gpt-5.6", &mut |event| events.push(event))?;

    // Then
    assert_eq!(response.media_candidates.len(), 1);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                ProviderEvent::MediaLifecycle(observation)
                    if observation.status() == ProviderMediaLifecycleStatus::Final
            ))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn completed_without_final_payload_is_not_success() {
    // Given
    let body = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_empty\"}}\n\n",
        "event: response.image_generation_call.completed\ndata: {\"type\":\"response.image_generation_call.completed\",\"item_id\":\"ig_empty\",\"sequence_number\":1}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n",
    );

    // When
    let result = parse_codex_media_stream(body, "gpt-5.6", &mut |_| {});

    // Then
    assert!(result.is_err());
}

#[test]
fn failed_image_lineage_emits_payload_free_failure() -> Result<(), Box<dyn Error>> {
    // Given
    let body = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"image_generation_call\",\"id\":\"AKIAIOSFODNN7EXAMPLE\"}}\n\n",
        "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"provider raw secret\"}}}\n\n",
    );
    let mut events = Vec::new();

    // When
    let error = parse_codex_media_stream(body, "gpt-5.6", &mut |event| events.push(event))
        .expect_err("failed lineage must fail");

    // Then
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::MediaLifecycle(observation)
            if observation.status() == ProviderMediaLifecycleStatus::Failed
    )));
    let failed_id = events.iter().find_map(|event| match event {
        ProviderEvent::MediaLifecycle(observation)
            if observation.status() == ProviderMediaLifecycleStatus::Failed =>
        {
            Some(observation.candidate_id().as_str())
        }
        _ => None,
    });
    assert!(failed_id.is_some_and(|id| id.starts_with("item_sha256_")));
    assert_ne!(failed_id, Some("AKIAIOSFODNN7EXAMPLE"));
    assert!(!error.to_string().contains("provider raw secret"));
    assert!(!format!("{events:?}").contains("provider raw secret"));
    assert!(!format!("{events:?}").contains("AKIAIOSFODNN7EXAMPLE"));
    Ok(())
}
