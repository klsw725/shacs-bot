use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::sync::Mutex;

use serde_json::{json, Map};
use shacs_core::runtime::{
    AgentLoop, AgentLoopConfig, ContextBuilder, InboundMessage, MessageBus, SessionManager,
};
use shacs_core::tools::{ProposeEntrustedWorkTool, ToolRegistry};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};

#[path = "entrusted_work_conversation/commands.rs"]
mod commands;
#[path = "entrusted_work_conversation/proposals.rs"]
mod proposals;

struct MockProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
}

impl MockProvider {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

impl ProviderClient for MockProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn inbound(channel: &str, sender: &str, chat_id: &str, content: &str) -> InboundMessage {
    InboundMessage::new(channel, sender, chat_id, content)
}

fn proposal_response(call_id: &str, instructions: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            call_id,
            "propose_entrusted_work",
            Map::from_iter([
                ("goal_text".to_owned(), json!("monitor release readiness")),
                ("instructions".to_owned(), json!(instructions)),
                ("turn_budget".to_owned(), json!(4)),
            ]),
        )],
        ..LlmResponse::default()
    }
}

fn text_response(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        ..LlmResponse::default()
    }
}

fn pending_proposal_id(
    runtime: &AgentLoop<'_>,
    session_key: &str,
) -> Result<String, Box<dyn Error>> {
    runtime
        .session_manager()
        .read_session_file(session_key)
        .and_then(|raw| {
            raw["metadata"]["pending_entrusted_work_proposal"]["proposal_id"]
                .as_str()
                .map(str::to_owned)
        })
        .ok_or_else(|| "missing proposal id".into())
}

fn backticked_id(text: &str) -> Option<&str> {
    text.split('`').nth(1)
}

fn provider_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
