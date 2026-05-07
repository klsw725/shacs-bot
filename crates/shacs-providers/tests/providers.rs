use serde_json::{json, Map, Value};
use shacs_providers::ProviderSpec;
use shacs_providers::{
    find_by_name, finish_reason_from_openai_responses, interpolate_env, prepare_provider_request,
    provider_client_from_config, provider_specs, resolve_azure_openai_api_base,
    resolve_provider_client, AgentDefaults, GenerationSettings, LlmResponse, ProviderConfig,
    ProviderError, ProviderRegistry, ProvidersConfig, ToolCallRequest,
};
use std::collections::BTreeMap;
use std::error::Error;

#[test]
fn provider_registry_preserves_nanobot_provider_order_and_aliases() -> Result<(), Box<dyn Error>> {
    let names = provider_specs()
        .iter()
        .map(|spec| spec.name)
        .collect::<Vec<_>>();
    if names.first() != Some(&"custom")
        || names.get(1) != Some(&"azure_openai")
        || names.last() != Some(&"qianfan")
        || names.len() != 30
    {
        return Err(format!("provider order drifted: {names:?}").into());
    }
    if find_by_name("github-copilot").map(|spec| spec.name) != Some("github_copilot")
        || find_by_name("azure.openai").map(|spec| spec.name) != Some("azure_openai")
        || find_by_name("OPENAI_CODEX").map(|spec| spec.name) != Some("openai_codex")
    {
        return Err("provider alias normalization drifted".into());
    }
    Ok(())
}

#[test]
fn provider_registry_matches_explicit_prefix_and_local_fallback() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("or-key".to_owned()),
            ..ProviderConfig::default()
        },
    );
    providers.insert(
        "ollama".to_owned(),
        ProviderConfig {
            api_base: Some("http://localhost:11434/ollama".to_owned()),
            ..ProviderConfig::default()
        },
    );

    let explicit = registry
        .match_provider("auto", "openrouter/anthropic/claude-opus", &providers)
        .ok_or("explicit provider prefix did not match")?;
    if explicit.provider_id != "openrouter" || explicit.model != "openrouter/anthropic/claude-opus"
    {
        return Err(format!("unexpected explicit match: {explicit:?}").into());
    }

    let local = registry
        .match_provider("auto", "ollama/llama3", &providers)
        .ok_or("local provider prefix did not match")?;
    if local.provider_id != "ollama" || local.model != "ollama/llama3" {
        return Err(format!("unexpected local match: {local:?}").into());
    }

    let local_fallback = registry
        .match_provider("auto", "unknown-model", &providers)
        .ok_or("local fallback provider did not match")?;
    if local_fallback.provider_id != "ollama" {
        return Err(format!("unexpected local fallback provider: {local_fallback:?}").into());
    }

    providers.remove("ollama");
    providers.insert(
        "lm_studio".to_owned(),
        ProviderConfig {
            api_base: Some("http://local-model-server/v1".to_owned()),
            ..ProviderConfig::default()
        },
    );
    let unmatched_local = registry
        .match_provider("auto", "unknown-model", &providers)
        .ok_or("configured local provider without keyword did not match")?;
    if unmatched_local.provider_id != "lm_studio" {
        return Err(format!(
            "configured local provider should fallback without keyword match: {unmatched_local:?}"
        )
        .into());
    }

    providers.remove("lm_studio");
    let fallback = registry
        .match_provider("auto", "unknown-model", &providers)
        .ok_or("api-key fallback provider did not match")?;
    if fallback.provider_id != "openrouter" {
        return Err(format!("unexpected fallback provider: {fallback:?}").into());
    }
    Ok(())
}

