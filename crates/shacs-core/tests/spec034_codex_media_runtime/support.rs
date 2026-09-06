use super::RAW_PROVIDER_ITEM_ID;
use serde_json::{json, Map};
use shacs_config::PermissionMode;
use shacs_core::runtime::{
    AgentHook, AgentHookContext, ContainerNetworkMode, ContainerRuntimeKind,
    ContainmentSnapshotRef, DockerContainmentSnapshot, PermissionModeSnapshot, PermissionRuleInput,
    RuntimeToolCall, RuntimeToolMessage, ToolExecutionContext,
};
use shacs_core::tools::ImageGenerateToolConfig;
use shacs_providers::{
    GeneratedImage, ImageGenerationClient, ImageGenerationItemId, ImageGenerationRequest,
    ImageGenerationResult, ImageMimeType, LlmResponse, ProviderClient, ProviderError,
    ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(super) struct FixtureImageClient {
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) stage: Arc<AtomicUsize>,
}

impl ImageGenerationClient for FixtureImageClient {
    fn generate_image(
        &self,
        _request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        if self.stage.load(Ordering::SeqCst) != 2 {
            return Err(provider_error(
                "native request preceded snapshot/hook gates",
            ));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ImageGenerationResult {
            provider_id: "openai_codex".to_owned(),
            model: "gpt-5.6".to_owned(),
            images: vec![GeneratedImage {
                index: 0,
                mime_type: ImageMimeType::Png,
                bytes: b"raw-image-secret".to_vec(),
                byte_len: b"raw-image-secret".len(),
                revised_prompt: None,
                provider_item_id: Some(ImageGenerationItemId::from_provider(RAW_PROVIDER_ITEM_ID)),
            }],
            remote_images: Vec::new(),
            usage: None,
            request_id: None,
        })
    }
}

pub(super) struct FixtureProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
    pub(super) calls: AtomicUsize,
}

impl FixtureProvider {
    pub(super) fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            calls: AtomicUsize::new(0),
        }
    }
}

impl ProviderClient for FixtureProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .ok_or_else(|| provider_error("unexpected provider retry"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

pub(super) struct AdmissionHook {
    pub(super) stage: Arc<AtomicUsize>,
    pub(super) block: bool,
}

impl AgentHook for AdmissionHook {
    fn receives_tool_arguments(&self) -> bool {
        true
    }

    fn block_tool_calls(
        &self,
        _context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        if self.block {
            return calls
                .iter()
                .map(|call| RuntimeToolMessage {
                    tool_call_id: call.id.clone(),
                    name: call.name.clone(),
                    content: "Error: blocked by fixture hook".to_owned(),
                })
                .collect();
        }
        if self.stage.load(Ordering::SeqCst) == 1 {
            self.stage.store(2, Ordering::SeqCst);
        }
        Vec::new()
    }
}

pub(super) fn tool_call_response() -> LlmResponse {
    LlmResponse {
        tool_calls: vec![ToolCallRequest::new(
            "call_image",
            "image_generate",
            Map::from_iter([("prompt".to_owned(), json!("draw"))]),
        )],
        finish_reason: "tool_calls".to_owned(),
        ..LlmResponse::default()
    }
}

pub(super) fn bypass_context() -> ToolExecutionContext {
    ToolExecutionContext {
        containment_snapshot: Some(ContainmentSnapshotRef {
            contained: Some(true),
            backend: Some("docker".to_owned()),
            digest: Some("spec034-contained".to_owned()),
            summary: Some("non-privileged fixture containment".to_owned()),
        }),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("spec034 fixture".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: Vec::new(),
                network_mode: ContainerNetworkMode::None,
                digest: Some("spec034-contained".to_owned()),
                summary: Some("non-privileged fixture containment".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        },
        ..ToolExecutionContext::default()
    }
}

pub(super) fn tool_config() -> ImageGenerateToolConfig {
    ImageGenerateToolConfig {
        provider_id: "openai_codex".to_owned(),
        model_id: "gpt-5.6".to_owned(),
        default_format: "png".to_owned(),
        max_count: 1,
        max_bytes: 1024,
    }
}

pub(super) fn provider_error(message: &str) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.to_owned(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
