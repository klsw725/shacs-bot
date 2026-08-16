use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::model::ModelInfo;
use crate::types::{GenerationSettings, LlmResponse};
use crate::ProviderMediaLifecycleObservation;
use serde_json::Value;
use shacs_config::RawCredential;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        delta: String,
    },
    ToolCallReady {
        id: String,
        name: String,
        input: Value,
    },
    Finish {
        usage: Value,
        reason: String,
    },
    MediaLifecycle(ProviderMediaLifecycleObservation),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRequest {
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub model: String,
    pub settings: GenerationSettings,
    pub tool_choice: Option<Value>,
}

#[derive(Clone)]
pub struct ProviderInvocation {
    runtime_override: Option<RawCredential>,
    cancellation: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

impl ProviderInvocation {
    pub fn new(runtime_override: Option<RawCredential>, cancellation: Arc<AtomicBool>) -> Self {
        Self {
            runtime_override,
            cancellation,
            deadline: None,
        }
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn runtime_override(&self) -> Option<&RawCredential> {
        self.runtime_override.as_ref()
    }

    pub fn uncancelled(runtime_override: Option<RawCredential>) -> Self {
        Self::new(runtime_override, Arc::new(AtomicBool::new(false)))
    }

    pub fn cancellation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancellation)
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for ProviderInvocation {
    fn default() -> Self {
        Self::uncancelled(None)
    }
}

impl fmt::Debug for ProviderInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderInvocation")
            .field(
                "runtime_override",
                &self.runtime_override.as_ref().map(|_| "[REDACTED]"),
            )
            .field("cancellation", &"[SHARED]")
            .finish()
    }
}

pub trait ProviderClient: Send + Sync {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError>;

    fn chat_with_invocation(
        &self,
        request: ProviderRequest,
        _invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError>;

    fn chat_stream_with_invocation(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        _invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        self.chat_stream(request, on_event)
    }
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn config(&self) -> &ProviderConfig;
    fn default_model(&self) -> &str;
    fn supports_progress_deltas(&self) -> bool {
        false
    }
    fn model_info(&self, model: &str) -> Option<ModelInfo>;
}
