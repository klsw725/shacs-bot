use super::{
    GeneratedImage, ImageGenerateTool, ImageGenerateToolConfig, ImageGenerationClient,
    ImageGenerationRequest, ImageGenerationResult, ImageMimeType,
};
use shacs_providers::ProviderError;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(super) struct CapturingClient {
    pub(super) requests: Arc<Mutex<Vec<ImageGenerationRequest>>>,
    pub(super) response: Result<ImageGenerationResult, ProviderError>,
}

impl CapturingClient {
    pub(super) fn success() -> (Self, Arc<Mutex<Vec<ImageGenerationRequest>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        Self {
            requests: requests.clone(),
            response: Ok(ImageGenerationResult {
                provider_id: "openai".to_owned(),
                model: "gpt-image-2".to_owned(),
                images: vec![GeneratedImage {
                    index: 0,
                    mime_type: ImageMimeType::Png,
                    bytes: b"not real png".to_vec(),
                    byte_len: b"not real png".len(),
                    revised_prompt: Some("expanded secret prompt".to_owned()),
                    provider_item_id: Some(shacs_providers::ImageGenerationItemId::from_provider(
                        "item_1",
                    )),
                }],
                remote_images: Vec::new(),
                usage: None,
                request_id: Some(shacs_providers::ImageGenerationRequestId::from_provider(
                    "req_1",
                )),
            }),
        }
        .with_requests(requests)
    }

    fn with_requests(
        self,
        requests: Arc<Mutex<Vec<ImageGenerationRequest>>>,
    ) -> (Self, Arc<Mutex<Vec<ImageGenerationRequest>>>) {
        (self, requests)
    }
}

impl ImageGenerationClient for CapturingClient {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.requests
            .lock()
            .map_err(|_| ProviderError::Api {
                status: None,
                message: "capture lock poisoned".to_owned(),
                retryable: false,
                headers: Default::default(),
                body: None,
            })?
            .push(request);
        self.response.clone()
    }
}

pub(super) fn tool_with_client(client: CapturingClient, media_dir: PathBuf) -> ImageGenerateTool {
    ImageGenerateTool::new(
        Box::new(client),
        media_dir,
        ImageGenerateToolConfig {
            provider_id: "openai".to_owned(),
            model_id: "gpt-image-2".to_owned(),
            default_format: "png".to_owned(),
            max_count: 2,
            max_bytes: 1024,
        },
    )
}
