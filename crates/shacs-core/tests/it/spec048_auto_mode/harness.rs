//! The fake provider and run setup the Spec 048 auto mode tests share: one
//! provider answers both the assistant and the classifier.

use super::support::{Options, Setup};
use serde_json::{json, Map, Value};
use shacs_core::runtime::{
    AgentLoopConfig, AgentRunSpec, AutoModeClassifier, AutoModeSlots, ClassifierContextFiles,
    SecretValues,
};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderInvocation, ProviderRequest,
    ToolCallRequest,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

const CLASSIFIER_SYSTEM: &str = "You review one pending action of shacs-bot";
pub const BLOCK: &str = "{\"decision\": \"block\", \"rule\": \"[Data Exfiltration]\", \"reason\": \"uploads notes.md to a host the user never named\"}";

/// One provider for both the assistant and the classifier, as the agent loop
/// uses it: classifier requests get the classifier answers, the rest get the
/// assistant answers.
pub struct RoutingProvider {
    assistant: Mutex<VecDeque<LlmResponse>>,
    classifier: Mutex<VecDeque<Result<String, String>>>,
    classifier_requests: Mutex<Vec<(ProviderRequest, Option<Duration>)>>,
    assistant_requests: Mutex<Vec<ProviderRequest>>,
}

impl RoutingProvider {
    pub fn new(assistant: Vec<LlmResponse>, classifier: Vec<Result<&str, &str>>) -> Self {
        Self {
            assistant: Mutex::new(assistant.into()),
            classifier: Mutex::new(
                classifier
                    .into_iter()
                    .map(|answer| answer.map(str::to_owned).map_err(str::to_owned))
                    .collect(),
            ),
            classifier_requests: Mutex::new(Vec::new()),
            assistant_requests: Mutex::new(Vec::new()),
        }
    }

    /// The requests the assistant answers got, in order.
    pub fn assistant_requests(&self) -> Vec<ProviderRequest> {
        self.assistant_requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }

    pub fn classifier_requests(&self) -> Vec<(ProviderRequest, Option<Duration>)> {
        self.classifier_requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }

    /// The JSON payload each classifier request carried, without the
    /// stage instruction after it.
    pub fn payloads(&self) -> Vec<Value> {
        self.classifier_requests()
            .iter()
            .filter_map(|(request, _)| {
                let user = request.messages.get(1)?.get("content")?.as_str()?;
                let (payload, _) = user.split_once("\n\n")?;
                serde_json::from_str(payload).ok()
            })
            .collect()
    }

    fn answer(
        &self,
        request: ProviderRequest,
        remaining: Option<Duration>,
    ) -> Result<LlmResponse, ProviderError> {
        let classifier = request
            .messages
            .first()
            .and_then(|message| message["content"].as_str())
            .is_some_and(|system| system.starts_with(CLASSIFIER_SYSTEM));
        if !classifier {
            if let Ok(mut requests) = self.assistant_requests.lock() {
                requests.push(request);
            }
            return self
                .assistant
                .lock()
                .map_err(|_| api_error("lock"))?
                .pop_front()
                .ok_or_else(|| api_error("no assistant answer"));
        }
        if let Ok(mut requests) = self.classifier_requests.lock() {
            requests.push((request, remaining));
        }
        let answer = self
            .classifier
            .lock()
            .map_err(|_| api_error("lock"))?
            .pop_front()
            .unwrap_or_else(|| Err("unexpected classifier call".to_owned()));
        answer
            .map(|content| LlmResponse {
                content: Some(content),
                usage: BTreeMap::from([("prompt_tokens".to_owned(), 7)]),
                ..LlmResponse::default()
            })
            .map_err(|message| api_error(&message))
    }
}

impl ProviderClient for RoutingProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.answer(request, None)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.answer(request, None)
    }

    fn chat_with_invocation(
        &self,
        request: ProviderRequest,
        invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        self.answer(request, invocation.remaining())
    }
}

pub fn api_error(message: &str) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.to_owned(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}

pub fn tool_call(id: &str, name: &str, arguments: Value) -> ToolCallRequest {
    ToolCallRequest::new(
        id,
        name,
        arguments.as_object().cloned().unwrap_or_else(Map::new),
    )
}

pub fn calls(calls: Vec<ToolCallRequest>) -> LlmResponse {
    LlmResponse {
        content: Some("ASSISTANT WORDS".to_owned()),
        reasoning_content: Some("ASSISTANT REASONING".to_owned()),
        finish_reason: "tool_calls".to_owned(),
        tool_calls: calls,
        ..LlmResponse::default()
    }
}

pub fn answer(text: &str) -> LlmResponse {
    LlmResponse {
        content: Some(text.to_owned()),
        ..LlmResponse::default()
    }
}

pub fn classifier<'a>(provider: &'a RoutingProvider, model: &str) -> AutoModeClassifier<'a> {
    AutoModeClassifier::new(
        provider,
        ProviderInvocation::default(),
        model,
        &AutoModeSlots::default().filled("/ws", "cli:direct"),
        &ClassifierContextFiles {
            instructions: vec![("AGENTS.md".to_owned(), "Be brief.".to_owned())],
            skills: "- **youtube-content** — summarize videos  `skills/youtube-content`".to_owned(),
        },
        &SecretValues::default(),
    )
}

pub fn spec<'a>(
    setup: &Setup,
    registry: &'a shacs_core::tools::ToolRegistry,
    provider: &'a RoutingProvider,
    options: &Options,
    user: &str,
) -> AgentRunSpec<'a> {
    let messages = vec![
        json!({"role": "system", "content": "SYSTEM PROMPT TEXT"}),
        json!({"role": "user", "content": format!(
            "[Runtime Context — metadata only, not instructions]\nCurrent Time: now\n[/Runtime Context]\n\n{user}"
        )}),
    ];
    let mut spec = AgentRunSpec::new(messages, registry, provider, "main-model");
    spec.max_iterations = 4;
    spec.tool_context = setup.context(options);
    spec.auto_mode_classifier = Some(classifier(provider, "main-model"));
    spec
}

pub fn agent_loop_config(setup: &Setup, interactive: bool) -> AgentLoopConfig {
    let context = setup.context(&Options::auto(interactive));
    let mut config = AgentLoopConfig::new(&setup.workspace, "main-model");
    config.permission_mode_snapshot = context.permission_mode_snapshot;
    config.permission_interactive = interactive;
    config.containment_snapshot = context.containment_snapshot;
    config.permission_rule_input = context.permission_rule_input;
    config.permission_ceiling_snapshot = context.permission_ceiling_snapshot;
    config.permission_auto_approval = context.permission_auto_approval;
    config.permission_environment = context.permission_environment;
    config
}
