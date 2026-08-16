use serde_json::{json, Value};
use shacs_providers::ProviderEvent;

pub fn stream_event_frame(
    event: &ProviderEvent,
    model: &str,
    request_id: &str,
    created: u64,
) -> String {
    let chunk = match event {
        ProviderEvent::TextDelta { text } => json!({
            "id": request_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": text},
                "finish_reason": Value::Null,
            }],
        }),
        ProviderEvent::ReasoningDelta { text } => json!({
            "id": request_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": text},
                "finish_reason": Value::Null,
            }],
        }),
        ProviderEvent::Finish { reason, .. } => {
            crate::finish_stream_chunk(model, request_id, created, reason)
        }
        ProviderEvent::ToolCallStart { .. }
        | ProviderEvent::ToolCallDelta { .. }
        | ProviderEvent::ToolCallReady { .. }
        | ProviderEvent::MediaLifecycle(_) => json!({
            "id": request_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": Value::Null,
            }],
        }),
    };
    crate::sse_data_frame(&chunk)
}