#[test]
fn provider_registry_preserves_key_nanobot_metadata() -> Result<(), Box<dyn Error>> {
    let custom = find_by_name("custom").ok_or("custom spec missing")?;
    let azure = find_by_name("azure_openai").ok_or("azure spec missing")?;
    let openai = find_by_name("openai").ok_or("openai spec missing")?;
    let openrouter = find_by_name("openrouter").ok_or("openrouter spec missing")?;
    let zhipu = find_by_name("zhipu").ok_or("zhipu spec missing")?;
    let xiaomi = find_by_name("xiaomi_mimo").ok_or("xiaomi spec missing")?;
    if !custom.is_direct
        || custom.env_key.is_some()
        || !azure.is_direct
        || azure.env_key.is_some()
        || openai.default_api_base != Some("https://api.openai.com/v1")
        || !openai.supports_max_completion_tokens
        || !openrouter.is_gateway
        || openrouter.detect_by_key_prefix != Some("sk-or-")
        || openrouter.default_api_base != Some("https://openrouter.ai/api/v1")
        || !openrouter.supports_prompt_caching
        || zhipu.env_key != Some("ZAI_API_KEY")
        || zhipu.env_extras != [("ZHIPUAI_API_KEY", "{api_key}")]
        || xiaomi.env_key != Some("XIAOMIMIMO_API_KEY")
    {
        return Err(format!(
            "provider metadata drifted: {custom:?} {azure:?} {openai:?} {openrouter:?} {zhipu:?} {xiaomi:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn provider_config_accepts_camel_and_snake_case_fields() -> Result<(), Box<dyn Error>> {
    let camel: ProviderConfig = serde_json::from_value(json!({
        "apiKey": "key",
        "apiBase": "https://api.example.test",
        "extraHeaders": {"X-Test": "yes"},
        "extraBody": {"metadata": true}
    }))?;
    let snake: ProviderConfig = serde_json::from_value(json!({
        "api_key": "key",
        "api_base": "https://api.example.test",
        "extra_headers": {"X-Test": "yes"},
        "extra_body": {"metadata": true}
    }))?;
    if camel != snake || camel.api_base_or(Some("fallback")) != Some("https://api.example.test") {
        return Err(format!("provider config aliases drifted: {camel:?} {snake:?}").into());
    }
    Ok(())
}

#[test]
fn provider_client_factory_builds_openai_compatible_client() -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("openai").ok_or("openai spec missing")?;
    let client = provider_client_from_config(
        ProviderConfig {
            api_key: Some("openai-key".to_owned()),
            ..ProviderConfig::default()
        },
        spec,
    )?;
    drop(client);
    Ok(())
}

#[test]
fn provider_client_factory_builds_anthropic_client() -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("anthropic").ok_or("anthropic spec missing")?;
    let client = provider_client_from_config(ProviderConfig::default(), spec)?;
    drop(client);
    Ok(())
}

#[test]
fn provider_client_factory_builds_codex_client() -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("openai_codex").ok_or("openai_codex spec missing")?;
    let client = provider_client_from_config(ProviderConfig::default(), spec)?;
    drop(client);
    Ok(())
}

#[test]
fn provider_client_factory_builds_github_copilot_client() -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("github_copilot").ok_or("github_copilot spec missing")?;
    let client = provider_client_from_config(
        ProviderConfig {
            api_key: Some("copilot-token".to_owned()),
            ..ProviderConfig::default()
        },
        spec,
    )?;
    drop(client);
    Ok(())
}

#[test]
fn provider_client_factory_builds_azure_openai_client() -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("azure_openai").ok_or("azure_openai spec missing")?;
    let config = ProviderConfig {
        api_key: Some("azure-key".to_owned()),
        api_base: Some("https://resource.openai.azure.com".to_owned()),
        ..ProviderConfig::default()
    };
    let client = provider_client_from_config(config.clone(), spec)?;
    if resolve_azure_openai_api_base(&config)? != "https://resource.openai.azure.com/openai/v1/" {
        return Err("Azure OpenAI base normalization drifted".into());
    }
    drop(client);
    Ok(())
}

#[test]
fn provider_client_factory_propagates_openai_compatible_config_errors() -> Result<(), Box<dyn Error>>
{
    let spec = find_by_name("custom").ok_or("custom spec missing")?;
    let error = match provider_client_from_config(ProviderConfig::default(), spec) {
        Ok(_) => return Err("custom without api_base should fail".into()),
        Err(error) => error,
    };
    if !error
        .to_string()
        .contains("missing OpenAI-compatible base URL for provider 'custom'")
    {
        return Err(format!("unexpected factory error: {error}").into());
    }
    Ok(())
}

