use shacs_providers::{
    find_by_name, image_generation_client_from_config, openai_compatible_client_from_config,
    DefaultModelImageGenerationClient, GeneratedImage, ImageGenerationClient,
    ImageGenerationRequest, ImageGenerationResult, ImageMimeType, ProviderConfig, ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

struct CapturingImageGenerationClient {
    models: Arc<Mutex<Vec<Option<String>>>>,
}

impl ImageGenerationClient for CapturingImageGenerationClient {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.models
            .lock()
            .map_err(|error| ProviderError::Api {
                status: None,
                message: error.to_string(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            })?
            .push(request.model);
        Ok(ImageGenerationResult {
            provider_id: "test".to_owned(),
            model: "captured".to_owned(),
            images: vec![GeneratedImage {
                index: 0,
                mime_type: ImageMimeType::Png,
                bytes: vec![1],
                byte_len: 1,
                revised_prompt: None,
                provider_item_id: None,
            }],
            remote_images: Vec::new(),
            usage: None,
            request_id: None,
        })
    }
}

#[test]
fn default_model_image_generation_client_injects_missing_model() -> Result<(), Box<dyn Error>> {
    let models = Arc::new(Mutex::new(Vec::new()));
    let client = DefaultModelImageGenerationClient::new(
        "custom-image-model",
        Box::new(CapturingImageGenerationClient {
            models: models.clone(),
        }),
    );

    client.generate_image(ImageGenerationRequest::new("say hi"))?;

    let models = models.lock().map_err(|error| error.to_string())?;
    if models.as_slice() != [Some("custom-image-model".to_owned())] {
        return Err(format!("default model wrapper drifted: {models:?}").into());
    }
    Ok(())
}

#[test]
fn openrouter_chat_registry_remains_openai_compatible() -> Result<(), Box<dyn Error>> {
    let openrouter = find_by_name("openrouter").ok_or("openrouter spec missing")?;
    let client = openai_compatible_client_from_config(ProviderConfig::default(), openrouter)?;
    if openrouter.backend != "openai_compat"
        || !openrouter.supports_image_generation
        || client.transport().base_url() != "https://openrouter.ai/api/v1"
    {
        return Err(format!(
            "OpenRouter registry drifted: backend={} image={} base={}",
            openrouter.backend,
            openrouter.supports_image_generation,
            client.transport().base_url()
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_factory_requires_api_key() -> Result<(), Box<dyn Error>> {
    let previous = std::env::var("OPENAI_API_KEY").ok();
    std::env::remove_var("OPENAI_API_KEY");
    let error = match image_generation_client_from_config("openai", ProviderConfig::default()) {
        Ok(_) => return Err("missing api key unexpectedly created a client".into()),
        Err(error) => error,
    };
    if let Some(previous) = previous {
        std::env::set_var("OPENAI_API_KEY", previous);
    }
    match error {
        ProviderError::AuthRequired { provider_id } if provider_id == "openai" => {}
        other => return Err(format!("unexpected missing api key error: {other:?}").into()),
    }
    Ok(())
}
