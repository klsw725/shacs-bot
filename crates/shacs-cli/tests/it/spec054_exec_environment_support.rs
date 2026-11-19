use serde_json::json;
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::{
    load_runtime_config_with_env, AgentLoopChatCompletionAdapter, RuntimeConfigOptions,
};
use shacs_config::ConfigBundle;
use shacs_core::runtime::{
    BrowserCallContext, BrowserError, BrowserHost, BrowserSession, ContainerNetworkMode,
    ContainerRuntimeKind, ContainmentSnapshotRef, DockerContainmentSnapshot,
};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest,
    ToolCallRequest,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;
use std::sync::{mpsc, Arc};

pub(super) type TestResult = Result<(), Box<dyn Error>>;

pub(super) fn assert_success(results: &mpsc::Receiver<String>) -> TestResult {
    let result = results.recv_timeout(std::time::Duration::from_secs(10))?;
    assert!(result.contains("credential-ok"), "{result}");
    Ok(())
}

struct NoBrowser;

impl BrowserHost for NoBrowser {
    fn acquire(&self, _: &BrowserCallContext) -> Result<Box<dyn BrowserSession>, BrowserError> {
        Err(BrowserError::ContextUnavailable)
    }
}

pub(super) struct Provider {
    pub(super) results: mpsc::Sender<String>,
}

impl ProviderClient for Provider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let system = request
            .messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or_default();
        if system.starts_with("You check one tool result") || system.starts_with("You review ") {
            return Ok(LlmResponse {
                content: Some("no".to_owned()),
                ..LlmResponse::default()
            });
        }
        if let Some(result) = request
            .messages
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
        {
            self.results
                .send(result["content"].as_str().unwrap_or_default().to_owned())
                .expect("test result receiver remains alive");
            return Ok(LlmResponse {
                content: Some("done".to_owned()),
                ..LlmResponse::default()
            });
        }
        let user = request
            .messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or_default();
        let (name, arguments) = if user.contains("spawn-parent") {
            (
                "spawn",
                json!({"task": "check-startup", "label": "credential-child"}),
            )
        } else {
            let script = if user.contains("check-missing") {
                "check-missing.sh"
            } else if user.contains("check-rotated") {
                "check-rotated.sh"
            } else {
                "check-startup.sh"
            };
            ("exec", json!({"command": format!("sh {script}")}))
        };
        Ok(LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "credential-call",
                name,
                arguments.as_object().cloned().unwrap_or_default(),
            )],
            ..LlmResponse::default()
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

pub(super) fn run(
    adapter: &AgentLoopChatCompletionAdapter,
    content: &str,
    session: &str,
) -> TestResult {
    adapter.complete_chat(ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role": "user", "content": content})],
            tools: Vec::new(),
            model: "fixture".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: None,
        session_key: session.to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    })?;
    Ok(())
}

pub(super) fn load(
    root: &Path,
    process: &BTreeMap<String, String>,
) -> Result<ConfigBundle, Box<dyn Error>> {
    Ok(load_runtime_config_with_env(
        RuntimeConfigOptions {
            config_path: Some(root.join("config.json")),
            workspace_override: None,
            resolve_env: true,
        },
        process,
    )?)
}

pub(super) fn adapter(
    bundle: ConfigBundle,
    provider: &Arc<Provider>,
) -> Result<AgentLoopChatCompletionAdapter, Box<dyn Error>> {
    Ok(
        AgentLoopChatCompletionAdapter::from_bundle(bundle, true)?.with_browser_clients(
            provider.clone(),
            Arc::new(NoBrowser),
            Some((
                ContainmentSnapshotRef {
                    contained: Some(true),
                    backend: Some("test-containment".to_owned()),
                    digest: Some("spec054-contained".to_owned()),
                    summary: None,
                },
                DockerContainmentSnapshot {
                    contained: Some(true),
                    runtime: ContainerRuntimeKind::Docker,
                    root_user: Some(false),
                    privileged: Some(false),
                    host_mounts_summary: Vec::new(),
                    network_mode: ContainerNetworkMode::None,
                    digest: Some("spec054-contained".to_owned()),
                    summary: None,
                },
            )),
        ),
    )
}
