use shacs_providers::{
    resolve_image_generation_client, resolve_image_generation_provider,
    ImageGenerationResolutionRequest, ProviderConfig, ProviderError, ProviderRegistry,
    ProvidersConfig,
};
use std::error::Error;

#[test]
fn image_generation_resolver_rejects_unsupported_provider() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();
    let error =
        match resolve_image_generation_client(&registry, "anthropic", "gpt-image-2", &providers) {
            Ok(_) => return Err("unsupported provider unexpectedly resolved".into()),
            Err(error) => error,
        };
    match error {
        ProviderError::UnsupportedCapability {
            provider_id,
            capability,
        } if provider_id == "anthropic" && capability == "image_generation" => {}
        other => return Err(format!("unexpected unsupported provider error: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn image_generation_resolver_requires_configured_auth() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();
    let error =
        match resolve_image_generation_client(&registry, "openai", "gpt-image-2", &providers) {
            Ok(_) => return Err("missing provider config unexpectedly resolved".into()),
            Err(error) => error,
        };
    match error {
        ProviderError::AuthRequired { provider_id } if provider_id == "openai" => {}
        other => return Err(format!("unexpected missing auth error: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn image_generation_auto_without_config_selects_openai_for_auth_resolution(
) -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();

    let resolved = resolve_image_generation_provider(&ImageGenerationResolutionRequest {
        registry: &registry,
        requested_provider: "auto",
        model: "gpt-image-2",
        providers: &providers,
    })?;

    assert_eq!(resolved.provider_id, "openai");
    assert_eq!(resolved.model, "gpt-image-2");
    Ok(())
}

#[test]
fn image_generation_resolver_returns_selected_model() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("sk-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved =
        resolve_image_generation_client(&registry, "openai", "custom-image-model", &providers)?;
    if resolved.provider_id != "openai" || resolved.model != "custom-image-model" {
        return Err(format!(
            "unexpected resolver success result: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_resolver_returns_selected_openrouter_model() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved = resolve_image_generation_client(
        &registry,
        "openrouter",
        "google/gemini-2.5-flash-image-preview",
        &providers,
    )?;
    if resolved.provider_id != "openrouter"
        || resolved.model != "google/gemini-2.5-flash-image-preview"
    {
        return Err(format!(
            "unexpected OpenRouter resolver success result: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_resolver_maps_openrouter_openai_default_to_openrouter_default(
) -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved =
        resolve_image_generation_client(&registry, "openrouter", "gpt-image-2", &providers)?;
    if resolved.provider_id != "openrouter" || resolved.model != "openai/gpt-5.4-image-2" {
        return Err(format!(
            "OpenRouter default model mapping drifted: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_auto_prefers_openai_when_openrouter_is_also_configured(
) -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );
    providers.insert(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("sk-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved = resolve_image_generation_client(&registry, "auto", "gpt-image-2", &providers)?;
    if resolved.provider_id != "openai" || resolved.model != "gpt-image-2" {
        return Err(format!(
            "auto image resolver should prefer OpenAI: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_auto_uses_openrouter_when_openai_is_unconfigured() -> Result<(), Box<dyn Error>>
{
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved = resolve_image_generation_client(&registry, "auto", "gpt-image-2", &providers)?;
    if resolved.provider_id != "openrouter" || resolved.model != "openai/gpt-5.4-image-2" {
        return Err(format!(
            "auto image resolver should fallback to OpenRouter: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}
