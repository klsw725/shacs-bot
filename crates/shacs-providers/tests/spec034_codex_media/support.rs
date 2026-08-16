use serde_json::json;
use shacs_providers::{
    CodexClient, CodexHttpStreamResponse, CodexRequestParts, ImageGenerationRequest, ProviderConfig,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub type CapturedRequests = Arc<Mutex<Vec<CodexRequestParts>>>;

pub fn recorded_fixture() -> &'static str {
    include_str!("../fixtures/spec034_codex_media.sse")
}

pub fn image_request() -> ImageGenerationRequest {
    let mut request = ImageGenerationRequest::new("draw a safe fixture");
    request.model = Some("gpt-5.6".to_owned());
    request.size = Some("1024x1024".to_owned());
    request.quality = Some("high".to_owned());
    request.output_format = Some("png".to_owned());
    request.background = Some("opaque".to_owned());
    request.count = Some(1);
    request
}

pub fn capturing_client(
    body: &'static str,
) -> (
    CodexClient<impl shacs_providers::CodexHttpTransport>,
    CapturedRequests,
) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let transport = move |request: CodexRequestParts| {
        sink.lock()
            .map_err(|_| shacs_providers::ProviderError::Api {
                status: None,
                message: "request capture lock poisoned".to_owned(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            })?
            .push(request);
        Ok(CodexHttpStreamResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: body.to_owned(),
        })
    };
    (
        CodexClient::new(
            ProviderConfig {
                api_key: Some("fixture-token".to_owned()),
                ..ProviderConfig::default()
            },
            transport,
        ),
        captured,
    )
}

pub fn partial_frame(item_id: &str, sequence: u32, partial_index: u32) -> String {
    format!(
        "event: response.image_generation_call.partial_image\ndata: {}\n\n",
        json!({
            "type": "response.image_generation_call.partial_image",
            "item_id": item_id,
            "sequence_number": sequence,
            "partial_image_index": partial_index,
            "partial_image_b64": "cGFydGlhbA==",
        })
    )
}
