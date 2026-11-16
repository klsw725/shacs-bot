use crate::{
    ApiError, ApiModel, ChatCompletionInvocation, ChatResponseWritten, DiagnosticsKind,
    DiagnosticsRecord, DiagnosticsSeverity, DiagnosticsSnapshot, RememberedPermissionProjection,
    Spec030RuntimeProjection, Spec030UnavailableReason, Spec031ApiProjection,
    Spec035TasksStreamEvent, WebSocketRuntimeTurn, WebSocketRuntimeTurnEnd, WebSocketRuntimeTurns,
};
use serde_json::{json, Value};
use shacs_channels::WebSocketServerEvent;
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec031ConstructionError, Spec035OwnerSurface,
    Spec035RevisedOwnerFacts, Spec035RevisedProjection,
};
use shacs_providers::{LlmResponse, ProviderEvent};
use std::path::PathBuf;
use std::sync::Arc;

pub trait ChatCompletionAdapter {
    fn configured_model(&self) -> &str;

    fn models(&self) -> Vec<ApiModel> {
        vec![ApiModel {
            id: self.configured_model().to_owned(),
            owned_by: "shacs-bot".to_owned(),
        }]
    }

    fn complete_chat(&self, invocation: ChatCompletionInvocation) -> Result<LlmResponse, ApiError>;

    fn stream_chat(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ApiError> {
        let response = self.complete_chat(invocation)?;
        if let Some(content) = response
            .content
            .as_ref()
            .filter(|content| !content.is_empty())
        {
            on_event(ProviderEvent::TextDelta {
                text: content.clone(),
            });
        }
        on_event(ProviderEvent::Finish {
            usage: json!(response.usage),
            reason: response.finish_reason.clone(),
        });
        Ok(response)
    }

    /// Like `complete_chat`, and also returns what to run once the response was written
    /// to the client in full. The server drops it without running it when the response
    /// did not get that far.
    fn complete_chat_reporting(
        &self,
        invocation: ChatCompletionInvocation,
    ) -> Result<(LlmResponse, Option<ChatResponseWritten>), ApiError> {
        self.complete_chat(invocation)
            .map(|response| (response, None))
    }

    /// The streaming counterpart of `complete_chat_reporting`.
    fn stream_chat_reporting(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(LlmResponse, Option<ChatResponseWritten>), ApiError> {
        self.stream_chat(invocation, on_event)
            .map(|response| (response, None))
    }

    fn persist_media_data_urls(&self, data_urls: &[String]) -> Result<Vec<String>, ApiError> {
        Ok(data_urls.to_vec())
    }

    fn persist_media_data_urls_for_session(
        &self,
        _session_key: &str,
        data_urls: &[String],
    ) -> Result<Vec<String>, ApiError> {
        self.persist_media_data_urls(data_urls)
    }

    fn persist_uploaded_file(
        &self,
        _filename: Option<&str>,
        _bytes: &[u8],
    ) -> Result<String, ApiError> {
        Err(ApiError::unsupported_media(
            "multipart file persistence is not configured for this adapter",
        ))
    }

    fn persist_uploaded_file_for_session(
        &self,
        _session_key: &str,
        filename: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        self.persist_uploaded_file(filename, bytes)
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        None
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Guard for 042 web watch reads: public addresses only unless a test build overrides it.
    fn web_watch_network_guard(&self) -> shacs_core::runtime::NetworkGuard {
        shacs_core::runtime::NetworkGuard::default()
    }

    fn rendered_web_reader(&self) -> Option<&dyn shacs_core::runtime::RenderedWebReader> {
        None
    }

    fn web_watch_email_ready(&self) -> bool {
        false
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn stream_spec035_tasks_events(
        &self,
        _session_id: &str,
        _on_event: &mut dyn FnMut(Spec035TasksStreamEvent),
    ) -> Result<(), ApiError> {
        Ok(())
    }

    fn local_improvement(
        &self,
        _action: &str,
        _proposal_id: &str,
        _body: Value,
    ) -> Result<Value, ApiError> {
        Err(ApiError::not_found(
            "local improvement surface is not configured",
        ))
    }

    fn diagnostics_snapshot(&self) -> DiagnosticsSnapshot {
        let mut snapshot = DiagnosticsSnapshot::unavailable(
            "runtime diagnostics are not configured for this adapter",
        );
        snapshot.diagnostics.push(DiagnosticsRecord::new(
            DiagnosticsSeverity::Info,
            DiagnosticsKind::Api,
            "diagnostics request was read-only",
        ));
        snapshot
    }

    fn diagnostics_projection(&self) -> Value {
        self.diagnostics_snapshot().redacted_value()
    }

    fn readiness_projection(&self) -> Option<Value> {
        None
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing)
    }

    fn spec035_revised_projection(
        &self,
    ) -> Result<Spec035RevisedProjection, Spec031ConstructionError> {
        let trusted_runtime = self.trusted_runtime_projection();
        project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
            Spec035OwnerSurface::Api,
            &trusted_runtime,
        ))
    }

    fn spec031_projection(
        &self,
        _projection: Spec031ApiProjection,
    ) -> Result<Option<shacs_projection::Spec031Envelope>, ApiError> {
        Ok(None)
    }

    fn workflow_recipes_projection(&self) -> Option<Value> {
        None
    }

    fn remembered_permissions_projection(&self) -> Option<RememberedPermissionProjection> {
        None
    }

    fn process_websocket_frame(
        &self,
        _frame: Value,
        _client_id: &str,
        _default_chat_id: &str,
    ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
        Err(ApiError::not_implemented(
            "websocket frame handling is not configured for this adapter",
        ))
    }

    /// Handles one frame and hands each event it makes to `on_event`, which returns
    /// whether the event was written to the connection. A queued event is not a written one.
    fn process_websocket_frame_streaming(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
        on_event: &mut dyn FnMut(WebSocketServerEvent) -> bool,
    ) -> Result<(), ApiError> {
        for event in self.process_websocket_frame(frame, client_id, default_chat_id)? {
            on_event(event);
        }
        Ok(())
    }

    /// Where the adapter offers the turns it starts by itself on a WebSocket
    /// connection. `None` when it never starts one.
    fn websocket_runtime_turns(&self) -> Option<Arc<WebSocketRuntimeTurns>> {
        None
    }

    /// Runs a turn the adapter offered, on the connection it was handed to. Each event
    /// goes to `on_event`, which returns whether it was written to the connection.
    fn process_websocket_runtime_turn_streaming(
        &self,
        _turn: &WebSocketRuntimeTurn,
        _on_event: &mut dyn FnMut(WebSocketServerEvent) -> bool,
    ) -> Result<WebSocketRuntimeTurnEnd, ApiError> {
        Ok(WebSocketRuntimeTurnEnd::Done)
    }
}
