use super::parsing::{
    parse_openai_image_generation_response_with_format, parse_openrouter_image_generation_response,
};
use super::requests::{
    build_openai_image_generation_request, build_openrouter_image_generation_request,
};
use super::{
    non_empty_model, openai_output_format_mime_type, DefaultModelImageGenerationClient,
    ImageGenerationClient, ImageGenerationHttpTransport, ImageGenerationRequest,
    ImageGenerationResult, ImageOperationRequest, ImageOperationResult,
    OpenAiImageGenerationClient, OpenRouterImageGenerationClient,
};
use crate::clients::image_operations::execute_openai_image_operation;
use crate::error::ProviderError;
use crate::{ProviderEvent, ProviderInvocation};

impl<T> OpenAiImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    pub fn new(
        api_key: impl Into<String>,
        api_base: impl Into<String>,
        default_model: impl Into<String>,
        transport: T,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_base: api_base.into(),
            default_model: default_model.into(),
            transport,
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }
}

impl<T> ImageGenerationClient for OpenAiImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        let model = request
            .model
            .as_deref()
            .and_then(non_empty_model)
            .unwrap_or(&self.default_model)
            .to_owned();
        let output_format = request
            .output_format
            .as_deref()
            .and_then(openai_output_format_mime_type);
        let parts = build_openai_image_generation_request(&self.api_key, &request, &model);
        parse_openai_image_generation_response_with_format(
            self.transport.post_json(parts)?,
            &model,
            output_format,
        )
    }

    fn execute_image_operation(
        &self,
        request: ImageOperationRequest,
    ) -> Result<ImageOperationResult, ProviderError> {
        match request {
            ImageOperationRequest::Generate(request) => self
                .generate_image(request)
                .map(ImageOperationResult::Generate),
            other => execute_openai_image_operation(
                &self.transport,
                &self.api_key,
                &self.default_model,
                other,
            ),
        }
    }
}

impl<T> OpenRouterImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    pub fn new(
        api_key: impl Into<String>,
        api_base: impl Into<String>,
        default_model: impl Into<String>,
        transport: T,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_base: api_base.into(),
            default_model: default_model.into(),
            transport,
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }
}

impl<T> ImageGenerationClient for OpenRouterImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        let model = request
            .model
            .as_deref()
            .and_then(non_empty_model)
            .unwrap_or(&self.default_model)
            .to_owned();
        let parts = build_openrouter_image_generation_request(&self.api_key, &request, &model);
        parse_openrouter_image_generation_response(self.transport.post_json(parts)?, &model)
    }

    fn execute_image_operation(
        &self,
        request: ImageOperationRequest,
    ) -> Result<ImageOperationResult, ProviderError> {
        match request {
            ImageOperationRequest::Generate(request) => self
                .generate_image(request)
                .map(ImageOperationResult::Generate),
            other => Err(ProviderError::UnsupportedCapability {
                provider_id: "openrouter".to_owned(),
                capability: other.operation().capability_name().to_owned(),
            }),
        }
    }
}

impl DefaultModelImageGenerationClient {
    pub fn new(default_model: impl Into<String>, inner: Box<dyn ImageGenerationClient>) -> Self {
        Self {
            default_model: default_model.into(),
            inner,
        }
    }

    fn apply_default_model(&self, mut request: ImageGenerationRequest) -> ImageGenerationRequest {
        if request.model.as_deref().and_then(non_empty_model).is_none() {
            request.model = Some(self.default_model.clone());
        }
        request
    }
}

impl ImageGenerationClient for DefaultModelImageGenerationClient {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.inner.generate_image(self.apply_default_model(request))
    }

    fn generate_image_with_observer(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.inner
            .generate_image_with_observer(self.apply_default_model(request), on_event)
    }

    fn generate_image_with_invocation(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: &ProviderInvocation,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.inner.generate_image_with_invocation(
            self.apply_default_model(request),
            on_event,
            invocation,
        )
    }

    fn execute_image_operation(
        &self,
        mut request: ImageOperationRequest,
    ) -> Result<ImageOperationResult, ProviderError> {
        request.apply_default_model(&self.default_model);
        self.inner.execute_image_operation(request)
    }
}
