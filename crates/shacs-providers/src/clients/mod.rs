pub mod anthropic;
pub mod azure_openai;
pub mod codex;
pub mod codex_image_generation;
pub mod image_generation;
mod image_generation_contract;
pub mod image_operations;
pub mod openai_compatible;
pub mod sse;
pub mod transcription;

use crate::config::{AgentDefaults, ProviderConfig, ProvidersConfig};
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderRequest};
use crate::registry::{ProviderRegistry, ProviderSpec};
use crate::types::GenerationSettings;
use anthropic::anthropic_client_from_config;
use azure_openai::azure_openai_client_from_config;
use codex::codex_client_from_config;
use openai_compatible::openai_compatible_client_from_config;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct ResolvedProviderClient {
    pub provider_id: String,
    pub model: String,
    pub client: Box<dyn ProviderClient>,
}

pub fn resolve_provider_client(
    registry: &ProviderRegistry,
    requested_provider: &str,
    model: &str,
    providers: &ProvidersConfig,
) -> Result<ResolvedProviderClient, ProviderError> {
    let provider_match = registry
        .match_provider(requested_provider, model, providers)
        .ok_or_else(|| provider_not_found(registry, requested_provider))?;
    let spec = registry
        .find_by_name(&provider_match.provider_id)
        .ok_or_else(|| provider_not_found(registry, &provider_match.provider_id))?;
    let config = providers
        .get(&provider_match.provider_id)
        .cloned()
        .ok_or_else(|| ProviderError::AuthRequired {
            provider_id: provider_match.provider_id.clone(),
        })?;
    let client = provider_client_from_config(config, spec)?;
    Ok(ResolvedProviderClient {
        provider_id: provider_match.provider_id,
        model: provider_match.model,
        client,
    })
}

pub fn provider_client_from_config(
    config: ProviderConfig,
    spec: &ProviderSpec,
) -> Result<Box<dyn ProviderClient>, ProviderError> {
    validate_provider_config(&config, spec)?;
    match spec.backend {
        "anthropic" => Ok(Box::new(anthropic_client_from_config(config, spec)?)),
        "azure_openai" => Ok(Box::new(azure_openai_client_from_config(config, spec)?)),
        "openai_codex" => Ok(Box::new(codex_client_from_config(config, spec)?)),
        "openai_compat" => Ok(Box::new(openai_compatible_client_from_config(
            config, spec,
        )?)),
        backend => Err(ProviderError::Api {
            status: None,
            message: format!(
                "provider '{}' backend '{}' is not implemented",
                spec.name, backend
            ),
            retryable: false,
            headers: BTreeMap::new(),
            body: None,
        }),
    }
}

fn validate_provider_config(
    config: &ProviderConfig,
    spec: &ProviderSpec,
) -> Result<(), ProviderError> {
    if spec.backend == "openai_compat"
        && !(spec.is_oauth || spec.is_local || spec.is_direct)
        && config
            .api_key
            .as_deref()
            .map_or(true, |api_key| api_key.trim().is_empty())
    {
        return Err(ProviderError::AuthRequired {
            provider_id: spec.name.to_owned(),
        });
    }
    Ok(())
}

pub fn prepare_provider_request(
    resolved: &ResolvedProviderClient,
    messages: Vec<Value>,
    tools: Vec<Value>,
    defaults: &AgentDefaults,
    settings: Option<GenerationSettings>,
    tool_choice: Option<Value>,
) -> ProviderRequest {
    ProviderRequest {
        messages,
        tools,
        model: resolved.model.clone(),
        settings: settings.unwrap_or_else(|| GenerationSettings {
            temperature: defaults.temperature,
            max_tokens: defaults.max_tokens,
            reasoning_effort: defaults.reasoning_effort.clone(),
        }),
        tool_choice,
    }
}

fn provider_not_found(registry: &ProviderRegistry, provider_id: &str) -> ProviderError {
    ProviderError::ProviderNotFound {
        provider_id: provider_id.to_owned(),
        suggestions: registry
            .specs()
            .iter()
            .map(|spec| spec.name.to_owned())
            .collect(),
    }
}