#[test]
fn provider_client_factory_requires_api_key_for_standard_openai_compatible_providers(
) -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("openai").ok_or("openai spec missing")?;
    let error = match provider_client_from_config(ProviderConfig::default(), spec) {
        Ok(_) => return Err("standard OpenAI-compatible provider without key should fail".into()),
        Err(error) => error,
    };
    match error {
        ProviderError::AuthRequired { provider_id } if provider_id == "openai" => Ok(()),
        other => Err(format!("unexpected missing key error: {other:?}").into()),
    }
}

#[test]
fn provider_client_factory_exempts_direct_and_local_openai_compatible_providers(
) -> Result<(), Box<dyn Error>> {
    for provider in ["custom", "ollama", "ovms"] {
        let spec = find_by_name(provider).ok_or("spec missing")?;
        let client = provider_client_from_config(
            ProviderConfig {
                api_base: Some(
                    spec.default_api_base
                        .unwrap_or("https://custom.example.test/v1")
                        .to_owned(),
                ),
                ..ProviderConfig::default()
            },
            spec,
        )?;
        drop(client);
    }
    Ok(())
}

#[test]
fn provider_client_factory_exempts_oauth_openai_compatible_providers() -> Result<(), Box<dyn Error>>
{
    let spec = ProviderSpec {
        name: "oauth_openai_compat_test",
        keywords: &[],
        env_key: None,
        display_name: "OAuth OpenAI Compat Test",
        backend: "openai_compat",
        env_extras: &[],
        is_gateway: false,
        is_local: false,
        detect_by_key_prefix: None,
        detect_by_base_keyword: None,
        default_api_base: Some("https://oauth.example.test/v1"),
        strip_model_prefix: false,
        supports_max_completion_tokens: false,
        model_overrides: &[],
        is_oauth: true,
        is_direct: false,
        supports_prompt_caching: false,
        thinking_style: None,
        reasoning_as_content: false,
    };
    let client = provider_client_from_config(ProviderConfig::default(), &spec)?;
    drop(client);
    Ok(())
}

#[test]
fn provider_client_factory_rejects_unimplemented_backends() -> Result<(), Box<dyn Error>> {
    let spec = ProviderSpec {
        name: "unsupported_backend_test",
        keywords: &[],
        env_key: None,
        display_name: "Unsupported Backend Test",
        backend: "unsupported_backend",
        env_extras: &[],
        is_gateway: false,
        is_local: false,
        detect_by_key_prefix: None,
        detect_by_base_keyword: None,
        default_api_base: None,
        strip_model_prefix: false,
        supports_max_completion_tokens: false,
        model_overrides: &[],
        is_oauth: false,
        is_direct: false,
        supports_prompt_caching: false,
        thinking_style: None,
        reasoning_as_content: false,
    };
    let error = match provider_client_from_config(ProviderConfig::default(), &spec) {
        Ok(_) => return Err("unsupported backend should fail".into()),
        Err(error) => error,
    };
    let expected = format!(
        "provider '{}' backend '{}' is not implemented",
        spec.name, spec.backend
    );
    if !error.to_string().contains(&expected) {
        return Err(format!("unexpected unsupported backend error: {error}").into());
    }
    Ok(())
}

