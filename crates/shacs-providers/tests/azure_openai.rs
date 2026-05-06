use serde_json::json;
use shacs_providers::{
    build_azure_openai_headers, build_azure_openai_responses_request,
    resolve_azure_openai_api_base, AzureOpenAiClient, GenerationSettings,
    OpenAiCompatibleRequestParts, OpenAiHttpResponse, ProviderClient, ProviderConfig,
    ProviderEvent, ProviderRequest,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

#[test]
fn azure_openai_base_url_targets_openai_v1_responses() -> Result<(), Box<dyn Error>> {
    let config = ProviderConfig {
        api_base: Some(" https://example-resource.openai.azure.com/ ".to_owned()),
        ..ProviderConfig::default()
    };
    if resolve_azure_openai_api_base(&config)?
        != "https://example-resource.openai.azure.com/openai/v1/"
    {
        return Err("Azure OpenAI base URL normalization drifted".into());
    }

    let missing = resolve_azure_openai_api_base(&ProviderConfig::default())
        .expect_err("missing api_base should fail");
    if !missing
        .to_string()
        .contains("Azure OpenAI api_base is required")
    {
        return Err(format!("unexpected missing base error: {missing}").into());
    }
    Ok(())
}

#[test]
fn azure_openai_builder_uses_responses_api_headers_and_body() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "gpt-5.2-chat".to_owned(),
        messages: vec![
            json!({"role": "system", "content": "be brief"}),
            json!({"role": "user", "content": "hi"}),
        ],
        tools: Vec::new(),
        settings: GenerationSettings {
            temperature: 0.9,
            max_tokens: 0,
            reasoning_effort: Some("medium".to_owned()),
        },
        tool_choice: None,
    };
    let config = ProviderConfig {
        api_key: Some("azure-key".to_owned()),
        extra_headers: Some(BTreeMap::from([("X-Test".to_owned(), "yes".to_owned())])),
        ..ProviderConfig::default()
    };

    let parts = build_azure_openai_responses_request(&request, &config, true, "affinity-test");
    if parts.path != "/responses"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer azure-key")
        || parts.headers.get("x-session-affinity").map(String::as_str) != Some("affinity-test")
        || parts.headers.get("X-Test").map(String::as_str) != Some("yes")
        || parts.body["model"] != "gpt-5.2-chat"
        || parts.body["instructions"] != "be brief"
        || parts.body["max_output_tokens"] != 1
        || parts.body["stream"] != true
        || parts.body["reasoning"]["effort"] != "medium"
        || parts.body.get("temperature").is_some()
    {
        return Err(format!("unexpected Azure request parts: {parts:?}").into());
    }

    let override_headers = build_azure_openai_headers(
        &ProviderConfig {
            extra_headers: Some(BTreeMap::from([(
                "x-session-affinity".to_owned(),
                "user-affinity".to_owned(),
            )])),
            ..ProviderConfig::default()
        },
        "default-affinity",
    );
    if override_headers
        .get("x-session-affinity")
        .map(String::as_str)
        != Some("user-affinity")
    {
        return Err("user header should override generated session affinity".into());
    }
    Ok(())
}

#[test]
fn azure_openai_client_posts_responses_request_and_parses_success() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = AzureOpenAiClient::with_session_affinity(
        ProviderConfig {
            api_key: Some("azure-key".to_owned()),
            ..ProviderConfig::default()
        },
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({
                    "status": "completed",
                    "output": [{"type": "message", "content": [{"type": "output_text", "text": "azure ok"}]}],
                    "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}
                }),
            })
        },
        "affinity-test",
    );

    let response = client.chat(ProviderRequest {
        model: "deployment-a".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    if response.content.as_deref() != Some("azure ok")
        || response.usage.get("total_tokens") != Some(&5)
        || captured.first().map(|request| request.path.as_str()) != Some("/responses")
        || captured
            .first()
            .and_then(|request| request.headers.get("x-session-affinity"))
            .map(String::as_str)
            != Some("affinity-test")
    {
        return Err(format!(
            "Azure client success path drifted: response={response:?} captured={captured:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn azure_openai_client_maps_non_success_response_to_error() -> Result<(), Box<dyn Error>> {
    let client = AzureOpenAiClient::new(
        ProviderConfig::default(),
        |_request: OpenAiCompatibleRequestParts| {
            Ok(OpenAiHttpResponse {
                status: 429,
                headers: BTreeMap::from([
                    ("retry-after-ms".to_owned(), "2500".to_owned()),
                    ("x-should-retry".to_owned(), "true".to_owned()),
                ]),
                body: json!({"error": {"message": "rate limited", "type": "rate_limit", "code": "too_many"}}),
            })
        },
    );
    let response = client.chat(ProviderRequest {
        model: "deployment-a".to_owned(),
        messages: Vec::new(),
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    if response.finish_reason != "error"
        || response.content.as_deref() != Some("Error: rate limited")
        || response.error_status_code != Some(429)
        || response.error_type.as_deref() != Some("rate_limit")
        || response.error_retry_after_s != Some(2.5)
        || response.error_should_retry != Some(true)
    {
        return Err(format!("Azure error mapping drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn azure_openai_streaming_uses_responses_sse_events() -> Result<(), Box<dyn Error>> {
    let client = AzureOpenAiClient::with_session_affinity(
        ProviderConfig::default(),
        AzureStreamTransport,
        "affinity-test",
    );
    let mut events = Vec::new();
    let response = client.chat_stream(
        ProviderRequest {
            model: "deployment-a".to_owned(),
            messages: Vec::new(),
            tools: Vec::new(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &mut |event| events.push(event),
    )?;
    if response.content.as_deref() != Some("hi")
        || !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::TextDelta { text } if text == "hi"))
        || !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Finish { .. }))
    {
        return Err(
            format!("Azure stream path drifted: response={response:?} events={events:?}").into(),
        );
    }
    Ok(())
}

struct AzureStreamTransport;

impl shacs_providers::OpenAiHttpTransport for AzureStreamTransport {
    fn post_json(
        &self,
        _request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpResponse, shacs_providers::ProviderError> {
        Ok(OpenAiHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({
                "status": "completed",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": "fallback"}]}]
            }),
        })
    }

    fn post_json_stream(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<shacs_providers::OpenAiHttpStreamResponse, shacs_providers::ProviderError> {
        if request.path != "/responses" || request.body["stream"] != true {
            return Err(shacs_providers::ProviderError::Api {
                status: None,
                message: format!("unexpected request: {request:?}"),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            });
        }
        Ok(shacs_providers::OpenAiHttpStreamResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
                "data: [DONE]\n\n",
            )
            .to_owned(),
        })
    }
}
