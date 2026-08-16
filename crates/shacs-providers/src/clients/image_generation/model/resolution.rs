use super::{
    DefaultModelImageGenerationClient, ImageGenerationCapability, ImageGenerationClient,
    OpenAiImageGenerationClient, OpenRouterImageGenerationClient, ResolvedImageGenerationClient,
    UreqImageGenerationHttpTransport, DEFAULT_OPENAI_IMAGE_GENERATION_BASE,
    DEFAULT_OPENROUTER_IMAGE_GENERATION_BASE, IMAGE_GENERATION_CAPABILITY,
    OPENAI_IMAGE_GENERATION_DEFAULT_MODEL, OPENROUTER_IMAGE_GENERATION_DEFAULT_MODEL,
};
use crate::config::{ProviderConfig, ProvidersConfig};
use crate::error::ProviderError;
use crate::registry::{ProviderRegistry, ProviderSpec};
use std::env;

pub fn resolve_image_generation_client(
    registry: &ProviderRegistry,
    requested_provider: &str,
    model: &str,
    providers: &ProvidersConfig,
) -> Result<ResolvedImageGenerationClient, ProviderError> {
    let spec = if requested_provider == "auto" {
        resolve_auto_image_generation_provider(registry, providers)
            .ok_or_else(|| unsupported_image_generation(requested_provider))?
    } else {
        registry.find_by_name(requested_provider).ok_or_else(|| {
            ProviderError::ProviderNotFound {
                provider_id: requested_provider.to_owned(),
                suggestions: registry
                    .specs()
                    .iter()
                    .map(|spec| spec.name.to_owned())
                    .collect(),
            }
        })?
    };
    ensure_image_generation_supported(spec)?;
    let config = providers
        .get(spec.name)
        .cloned()
        .ok_or_else(|| ProviderError::AuthRequired {
            provider_id: spec.name.to_owned(),
        })?;
    let selected_model = default_image_generation_model(spec, model);
    let client = image_generation_client_from_config(spec.name, config)?;
    Ok(ResolvedImageGenerationClient {
        provider_id: spec.name.to_owned(),
        model: selected_model.to_owned(),
        client: Box::new(DefaultModelImageGenerationClient::new(
            selected_model,
            client,
        )),
    })
}

pub fn image_generation_client_from_config(
    provider_id: &str,
    config: ProviderConfig,
) -> Result<Box<dyn ImageGenerationClient>, ProviderError> {
    match provider_id {
        "openai" => Ok(Box::new(openai_image_generation_client_from_config(
            config,
        )?)),
        "openrouter" => Ok(Box::new(openrouter_image_generation_client_from_config(
            config,
        )?)),
        "openai_codex" => {
            let spec = crate::registry::find_by_name("openai_codex")
                .ok_or_else(|| unsupported_image_generation("openai_codex"))?;
            Ok(Box::new(crate::clients::codex::codex_client_from_config(
                config, spec,
            )?))
        }
        other => Err(unsupported_image_generation(other)),
    }
}

pub fn openai_image_generation_client_from_config(
    config: ProviderConfig,
) -> Result<OpenAiImageGenerationClient<UreqImageGenerationHttpTransport>, ProviderError> {
    let api_key = api_key_from_config_or_env(&config, "OPENAI_API_KEY", "openai")?;
    let api_base = resolve_image_generation_api_base(
        config.api_base.as_deref(),
        env::var("OPENAI_IMAGE_GENERATION_BASE_URL").ok().as_deref(),
        DEFAULT_OPENAI_IMAGE_GENERATION_BASE,
    );
    Ok(OpenAiImageGenerationClient::new(
        api_key,
        api_base.clone(),
        OPENAI_IMAGE_GENERATION_DEFAULT_MODEL,
        UreqImageGenerationHttpTransport::new(api_base),
    ))
}

pub fn openai_image_generation_capability() -> ImageGenerationCapability {
    ImageGenerationCapability {
        provider_id: "openai".to_owned(),
        supported_actions: vec![
            "text_to_image".to_owned(),
            "edit".to_owned(),
            "mask".to_owned(),
        ],
        supported_formats: vec!["png".to_owned(), "jpeg".to_owned(), "webp".to_owned()],
        supported_size_policy: "provider_defined".to_owned(),
        default_model: OPENAI_IMAGE_GENERATION_DEFAULT_MODEL.to_owned(),
    }
}

pub fn openrouter_image_generation_client_from_config(
    config: ProviderConfig,
) -> Result<OpenRouterImageGenerationClient<UreqImageGenerationHttpTransport>, ProviderError> {
    let api_key = api_key_from_config_or_env(&config, "OPENROUTER_API_KEY", "openrouter")?;
    let api_base = resolve_image_generation_api_base(
        config.api_base.as_deref(),
        env::var("OPENROUTER_IMAGE_GENERATION_BASE_URL")
            .ok()
            .as_deref(),
        DEFAULT_OPENROUTER_IMAGE_GENERATION_BASE,
    );
    Ok(OpenRouterImageGenerationClient::new(
        api_key,
        api_base.clone(),
        OPENROUTER_IMAGE_GENERATION_DEFAULT_MODEL,
        UreqImageGenerationHttpTransport::new(api_base),
    ))
}

pub fn resolve_image_generation_api_base(
    configured: Option<&str>,
    env_override: Option<&str>,
    default_base: &str,
) -> String {
    configured
        .or(env_override)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base)
        .trim_end_matches('/')
        .to_owned()
}

fn default_image_generation_model<'a>(spec: &ProviderSpec, model: &'a str) -> &'a str {
    match (spec.name, super::non_empty_model(model)) {
        ("openrouter", None | Some(OPENAI_IMAGE_GENERATION_DEFAULT_MODEL)) => {
            OPENROUTER_IMAGE_GENERATION_DEFAULT_MODEL
        }
        (_, Some(model)) => model,
        _ => OPENAI_IMAGE_GENERATION_DEFAULT_MODEL,
    }
}

fn resolve_auto_image_generation_provider<'a>(
    registry: &'a ProviderRegistry,
    providers: &ProvidersConfig,
) -> Option<&'a ProviderSpec> {
    registry
        .find_by_name("openai")
        .filter(|spec| spec.supports_image_generation && providers.contains_key(spec.name))
        .or_else(|| {
            registry
                .specs()
                .iter()
                .find(|spec| spec.supports_image_generation && providers.contains_key(spec.name))
        })
}

fn ensure_image_generation_supported(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.supports_image_generation {
        return Ok(());
    }
    Err(unsupported_image_generation(spec.name))
}

fn unsupported_image_generation(provider_id: &str) -> ProviderError {
    ProviderError::UnsupportedCapability {
        provider_id: provider_id.to_owned(),
        capability: IMAGE_GENERATION_CAPABILITY.to_owned(),
    }
}

fn api_key_from_config_or_env(
    config: &ProviderConfig,
    env_key: &str,
    provider_id: &str,
) -> Result<String, ProviderError> {
    config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            env::var(env_key)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| ProviderError::AuthRequired {
            provider_id: provider_id.to_owned(),
        })
}
