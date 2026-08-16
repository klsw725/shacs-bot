use super::{candidate_id, media_error, sequence, stale, CodexMediaStreamState};
use crate::error::ProviderError;
use crate::media::{
    ProviderMediaBytes, ProviderMediaCandidate, ProviderMediaLifecycleObservation,
    ProviderMediaOrigin,
};
use crate::provider::ProviderEvent;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{Map, Value};

impl CodexMediaStreamState {
    pub(super) fn start_item(
        &mut self,
        id: &str,
        event_sequence: Option<u32>,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        let candidate_id = candidate_id(id)?;
        let item = self.items.entry(id.to_owned()).or_default();
        if stale(item, event_sequence) {
            return Ok(());
        }
        item.last_sequence = event_sequence.or(item.last_sequence);
        if !item.started {
            item.started = true;
            on_event(ProviderEvent::MediaLifecycle(
                ProviderMediaLifecycleObservation::started(candidate_id),
            ));
        }
        Ok(())
    }

    pub(super) fn partial(
        &mut self,
        value: &Value,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        let id = super::event_item_id(value)?;
        let event_sequence =
            sequence(value).ok_or_else(|| media_error("missing image sequence"))?;
        if self
            .items
            .get(id)
            .is_some_and(|item| stale(item, Some(event_sequence)))
        {
            return Ok(());
        }
        self.start_item(id, None, on_event)?;
        let item = self
            .items
            .get_mut(id)
            .ok_or_else(|| media_error("missing image item"))?;
        if item.last_sequence.is_some_and(|last| event_sequence < last) {
            return Ok(());
        }
        self.partial_count = self.partial_count.saturating_add(1);
        if self.partial_count > super::CODEX_SSE_MAX_PARTIAL_IMAGES {
            return Err(media_error("Codex partial image limit exceeded"));
        }
        let encoded = value
            .get("partial_image_b64")
            .and_then(Value::as_str)
            .ok_or_else(|| media_error("missing partial image payload"))?;
        STANDARD
            .decode(encoded)
            .map_err(|_| media_error("malformed partial image payload"))?;
        item.last_sequence = Some(event_sequence);
        on_event(ProviderEvent::MediaLifecycle(
            ProviderMediaLifecycleObservation::partial(candidate_id(id)?, event_sequence),
        ));
        Ok(())
    }

    pub(super) fn final_item(
        &mut self,
        item: &Map<String, Value>,
        event_sequence: Option<u32>,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        let id = super::item_id(item)?;
        if self
            .items
            .get(id)
            .is_some_and(|state| state.final_seen || stale(state, event_sequence))
        {
            return Ok(());
        }
        self.start_item(id, None, on_event)?;
        let state = self
            .items
            .get_mut(id)
            .ok_or_else(|| media_error("missing image item"))?;
        let encoded = item
            .get("result")
            .and_then(Value::as_str)
            .ok_or_else(|| media_error("missing final image payload"))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| media_error("malformed final image payload"))?;
        let sequence = event_sequence.or(state.last_sequence).unwrap_or_default();
        state.last_sequence = Some(sequence);
        state.completed = true;
        state.final_seen = true;
        let candidate_id = candidate_id(id)?;
        self.candidates.push(ProviderMediaCandidate::bytes(
            candidate_id.clone(),
            ProviderMediaOrigin::new("openai_codex", self.model.clone()),
            ProviderMediaBytes::new(self.mime_type.clone(), bytes),
        ));
        on_event(ProviderEvent::MediaLifecycle(
            ProviderMediaLifecycleObservation::final_candidate(candidate_id, sequence),
        ));
        Ok(())
    }

    pub(super) fn final_response_items(
        &mut self,
        response: &Map<String, Value>,
        event_sequence: Option<u32>,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        for item in response
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter(|item| {
                item.get("type").and_then(Value::as_str) == Some("image_generation_call")
            })
        {
            self.final_item(item, event_sequence, on_event)?;
        }
        Ok(())
    }

    pub(super) fn fail(&mut self, on_event: &mut dyn FnMut(ProviderEvent)) {
        for (id, item) in &self.items {
            if item.final_seen {
                continue;
            }
            if let Ok(id) = candidate_id(id) {
                on_event(ProviderEvent::MediaLifecycle(
                    ProviderMediaLifecycleObservation::failed(id, item.last_sequence),
                ));
            }
        }
    }
}
