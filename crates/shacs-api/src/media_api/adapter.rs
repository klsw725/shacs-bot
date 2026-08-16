use crate::{ApiError, ApiModel, ChatCompletionInvocation, Spec031ApiProjection};
use crate::{
    DiagnosticsKind, DiagnosticsRecord, DiagnosticsSeverity, DiagnosticsSnapshot,
    RememberedPermissionProjection, Spec030RuntimeProjection, Spec030UnavailableReason,
};
use serde_json::{json, Value};
use shacs_channels::WebSocketServerEvent;
use shacs_projection::Spec035MediaProjection;
use shacs_providers::{LlmResponse, ProviderEvent};
use std::path::PathBuf;

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

    fn media_projection(&self) -> Option<Spec035MediaProjection> {
        let data_dir = self.runtime_data_dir()?;
        shacs_core::runtime::Spec035MediaProjectionStore::new(data_dir)
            .read()
            .ok()
            .flatten()
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing)
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

    fn process_websocket_frame_streaming(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
        on_event: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        for event in self.process_websocket_frame(frame, client_id, default_chat_id)? {
            on_event(event);
        }
        Ok(())
    }
}
