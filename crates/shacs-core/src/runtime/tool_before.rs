use crate::runtime::RuntimeToolCall;
use shacs_projection::{
    HookDenialProjection, HookDenialReason, HookDiagnosticKind, HookDiagnosticProjection,
    HookFailureBehavior, HookRuntimeProjection, HookRuntimeStatus, Spec030Availability,
};
use std::cmp::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const DEFAULT_TRUSTED_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBeforeOrderKey(String);

impl ToolBeforeOrderKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl Ord for ToolBeforeOrderKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for ToolBeforeOrderKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolBeforeDecision {
    Allow,
    Block { reason: String },
    InvalidOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBeforeConfirmRequest {
    pub call_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBeforeSelectRequest {
    pub call_id: String,
    pub prompt: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBeforeNotifyRequest {
    pub call_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolBeforeConfirmation {
    Confirmed,
    Denied,
    HeadlessDenied,
}

pub trait ToolBeforeInteraction: Send + Sync {
    fn confirm(&self, request: &ToolBeforeConfirmRequest) -> ToolBeforeConfirmation;

    fn select(&self, request: &ToolBeforeSelectRequest) -> Option<String>;

    fn notify(&self, request: &ToolBeforeNotifyRequest);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeadlessToolBeforeInteraction;

impl ToolBeforeInteraction for HeadlessToolBeforeInteraction {
    fn confirm(&self, _request: &ToolBeforeConfirmRequest) -> ToolBeforeConfirmation {
        ToolBeforeConfirmation::HeadlessDenied
    }

    fn select(&self, _request: &ToolBeforeSelectRequest) -> Option<String> {
        None
    }

    fn notify(&self, _request: &ToolBeforeNotifyRequest) {}
}

pub struct ToolBeforeContext<'a> {
    call: &'a RuntimeToolCall,
    interaction: &'a dyn ToolBeforeInteraction,
}

impl<'a> ToolBeforeContext<'a> {
    pub(crate) const fn new(
        call: &'a RuntimeToolCall,
        interaction: &'a dyn ToolBeforeInteraction,
    ) -> Self {
        Self { call, interaction }
    }

    pub const fn call(&self) -> &RuntimeToolCall {
        self.call
    }

    pub fn confirm(&self, prompt: impl Into<String>) -> ToolBeforeConfirmation {
        self.interaction.confirm(&ToolBeforeConfirmRequest {
            call_id: self.call.id.clone(),
            prompt: prompt.into(),
        })
    }

    pub fn select(&self, prompt: impl Into<String>, options: Vec<String>) -> Option<String> {
        self.interaction.select(&ToolBeforeSelectRequest {
            call_id: self.call.id.clone(),
            prompt: prompt.into(),
            options,
        })
    }

    pub fn notify(&self, message: impl Into<String>) {
        self.interaction.notify(&ToolBeforeNotifyRequest {
            call_id: self.call.id.clone(),
            message: message.into(),
        });
    }
}

pub trait ToolBeforeHandler: Send + Sync {
    fn hook_ref(&self) -> &str;

    fn order_key(&self) -> ToolBeforeOrderKey;

    fn timeout(&self) -> Duration {
        DEFAULT_TRUSTED_HOOK_TIMEOUT
    }

    fn evaluate(&self, context: &ToolBeforeContext<'_>) -> ToolBeforeDecision;
}

#[derive(Clone)]
pub(crate) struct ToolBeforeRuntimeFacts {
    registered_handlers: u32,
    state: Arc<Mutex<ToolBeforeRuntimeState>>,
}

#[derive(Default)]
struct ToolBeforeRuntimeState {
    diagnostics: Vec<HookDiagnosticProjection>,
    denials: Vec<HookDenialProjection>,
}

impl ToolBeforeRuntimeFacts {
    pub(crate) fn new(registered_handlers: usize) -> Self {
        Self {
            registered_handlers: u32::try_from(registered_handlers).unwrap_or(u32::MAX),
            state: Arc::new(Mutex::new(ToolBeforeRuntimeState::default())),
        }
    }

    pub(crate) fn diagnostic(&self, hook_ref: &str, kind: HookDiagnosticKind) {
        self.lock().diagnostics.push(HookDiagnosticProjection {
            hook_ref: hook_ref.to_owned(),
            kind,
            behavior: HookFailureBehavior::ContinuedFailOpen,
        });
    }

    pub(crate) fn denial(&self, hook_ref: &str, call_ref: &str, reason: HookDenialReason) {
        self.lock().denials.push(HookDenialProjection {
            hook_ref: hook_ref.to_owned(),
            call_ref: call_ref.to_owned(),
            reason,
        });
    }

    pub(crate) fn projection(&self) -> HookRuntimeProjection {
        let state = self.lock();
        HookRuntimeProjection {
            availability: Spec030Availability::Available,
            status: if self.registered_handlers == 0 {
                HookRuntimeStatus::Inactive
            } else {
                HookRuntimeStatus::Active
            },
            registered_handlers: self.registered_handlers,
            diagnostics: state.diagnostics.clone(),
            recent_denials: state.denials.clone(),
        }
    }

    pub(crate) fn restore_history(&self, projection: &HookRuntimeProjection) {
        let mut state = self.lock();
        state.diagnostics = projection.diagnostics.clone();
        state.denials = projection.recent_denials.clone();
    }

    fn lock(&self) -> MutexGuard<'_, ToolBeforeRuntimeState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}
