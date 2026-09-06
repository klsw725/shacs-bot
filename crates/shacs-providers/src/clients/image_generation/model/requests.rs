use super::{
    non_empty_option, ImageGenerationRequest, ImageGenerationRequestParts, IMAGE_GENERATION_PATH,
    OPENROUTER_IMAGE_GENERATION_PATH,
};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;

pub fn build_openai_image_generation_request(
    api_key: &str,
    request: &ImageGenerationRequest,
    model: &str,
) -> ImageGenerationRequestParts {
    let mut body = request.provider_options.clone();
    body.insert("model".to_owned(), Value::String(model.to_owned()));
    body.insert("prompt".to_owned(), Value::String(request.prompt.clone()));
    if let Some(size) = non_empty_option(request.size.as_deref()) {
        body.insert("size".to_owned(), Value::String(size.to_owned()));
    }
    if let Some(quality) = non_empty_option(request.quality.as_deref()) {
        body.insert("quality".to_owned(), Value::String(quality.to_owned()));
    }
    if let Some(output_format) = non_empty_option(request.output_format.as_deref()) {
        body.insert(
            "output_format".to_owned(),
            Value::String(output_format.to_owned()),
        );
    }
    if let Some(background) = non_empty_option(request.background.as_deref()) {
        body.insert(
            "background".to_owned(),
            Value::String(background.to_owned()),
        );
    }
    if let Some(count) = request.count {
        body.insert("n".to_owned(), Value::Number(Number::from(count)));
    }
    ImageGenerationRequestParts {
        path: IMAGE_GENERATION_PATH.to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {api_key}"))]),
        body: Value::Object(body),
    }
}

pub fn build_openrouter_image_generation_request(
    api_key: &str,
    request: &ImageGenerationRequest,
    model: &str,
) -> ImageGenerationRequestParts {
    let mut body = Map::new();
    body.insert("model".to_owned(), Value::String(model.to_owned()));
    body.insert(
        "messages".to_owned(),
        Value::Array(vec![Value::Object(Map::from_iter([
            ("role".to_owned(), Value::String("user".to_owned())),
            ("content".to_owned(), Value::String(request.prompt.clone())),
        ]))]),
    );
    body.insert(
        "modalities".to_owned(),
        Value::Array(vec![
            Value::String("image".to_owned()),
            Value::String("text".to_owned()),
        ]),
    );
    body.insert("stream".to_owned(), Value::Bool(false));
    let mut image_config = request.provider_options.clone();
    for (key, value) in [
        ("size", non_empty_option(request.size.as_deref())),
        ("quality", non_empty_option(request.quality.as_deref())),
        (
            "output_format",
            non_empty_option(request.output_format.as_deref()),
        ),
        (
            "background",
            non_empty_option(request.background.as_deref()),
        ),
    ] {
        if let Some(value) = value {
            image_config.insert(key.to_owned(), Value::String(value.to_owned()));
        }
    }
    if let Some(count) = request.count {
        image_config.insert("n".to_owned(), Value::Number(Number::from(count)));
    }
    if !image_config.is_empty() {
        body.insert("image_config".to_owned(), Value::Object(image_config));
    }
    ImageGenerationRequestParts {
        path: OPENROUTER_IMAGE_GENERATION_PATH.to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {api_key}"))]),
        body: Value::Object(body),
    }
}