#[test]
fn provider_client_resolver_selects_provider_and_normalizes_model() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::from([(
        "aihubmix".to_owned(),
        ProviderConfig {
            api_key: Some("aihubmix-key".to_owned()),
            ..ProviderConfig::default()
        },
    )]);
    let resolved =
        resolve_provider_client(&registry, "auto", "aihubmix/openai/gpt-4o", &providers)?;
    if resolved.provider_id != "aihubmix" || resolved.model != "gpt-4o" {
        return Err(format!(
            "resolver selected unexpected provider/model: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    drop(resolved.client);
    Ok(())
}

#[test]
fn provider_client_resolver_rejects_missing_provider_match() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();
    let error = match resolve_provider_client(&registry, "auto", "unknown-model", &providers) {
        Ok(_) => return Err("unknown model without configured providers should fail".into()),
        Err(error) => error,
    };
    match error {
        ProviderError::ProviderNotFound {
            provider_id,
            suggestions,
        } if provider_id == "auto" && suggestions.iter().any(|name| name == "openai") => Ok(()),
        other => Err(format!("unexpected missing provider error: {other:?}").into()),
    }
}

#[test]
fn provider_client_resolver_rejects_missing_selected_config() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();
    let error = match resolve_provider_client(&registry, "openai", "gpt-5", &providers) {
        Ok(_) => return Err("explicit provider without config should fail".into()),
        Err(error) => error,
    };
    match error {
        ProviderError::AuthRequired { provider_id } if provider_id == "openai" => Ok(()),
        other => Err(format!("unexpected missing config error: {other:?}").into()),
    }
}

#[test]
fn provider_client_resolver_propagates_azure_openai_config_errors() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::from([(
        "azure_openai".to_owned(),
        ProviderConfig {
            api_key: Some("azure-key".to_owned()),
            ..ProviderConfig::default()
        },
    )]);
    let error = match resolve_provider_client(&registry, "azure_openai", "gpt-4", &providers) {
        Ok(_) => return Err("Azure without api_base should fail".into()),
        Err(error) => error,
    };
    if !error
        .to_string()
        .contains("Azure OpenAI api_base is required")
    {
        return Err(format!("unexpected Azure config error: {error}").into());
    }
    Ok(())
}

#[test]
fn provider_client_resolver_builds_azure_openai_client() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::from([(
        "azure_openai".to_owned(),
        ProviderConfig {
            api_key: Some("azure-key".to_owned()),
            api_base: Some("https://resource.openai.azure.com".to_owned()),
            ..ProviderConfig::default()
        },
    )]);
    let resolved = resolve_provider_client(&registry, "azure_openai", "gpt-5.2-chat", &providers)?;
    if resolved.provider_id != "azure_openai" || resolved.model != "gpt-5.2-chat" {
        return Err(format!(
            "unexpected Azure resolver result: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    drop(resolved.client);
    Ok(())
}

#[test]
fn provider_request_preparation_uses_resolved_model_and_agent_defaults(
) -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::from([(
        "aihubmix".to_owned(),
        ProviderConfig {
            api_key: Some("aihubmix-key".to_owned()),
            ..ProviderConfig::default()
        },
    )]);
    let resolved =
        resolve_provider_client(&registry, "auto", "aihubmix/openai/gpt-4o", &providers)?;
    let defaults = AgentDefaults {
        temperature: 0.2,
        max_tokens: 1234,
        reasoning_effort: Some("medium".to_owned()),
        ..AgentDefaults::default()
    };
    let messages = vec![json!({"role": "user", "content": "hi"})];
    let tools = vec![json!({"type": "function", "function": {"name": "search"}})];
    let tool_choice = Some(json!({"type": "function", "function": {"name": "search"}}));
    let request = prepare_provider_request(
        &resolved,
        messages.clone(),
        tools.clone(),
        &defaults,
        None,
        tool_choice.clone(),
    );
    if request.model != "gpt-4o"
        || request.messages != messages
        || request.tools != tools
        || request.tool_choice != tool_choice
        || request.settings.temperature != 0.2
        || request.settings.max_tokens != 1234
        || request.settings.reasoning_effort.as_deref() != Some("medium")
    {
        return Err(format!("provider request preparation drifted: {request:?}").into());
    }
    Ok(())
}

#[test]
fn provider_request_preparation_prefers_explicit_generation_settings() -> Result<(), Box<dyn Error>>
{
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::from([(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("openai-key".to_owned()),
            ..ProviderConfig::default()
        },
    )]);
    let resolved = resolve_provider_client(&registry, "openai", "gpt-5", &providers)?;
    let explicit = GenerationSettings {
        temperature: 0.9,
        max_tokens: 99,
        reasoning_effort: Some("high".to_owned()),
    };
    let request = prepare_provider_request(
        &resolved,
        Vec::new(),
        Vec::new(),
        &AgentDefaults::default(),
        Some(explicit.clone()),
        None,
    );
    if request.model != "gpt-5" || request.settings != explicit || request.tool_choice.is_some() {
        return Err(format!("explicit settings should win: {request:?}").into());
    }
    Ok(())
}

