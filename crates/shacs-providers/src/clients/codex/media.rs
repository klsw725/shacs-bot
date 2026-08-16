mod state;

use super::{api_error, build_codex_responses_request, CodexClient, CodexHttpTransport};
use crate::clients::image_generation::{
    GeneratedImage, ImageGenerationClient, ImageGenerationItemId, ImageGenerationRequest,
    ImageGenerationResult, ImageMimeType,
};
use crate::clients::image_operations::{ImageOperationRequest, ImageOperationResult};
use crate::error::ProviderError;
use crate::provider::{ProviderEvent, ProviderInvocation, ProviderRequest};
use crate::types::{GenerationSettings, LlmResponse};
use serde_json::{json, Map, Value};

use state::CodexMediaStreamState;

pub const CODEX_SSE_MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const CODEX_SSE_MAX_FRAME_BYTES: usize = 20 * 1024 * 1024;
pub const CODEX_SSE_MAX_AGGREGATE_BYTES: usize = 32 * 1024 * 1024;
pub const CODEX_SSE_MAX_PARTIAL_IMAGES: u32 = 3;

struct NativeImageAdmission;

impl<T> CodexClient<T>
where
    T: CodexHttpTransport,
{
    pub fn generate_image_with_observer(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.generate_image_response(request, on_event, None)
    }

    pub fn generate_image_with_invocation(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: &ProviderInvocation,
    ) -> Result<ImageGenerationResult, ProviderError> {
        if invocation.is_cancelled() {
            return Err(api_error(None, "Codex native image generation cancelled"));
        }
        self.generate_image_response(request, on_event, Some(invocation))
    }

    fn generate_image_response(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: Option<&ProviderInvocation>,
    ) -> Result<ImageGenerationResult, ProviderError> {
        if request.count.unwrap_or(1) != 1 {
            return Err(api_error(
                None,
                "Codex native image generation supports exactly one image per tool call",
            ));
        }
        let model = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .unwrap_or("gpt-5.6")
            .to_owned();
        let mime_type = output_mime_type(request.output_format.as_deref());
        let parts =
            build_native_image_request(&request, &self.config, &model, NativeImageAdmission);
        let mut stream = CodexMediaStreamState::new(model.clone(), mime_type.as_str().to_owned());
        let response = self.transport.post_json_stream_frames_bounded(
            parts,
            &mut |frame| {
                if invocation.is_some_and(ProviderInvocation::is_cancelled) {
                    stream.cancel(on_event);
                    return Ok(true);
                }
                let done = stream.process_frame_text(frame, on_event)?;
                if invocation.is_some_and(ProviderInvocation::is_cancelled) {
                    stream.cancel(on_event);
                    return Ok(true);
                }
                Ok(done)
            },
            invocation.and_then(ProviderInvocation::remaining),
        )?;
        if !(200..300).contains(&response.status) {
            return Err(api_error(
                Some(response.status),
                "Codex native image generation request failed",
            ));
        }
        let llm_response = stream.finish(on_event)?;
        image_result_from_response(llm_response, model)
    }
}

impl<T> ImageGenerationClient for CodexClient<T>
where
    T: CodexHttpTransport,
{
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.generate_image_with_observer(request, &mut |_| {})
    }

    fn generate_image_with_observer(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<ImageGenerationResult, ProviderError> {
        CodexClient::generate_image_with_observer(self, request, on_event)
    }

    fn generate_image_with_invocation(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: &ProviderInvocation,
    ) -> Result<ImageGenerationResult, ProviderError> {
        CodexClient::generate_image_with_invocation(self, request, on_event, invocation)
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
                provider_id: "openai_codex".to_owned(),
                capability: other.operation().capability_name().to_owned(),
            }),
        }
    }
}

pub fn parse_codex_media_stream(
    body: &str,
    model: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    let frames = crate::clients::sse::split_sse_frame_texts_bounded(
        body,
        CODEX_SSE_MAX_LINE_BYTES,
        CODEX_SSE_MAX_FRAME_BYTES,
        CODEX_SSE_MAX_AGGREGATE_BYTES,
    )
    .map_err(|error| api_error(None, error))?;
    let mut stream = CodexMediaStreamState::new(model.to_owned(), "image/png".to_owned());
    for frame in frames {
        if stream.process_frame_text(&frame, on_event)? {
            break;
        }
    }
    stream.finish(on_event)
}

fn build_native_image_request(
    request: &ImageGenerationRequest,
    config: &crate::config::ProviderConfig,
    model: &str,
    _admission: NativeImageAdmission,
) -> super::CodexRequestParts {
    let provider_request = ProviderRequest {
        messages: vec![json!({"role": "user", "content": request.prompt})],
        tools: Vec::new(),
        model: model.to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    };
    let mut parts = build_codex_responses_request(&provider_request, config);
    let mut tool = Map::from_iter([(
        "type".to_owned(),
        Value::String("image_generation".to_owned()),
    )]);
    insert_option(&mut tool, "size", request.size.as_deref());
    insert_option(&mut tool, "quality", request.quality.as_deref());
    insert_option(&mut tool, "output_format", request.output_format.as_deref());
    insert_option(&mut tool, "background", request.background.as_deref());
    tool.insert(
        "partial_images".to_owned(),
        Value::Number(CODEX_SSE_MAX_PARTIAL_IMAGES.into()),
    );
    if let Some(body) = parts.body.as_object_mut() {
        body.insert("tools".to_owned(), Value::Array(vec![Value::Object(tool)]));
        body.insert(
            "tool_choice".to_owned(),
            json!({"type": "image_generation"}),
        );
        body.insert("parallel_tool_calls".to_owned(), Value::Bool(false));
    }
    parts
}

fn insert_option(target: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        target.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn image_result_from_response(
    response: LlmResponse,
    model: String,
) -> Result<ImageGenerationResult, ProviderError> {
    let mut images = Vec::with_capacity(response.media_candidates.len());
    for candidate in response.media_candidates {
        let local = candidate.into_local_bytes().map_err(|_| {
            api_error(
                None,
                "Codex native image generation returned unsupported remote media",
            )
        })?;
        let (candidate_id, _, media) = local.into_parts();
        let (mime_type, bytes) = media.into_parts();
        let mime_type = ImageMimeType::parse_provider(&mime_type)
            .ok_or_else(|| api_error(None, "unsupported Codex image MIME type"))?;
        let provider_item_id = ImageGenerationItemId::from_projected(candidate_id.as_str())
            .ok_or_else(|| api_error(None, "invalid projected Codex image item id"))?;
        let byte_len = bytes.len();
        images.push(GeneratedImage {
            index: images.len(),
            mime_type,
            bytes,
            byte_len,
            revised_prompt: None,
            provider_item_id: Some(provider_item_id),
        });
    }
    if images.is_empty() {
        return Err(api_error(
            None,
            "Codex native image generation completed without a final image",
        ));
    }
    Ok(ImageGenerationResult {
        provider_id: "openai_codex".to_owned(),
        model,
        images,
        remote_images: Vec::new(),
        usage: crate::ImageGenerationUsage::from_token_counts(&response.usage),
        request_id: None,
    })
}

fn output_mime_type(format: Option<&str>) -> ImageMimeType {
    match format.unwrap_or("png").trim().to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => ImageMimeType::Jpeg,
        "webp" => ImageMimeType::Webp,
        _ => ImageMimeType::Png,
    }
}
