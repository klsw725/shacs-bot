use crate::{ApiError, ChatCompletionAdapter};
use serde_json::{json, Value};
use shacs_core::runtime::{build_spec035_tasks_projection, serialize_spec035_tasks_projection};
use shacs_projection::{
    Spec035TasksProjection, Spec035TransportClientHello, Spec035TransportClientHelloInput,
    Spec035TransportClientId, Spec035TransportDecision, Spec035TransportEventKind,
    Spec035TransportEventMetadata, Spec035TransportGeneration, Spec035TransportOffer,
    Spec035TransportOfferInput, Spec035TransportOrdering, Spec035TransportSchemaVersion,
    Spec035TransportSequence,
};

pub const SPEC035_CLIENT_ID_HEADER: &str = "x-shacs-spec035-client-id";

#[derive(Debug, Clone, PartialEq)]
pub struct Spec035TasksStreamEvent {
    metadata: Value,
    payload: Value,
}

impl Spec035TasksStreamEvent {
    pub const fn new(metadata: Value, payload: Value) -> Self {
        Self { metadata, payload }
    }

    pub const fn owner_projection(payload: Value) -> Self {
        Self {
            metadata: Value::Null,
            payload,
        }
    }
}

pub(crate) struct Spec035ReconnectConnection {
    ordering: Spec035TransportOrdering,
    generation: Spec035TransportGeneration,
    next_sequence: u64,
    reconnect_gap: bool,
}

impl Spec035ReconnectConnection {
    pub(crate) fn bootstrap(
        adapter: &(impl ChatCompletionAdapter + ?Sized),
        session_id: &str,
        generation: u64,
        reconnect_gap: bool,
    ) -> Result<(Self, Value), ApiError> {
        let generation = transport_generation(generation)?;
        let server = server_hello(&generation)?;
        let mut connection = Self {
            ordering: Spec035TransportOrdering::new(),
            generation,
            next_sequence: 2,
            reconnect_gap,
        };
        if connection.ordering.accept_hello(&server) != Spec035TransportDecision::AcceptHello {
            return Err(ApiError::internal("Spec035 reconnect hello was rejected"));
        }
        let projection = tasks_projection(adapter, session_id)?;
        let metadata = Spec035TransportEventMetadata::try_new(
            Spec035TransportEventKind::Snapshot,
            connection.generation.clone(),
            Spec035TransportSequence::new(1),
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
        if connection.ordering.observe(&metadata) != Spec035TransportDecision::ApplySnapshot {
            return Err(ApiError::internal(
                "Spec035 reconnect snapshot was rejected",
            ));
        }
        let frame = json!({
            "type": "tasks_snapshot",
            "metadata": metadata,
            "payload": projection,
            "reconnect_gap": reconnect_gap,
            "counters": connection.ordering.counters(),
        });
        Ok((connection, frame))
    }

    pub(crate) fn observe(&mut self, event: Spec035TasksStreamEvent) -> Value {
        let metadata = match Spec035TransportEventMetadata::parse_json(&event.metadata.to_string())
        {
            Ok(metadata) => metadata,
            Err(_) => return self.accounting("reject_invalid_metadata"),
        };
        let payload = match Spec035TasksProjection::parse_json(&event.payload.to_string()) {
            Ok(payload) => payload,
            Err(_) => return self.accounting("reject_invalid_payload"),
        };
        let decision = self.ordering.observe(&metadata);
        match decision {
            Spec035TransportDecision::ApplyDelta => json!({
                "type": "tasks_delta",
                "metadata": metadata,
                "payload": payload,
                "reconnect_gap": self.reconnect_gap,
                "counters": self.ordering.counters(),
            }),
            Spec035TransportDecision::RejectPreSnapshot => self.accounting("reject_pre_snapshot"),
            Spec035TransportDecision::RejectStaleGeneration => {
                self.accounting("reject_stale_generation")
            }
            Spec035TransportDecision::RejectDuplicate => self.accounting("reject_duplicate"),
            Spec035TransportDecision::RejectGap => self.accounting("reject_gap"),
            Spec035TransportDecision::AcceptHello
            | Spec035TransportDecision::ApplySnapshot
            | Spec035TransportDecision::RejectUnexpectedHello => {
                self.accounting("reject_unexpected_event")
            }
        }
    }

    pub(crate) fn observe_owner_projection(&mut self, event: Spec035TasksStreamEvent) -> Value {
        let metadata = match Spec035TransportEventMetadata::try_new(
            Spec035TransportEventKind::Delta,
            self.generation.clone(),
            Spec035TransportSequence::new(self.next_sequence),
        ) {
            Ok(metadata) => metadata,
            Err(_) => return self.accounting("reject_invalid_metadata"),
        };
        let frame = self.observe(Spec035TasksStreamEvent::new(
            match serde_json::to_value(metadata) {
                Ok(metadata) => metadata,
                Err(_) => return self.accounting("reject_invalid_metadata"),
            },
            event.payload,
        ));
        if frame["type"] == "tasks_delta" {
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        frame
    }

    fn accounting(&self, reason: &str) -> Value {
        json!({
            "type": "reconnect_accounting",
            "reason": reason,
            "generation": self.generation,
            "reconnect_gap": self.reconnect_gap,
            "counters": self.ordering.counters(),
        })
    }
}

pub(crate) fn parse_client_id(value: &str) -> Result<Spec035TransportClientId, ApiError> {
    Spec035TransportClientId::try_new(value)
        .map_err(|_| ApiError::invalid_request("invalid Spec035 client id"))
}

pub(crate) fn stable_reconnect_key(client_id: &str, session_id: &str) -> String {
    format!(
        "spec035:{}",
        crate::chat_completion_id(&format!("{}:{client_id}{session_id}", client_id.len()))
    )
}

fn tasks_projection(
    adapter: &(impl ChatCompletionAdapter + ?Sized),
    session_id: &str,
) -> Result<Spec035TasksProjection, ApiError> {
    let workspace = adapter
        .session_workspace()
        .ok_or_else(|| ApiError::not_found("tasks owner sources are not configured"))?;
    let data_dir = adapter
        .runtime_data_dir()
        .unwrap_or_else(|| workspace.clone());
    let projection = build_spec035_tasks_projection(&workspace, &data_dir, session_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let encoded = serialize_spec035_tasks_projection(&projection)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Spec035TasksProjection::parse_json(&encoded)
        .map_err(|error| ApiError::internal(error.to_string()))
}

fn transport_generation(value: u64) -> Result<Spec035TransportGeneration, ApiError> {
    Spec035TransportGeneration::try_new(&format!("generation:{value}"))
        .map_err(|error| ApiError::internal(error.to_string()))
}

fn server_hello(
    generation: &Spec035TransportGeneration,
) -> Result<shacs_projection::Spec035TransportServerHello, ApiError> {
    let schema = Spec035TransportSchemaVersion::try_new(1)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let client = Spec035TransportClientHello::try_new(Spec035TransportClientHelloInput {
        client_id: Spec035TransportClientId::try_new("client:api")
            .map_err(|error| ApiError::internal(error.to_string()))?,
        schema_versions: vec![schema],
        mutation_capabilities: Vec::new(),
        resume: None,
    })
    .map_err(|error| ApiError::internal(error.to_string()))?;
    Spec035TransportOffer::try_new(Spec035TransportOfferInput {
        schema_versions: vec![schema],
        mutation_capabilities: Vec::new(),
        generation: generation.clone(),
    })
    .and_then(|offer| offer.select(&client))
    .map_err(|error| ApiError::internal(error.to_string()))
}

#[cfg(test)]
mod tests;
