use serde_json::json;
use shacs_core::generated_media::ProviderRemoteMediaCandidate;
use shacs_providers::{parse_openrouter_image_generation_response, ImageGenerationHttpResponse};
use std::collections::BTreeMap;

pub fn candidate(url: &str) -> ProviderRemoteMediaCandidate {
    let mut result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({
                "choices": [{"message": {"images": [{
                    "mime_type": "image/png",
                    "image_url": {"url": url}
                }]}}]
            }),
        },
        "image-model",
    )
    .expect("provider fixture parses");
    result
        .remote_images
        .pop()
        .expect("remote fixture candidate")
}

pub fn png() -> Vec<u8> {
    b"\x89PNG\r\n\x1a\nfixture".to_vec()
}
