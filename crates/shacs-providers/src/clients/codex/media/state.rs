use super::{
    CODEX_SSE_MAX_AGGREGATE_BYTES, CODEX_SSE_MAX_FRAME_BYTES, CODEX_SSE_MAX_LINE_BYTES,
    CODEX_SSE_MAX_PARTIAL_IMAGES,
};
use crate::clients::openai_compatible::OpenAiResponsesStreamState;
use crate::error::ProviderError;
use crate::media::{
    ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaLifecycleObservation,
};
use crate::provider::ProviderEvent;
use crate::types::LlmResponse;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

mod items;

#[derive(Debug, Default)]
struct ImageItemState {
    last_sequence: Option<u32>,
    started: bool,
    completed: bool,
    final_seen: bool,
}

pub(super) struct CodexMediaStreamState {
    text: OpenAiResponsesStreamState,
    model: String,
    mime_type: String,
    items: BTreeMap<String, ImageItemState>,
    candidates: Vec<ProviderMediaCandidate>,
    aggregate_bytes: usize,
    partial_count: u32,
    cancelled: bool,
}

impl CodexMediaStreamState {
    pub(super) fn new(model: String, mime_type: String) -> Self {
        Self {
            text: OpenAiResponsesStreamState::default(),
            model,
            mime_type,
            items: BTreeMap::new(),
            candidates: Vec::new(),
            aggregate_bytes: 0,
            partial_count: 0,
            cancelled: false,
        }
    }

    pub(super) fn process_frame_text(
        &mut self,
        frame_text: &str,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<bool, ProviderError> {
        self.validate_bounds(frame_text)?;
        let Some(value) = frame_value(frame_text)? else {
            return self.text.process_frame_text(frame_text, on_event);
        };
        let event_type = value.get("type").and_then(Value::as_str);
        match event_type {
            Some("response.output_item.added") => {
                if let Some(item) = image_item(&value) {
                    self.start_item(item_id(item)?, sequence(&value), on_event)?;
                }
            }
            Some(
                "response.image_generation_call.in_progress"
                | "response.image_generation_call.generating",
            ) => {
                self.start_item(event_item_id(&value)?, sequence(&value), on_event)?;
            }
            Some("response.image_generation_call.partial_image") => {
                self.partial(&value, on_event)?;
            }
            Some("response.image_generation_call.completed") => {
                let id = event_item_id(&value)?;
                self.start_item(id, sequence(&value), on_event)?;
                if let Some(item) = self.items.get_mut(id) {
                    item.completed = true;
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = image_item(&value) {
                    self.final_item(item, sequence(&value), on_event)?;
                }
            }
            Some("response.completed") => {
                if let Some(response) = value.get("response").and_then(Value::as_object) {
                    match response.get("status").and_then(Value::as_str) {
                        Some("cancelled") => {
                            self.cancel(on_event);
                            return Ok(true);
                        }
                        Some("failed") => {
                            self.fail(on_event);
                            return Err(media_error("Codex native image generation failed"));
                        }
                        _ => self.final_response_items(response, sequence(&value), on_event)?,
                    }
                }
            }
            Some("response.cancelled") => {
                self.cancel(on_event);
                return Ok(true);
            }
            Some("error" | "response.failed") => {
                self.fail(on_event);
                return Err(media_error("Codex native image generation failed"));
            }
            Some("response.incomplete")
                if value
                    .get("response")
                    .and_then(|response| response.get("status"))
                    .and_then(Value::as_str)
                    == Some("cancelled") =>
            {
                self.cancel(on_event);
                return Ok(true);
            }
            Some("response.incomplete") => {}
            _ => {}
        }
        self.text.process_frame_text(frame_text, on_event)
    }

    pub(super) fn cancel(&mut self, on_event: &mut dyn FnMut(ProviderEvent)) {
        if self.cancelled {
            return;
        }
        self.cancelled = true;
        if self.items.is_empty() {
            if let Ok(id) = ProviderMediaCandidateId::new("codex_image") {
                on_event(ProviderEvent::MediaLifecycle(
                    ProviderMediaLifecycleObservation::cancelled(id, None),
                ));
            }
            return;
        }
        for (id, item) in &self.items {
            if item.final_seen {
                continue;
            }
            if let Ok(id) = candidate_id(id) {
                on_event(ProviderEvent::MediaLifecycle(
                    ProviderMediaLifecycleObservation::cancelled(id, item.last_sequence),
                ));
            }
        }
    }

    pub(super) fn finish(
        self,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        if self.cancelled {
            return Err(media_error("Codex native image generation cancelled"));
        }
        if self.candidates.is_empty()
            && self
                .items
                .values()
                .any(|item| item.started || item.completed)
        {
            return Err(media_error(
                "Codex native image generation completed without a final image",
            ));
        }
        let mut response = self.text.finish(on_event)?;
        response.media_candidates = self.candidates;
        Ok(response)
    }

    fn validate_bounds(&mut self, frame_text: &str) -> Result<(), ProviderError> {
        self.aggregate_bytes = self.aggregate_bytes.saturating_add(frame_text.len());
        if self.aggregate_bytes > CODEX_SSE_MAX_AGGREGATE_BYTES {
            return Err(media_error("Codex SSE aggregate limit exceeded"));
        }
        crate::clients::sse::split_sse_frame_texts_bounded(
            frame_text,
            CODEX_SSE_MAX_LINE_BYTES,
            CODEX_SSE_MAX_FRAME_BYTES,
            CODEX_SSE_MAX_FRAME_BYTES,
        )
        .map(|_| ())
        .map_err(|error| media_error(error.to_string()))
    }
}

fn frame_value(frame_text: &str) -> Result<Option<Value>, ProviderError> {
    let data = frame_text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str(&data)
        .map(Some)
        .map_err(|_| media_error("invalid Codex SSE JSON"))
}

fn image_item(value: &Value) -> Option<&Map<String, Value>> {
    value
        .get("item")
        .and_then(Value::as_object)
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("image_generation_call"))
}

fn item_id(item: &Map<String, Value>) -> Result<&str, ProviderError> {
    item.get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| media_error("missing image item id"))
}

fn event_item_id(value: &Value) -> Result<&str, ProviderError> {
    value
        .get("item_id")
        .and_then(Value::as_str)
        .ok_or_else(|| media_error("missing image item id"))
}

fn sequence(value: &Value) -> Option<u32> {
    value
        .get("sequence_number")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn candidate_id(id: &str) -> Result<ProviderMediaCandidateId, ProviderError> {
    let projected = crate::ImageGenerationItemId::from_provider(id).into_string();
    ProviderMediaCandidateId::new(projected).map_err(|_| media_error("invalid Codex image item id"))
}

fn stale(item: &ImageItemState, event_sequence: Option<u32>) -> bool {
    event_sequence
        .zip(item.last_sequence)
        .is_some_and(|(current, previous)| current <= previous)
}

fn media_error(message: impl ToString) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.to_string(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
