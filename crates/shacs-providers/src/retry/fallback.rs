use crate::provider::ProviderRequest;
use serde_json::{json, Value};

pub(super) fn request_with_stripped_images(request: &ProviderRequest) -> Option<ProviderRequest> {
    let mut found = false;
    let messages = request
        .messages
        .iter()
        .map(|message| strip_images_from_message(message, &mut found))
        .collect::<Vec<_>>();
    found.then(|| ProviderRequest {
        messages,
        tools: request.tools.clone(),
        model: request.model.clone(),
        settings: request.settings.clone(),
        tool_choice: request.tool_choice.clone(),
    })
}

fn strip_images_from_message(message: &Value, found: &mut bool) -> Value {
    let Some(object) = message.as_object() else {
        return message.clone();
    };
    let Some(content) = object.get("content").and_then(Value::as_array) else {
        return message.clone();
    };
    let mut stripped = object.clone();
    stripped.insert(
        "content".to_owned(),
        Value::Array(
            content
                .iter()
                .map(|block| strip_image_block(block, found))
                .collect(),
        ),
    );
    Value::Object(stripped)
}

fn strip_image_block(block: &Value, found: &mut bool) -> Value {
    let Some(object) = block.as_object() else {
        return block.clone();
    };
    if object.get("type").and_then(Value::as_str) != Some("image_url") {
        return block.clone();
    }
    *found = true;
    json!({
        "type": "text",
        "text": image_placeholder_text(
            object
                .get("_meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("path"))
                .and_then(Value::as_str)
        ),
    })
}

fn image_placeholder_text(path: Option<&str>) -> String {
    path.filter(|path| !path.is_empty())
        .map(|path| format!("[image: {path}]"))
        .unwrap_or_else(|| "[image omitted]".to_owned())
}