#[test]
fn agent_defaults_match_nanobot_schema_defaults() -> Result<(), Box<dyn Error>> {
    let defaults = AgentDefaults::default();
    if defaults.model != "anthropic/claude-opus-4-5"
        || defaults.provider != "auto"
        || defaults.max_tokens != 8192
        || defaults.temperature != 0.1
        || defaults.context_window_tokens != 65_536
        || defaults.max_tool_iterations != 200
        || defaults.max_tool_result_chars != 16_000
        || defaults.provider_retry_mode != "standard"
    {
        return Err(format!("AgentDefaults drifted from nanobot: {defaults:?}").into());
    }
    Ok(())
}

#[test]
fn env_interpolation_resolves_and_rejects_missing_vars() -> Result<(), Box<dyn Error>> {
    let env = BTreeMap::from([("TOKEN".to_owned(), "secret".to_owned())]);
    if interpolate_env("Bearer ${TOKEN}", &env)? != "Bearer secret" {
        return Err("env interpolation did not substitute value".into());
    }
    if interpolate_env("Bearer ${MISSING}", &env).is_ok() {
        return Err("env interpolation should reject missing variables".into());
    }
    Ok(())
}

#[test]
fn tool_call_request_serializes_openai_arguments_as_string() -> Result<(), Box<dyn Error>> {
    let mut arguments = Map::new();
    arguments.insert("query".to_owned(), Value::String("rust".to_owned()));
    let mut request = ToolCallRequest::new("call_1", "search", arguments);
    request.provider_specific_fields = Some(Map::from_iter([(
        "index".to_owned(),
        Value::Number(1.into()),
    )]));
    request.function_provider_specific_fields =
        Some(Map::from_iter([("strict".to_owned(), Value::Bool(true))]));

    if request.to_openai_tool_call()
        != json!({
            "id": "call_1",
            "type": "function",
            "provider_specific_fields": {"index": 1},
            "function": {
                "name": "search",
                "arguments": "{\"query\":\"rust\"}",
                "provider_specific_fields": {"strict": true}
            }
        })
    {
        return Err(format!(
            "OpenAI tool call shape drifted: {}",
            request.to_openai_tool_call()
        )
        .into());
    }
    Ok(())
}

#[test]
fn llm_response_should_execute_tools_matches_nanobot_contract() -> Result<(), Box<dyn Error>> {
    let tool_call = ToolCallRequest::new("call", "search", Map::new());
    for finish_reason in ["tool_calls", "stop"] {
        let response = LlmResponse {
            finish_reason: finish_reason.to_owned(),
            tool_calls: vec![tool_call.clone()],
            ..LlmResponse::default()
        };
        if !response.should_execute_tools() {
            return Err(format!("{finish_reason} should execute tools").into());
        }
    }
    for finish_reason in [
        "error",
        "length",
        "refusal",
        "content_filter",
        "",
        "function_call",
    ] {
        let response = LlmResponse {
            finish_reason: finish_reason.to_owned(),
            tool_calls: vec![tool_call.clone()],
            ..LlmResponse::default()
        };
        if response.should_execute_tools() {
            return Err(format!("{finish_reason} should not execute tools").into());
        }
    }
    Ok(())
}

#[test]
fn openai_responses_finish_reason_mapping_matches_nanobot() -> Result<(), Box<dyn Error>> {
    let cases = [
        (Some("completed"), "stop"),
        (Some("incomplete"), "length"),
        (Some("failed"), "error"),
        (Some("cancelled"), "error"),
        (Some("unknown"), "stop"),
        (None, "stop"),
    ];
    for (status, expected) in cases {
        if finish_reason_from_openai_responses(status) != expected {
            return Err(format!("status {status:?} did not map to {expected}").into());
        }
    }
    Ok(())
}
