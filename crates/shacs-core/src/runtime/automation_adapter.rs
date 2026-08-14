use super::{
    AutomationDeliveryResult, AutomationExecutionRequirements, AutomationJobResult,
    AutomationTaskOutcomeRecord, ExecutionSnapshot, PluginHookDispatchRecord,
    ProcessExecutionReceipt,
};
use serde::{Deserialize, Serialize};
use shacs_eval::evaluator::AutomationRunRequest;
use shacs_projection::HookDenialReason;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const AUTOMATION_RUNTIME_DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct AutomationExecutionControl {
    cancellation_token: super::CancellationToken,
    deadline: Instant,
    timeout_ref: String,
    deadline_elapsed: Arc<AtomicBool>,
}

impl AutomationExecutionControl {
    pub fn runtime_default() -> Self {
        Self::with_timeout("runtime-default", AUTOMATION_RUNTIME_DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(timeout_ref: impl Into<String>, timeout: Duration) -> Self {
        Self {
            cancellation_token: super::CancellationToken::new(),
            deadline: Instant::now() + timeout,
            timeout_ref: timeout_ref.into(),
            deadline_elapsed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancellation_token(&self) -> super::CancellationToken {
        self.cancellation_token.clone()
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn timeout_ref(&self) -> &str {
        &self.timeout_ref
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    pub fn mark_deadline_elapsed(&self) {
        self.deadline_elapsed.store(true, Ordering::SeqCst);
        self.cancel();
    }

    pub fn deadline_elapsed(&self) -> bool {
        self.deadline_elapsed.load(Ordering::SeqCst) || Instant::now() >= self.deadline
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }
}

impl Default for AutomationExecutionControl {
    fn default() -> Self {
        Self::runtime_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationExecutionTerminalFact {
    Completed,
    Failed { reason_ref: String },
    TimedOut { timeout_ref: String },
    Cancelled { reason_ref: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDispatchRequest {
    pub work_id: String,
    pub session_key: String,
    pub work_kind: String,
    pub dedupe_key: String,
    pub idempotency_key: String,
    pub run: AutomationRunRequest,
    pub requirements: AutomationExecutionRequirements,
    pub instruction: Option<String>,
    pub outcome_policy: super::AutomationOutcomePolicy,
    pub owner_target_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationHookEvaluation {
    pub records: Vec<PluginHookDispatchRecord>,
    pub denial: Option<HookDenialReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationProcessCleanupFact {
    NotRequired,
    Succeeded,
    Failed { reason_ref: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationExecutionReceipt {
    pub job_result: AutomationJobResult,
    pub terminal_fact: AutomationExecutionTerminalFact,
    pub delivery_result: AutomationDeliveryResult,
    pub process_receipt: Option<ProcessExecutionReceipt>,
    pub process_cleanup: AutomationProcessCleanupFact,
    pub task_outcome: Option<AutomationTaskOutcomeRecord>,
}

pub trait AutomationExecutor {
    fn execute(
        &mut self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt;
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationGateResolution {
    pub execution_snapshot: Option<ExecutionSnapshot>,
    pub hook_evidence: Vec<PluginHookDispatchRecord>,
    pub hook_denial: Option<HookDenialReason>,
    pub adapter_supported: bool,
    pub requirements: AutomationExecutionRequirements,
}

pub trait AutomationGateResolver {
    fn resolve(&mut self, request: &AutomationDispatchRequest) -> AutomationGateResolution;
}
