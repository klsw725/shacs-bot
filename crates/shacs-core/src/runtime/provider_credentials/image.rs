use super::{
    ProviderConfigResolutionRequest, ProviderCredentialClientConfig, ProviderCredentialRuntime,
};
use shacs_providers::{
    image_generation_client_from_config, resolve_image_generation_provider, ImageGenerationClient,
    ImageGenerationRequest, ImageGenerationResolutionRequest, ImageGenerationResult, ProviderError,
    ProviderRegistry,
};
use std::sync::Arc;

pub struct CredentialResolvingImageGenerationClient {
    config: ProviderCredentialClientConfig,
    runtime: Arc<ProviderCredentialRuntime>,
}

impl CredentialResolvingImageGenerationClient {
    pub fn new(
        config: ProviderCredentialClientConfig,
        runtime: Arc<ProviderCredentialRuntime>,
    ) -> Self {
        Self { config, runtime }
    }
}

impl ImageGenerationClient for CredentialResolvingImageGenerationClient {
    fn generate_image(
        &self,
        mut request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        let registry = ProviderRegistry::new();
        let selection = self.runtime.providers_for_selection(&self.config.providers);
        let provider_match =
            resolve_image_generation_provider(&ImageGenerationResolutionRequest {
                registry: &registry,
                requested_provider: &self.config.requested_provider,
                model: &self.config.model,
                providers: &selection,
            })?;
        let resolved = self
            .runtime
            .resolve_provider_config(ProviderConfigResolutionRequest {
                registry: &registry,
                provider_match,
                providers: &self.config.providers,
            })?;
        request.model = Some(resolved.model);
        image_generation_client_from_config(&resolved.provider_id, resolved.config)?
            .generate_image(request)
    }
}
