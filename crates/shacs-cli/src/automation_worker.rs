use super::{durable_work_state_for_owner, now_millis, AgentLoopChatCompletionAdapter};
use shacs_core::runtime::{
    AgentHookContext, AutomationConfirmationFact, AutomationDeliveryResult,
    AutomationDispatchRequest, AutomationExecutionControl, AutomationExecutionReceipt,
    AutomationExecutionTerminalFact, AutomationExecutor, AutomationGateResolution,
    AutomationGateResolver, AutomationHookEvaluation, AutomationJobResult,
    AutomationProcessCleanupFact, DurableWorkDispatcher, ExecutionSnapshot,
    PluginHookDispatchRecord, PluginRuntimeHookAgentHook, RuntimeToolCall,
};
use shacs_eval::evaluator::AutomationExecutionMode;
use shacs_providers::{GenerationSettings, ProviderRequest};
use std::path::Path;

mod trajectory_recorder;
pub(super) use trajectory_recorder::record_no_provider_trajectory;

pub(super) trait AutomationOwnerAdapters {
    fn snapshot(&self, request: &ProviderRequest) -> Result<ExecutionSnapshot, String>;
    fn hooks(&self, request: &AutomationDispatchRequest) -> AutomationHookEvaluation;
    fn execute(
        &self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt;
}

impl AutomationOwnerAdapters for AgentLoopChatCompletionAdapter {
    fn snapshot(&self, request: &ProviderRequest) -> Result<ExecutionSnapshot, String> {
        self.execution_snapshot_source()
            .resolve(request, Vec::new(), None)
            .and_then(ExecutionSnapshot::create)
            .map_err(|error| error.to_string())
    }

    fn hooks(&self, request: &AutomationDispatchRequest) -> AutomationHookEvaluation {
        if !request.requirements.execution_sensitive {
            return AutomationHookEvaluation {
                records: Vec::new(),
                denial: None,
            };
        }
        #[cfg(not(test))]
        let hook = PluginRuntimeHookAgentHook::new(self.plugin_runtime_snapshot.clone())
            .with_trusted_handlers(self.trusted_tool_before_handlers.clone())
            .with_spec030_fact_store(self.spec030_provider.fact_store());
        #[cfg(test)]
        let hook = PluginRuntimeHookAgentHook::new(self.plugin_runtime_snapshot.clone());
        let call = RuntimeToolCall::new(
            request.work_id.clone(),
            automation_tool_name(&request.run.execution_mode),
            serde_json::json!({"instruction": request.instruction}),
        );
        let context = AgentHookContext {
            iteration: 0,
            messages: Vec::new(),
        };
        let diagnostics_before = hook.hook_runtime_projection().diagnostics.len();
        let summary = hook.dispatch_tool_before(&context, std::slice::from_ref(&call));
        let projection = hook.hook_runtime_projection();
        let diagnostic_failure = (projection.diagnostics.len() > diagnostics_before)
            .then_some(shacs_projection::HookDenialReason::HookFailed);
        AutomationHookEvaluation {
            records: summary.map_or_else(
                || vec![PluginHookDispatchRecord::successful_noop()],
                |summary| summary.records,
            ),
            denial: current_hook_denial(&projection, &call.id).or(diagnostic_failure),
        }
    }

    fn execute(
        &self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        let result = match request.run.execution_mode {
            AutomationExecutionMode::SkillBackedAgent => request
                .instruction
                .as_deref()
                .ok_or_else(|| "automation:missing_instruction".to_owned())
                .and_then(|instruction| {
                    self.execute_agent_automation(&request, instruction, control.clone())
                }),
            AutomationExecutionMode::NoAgentCheck if !request.requirements.execution_sensitive => {
                Ok(format!("automation:check:{}", request.work_id))
            }
            AutomationExecutionMode::NoAgentCheck => {
                Err("automation:unsupported_effectful_no_agent".to_owned())
            }
            AutomationExecutionMode::ScriptOnly => {
                Err("automation:unsupported_script_adapter".to_owned())
            }
            AutomationExecutionMode::AppTask => {
                Err("automation:unsupported_app_adapter".to_owned())
            }
        };
        let mut job_result = match result {
            Ok(result_ref) => AutomationJobResult::Succeeded { result_ref },
            Err(reason_ref) => AutomationJobResult::Failed { reason_ref },
        };
        if control.deadline_elapsed() {
            job_result = AutomationJobResult::TimedOut {
                timeout_ref: control.timeout_ref().to_owned(),
            };
        } else if control.is_cancelled() {
            job_result = AutomationJobResult::Cancelled {
                reason_ref: "automation:cancelled".to_owned(),
            };
        }
        let task_outcome = match self
            .config_path
            .parent()
            .ok_or_else(|| "automation:data_dir_unavailable".to_owned())
            .and_then(|data_dir| {
                super::automation_outcome::evaluate_and_route(
                    data_dir,
                    &self.workspace,
                    &request,
                    &job_result,
                )
            }) {
            Ok(record) => Some(record),
            Err(reason_ref) => {
                job_result = AutomationJobResult::Failed {
                    reason_ref: format!("automation:task_outcome_route:{reason_ref}"),
                };
                None
            }
        };
        AutomationExecutionReceipt {
            terminal_fact: match &job_result {
                AutomationJobResult::Succeeded { .. } => AutomationExecutionTerminalFact::Completed,
                AutomationJobResult::Failed { reason_ref } => {
                    AutomationExecutionTerminalFact::Failed {
                        reason_ref: reason_ref.clone(),
                    }
                }
                AutomationJobResult::TimedOut { timeout_ref } => {
                    AutomationExecutionTerminalFact::TimedOut {
                        timeout_ref: timeout_ref.clone(),
                    }
                }
                AutomationJobResult::Cancelled { reason_ref } => {
                    AutomationExecutionTerminalFact::Cancelled {
                        reason_ref: reason_ref.clone(),
                    }
                }
                AutomationJobResult::Pending => AutomationExecutionTerminalFact::Failed {
                    reason_ref: "automation:pending_terminal".to_owned(),
                },
            },
            job_result,
            delivery_result: AutomationDeliveryResult::NotRequested,
            process_receipt: None,
            process_cleanup: AutomationProcessCleanupFact::NotRequired,
            task_outcome,
        }
    }
}

fn current_hook_denial(
    projection: &shacs_projection::HookRuntimeProjection,
    call_ref: &str,
) -> Option<shacs_projection::HookDenialReason> {
    projection
        .recent_denials
        .iter()
        .rev()
        .find(|denial| denial.call_ref == call_ref)
        .map(|denial| denial.reason)
}

pub(super) fn process_due_automation(
    dispatcher: &mut DurableWorkDispatcher,
    data_dir: &Path,
    adapters: &dyn AutomationOwnerAdapters,
) -> Result<(), String> {
    super::automation_outcome::consume_verification_requests(dispatcher, data_dir)?;
    let (state, admission) = durable_work_state_for_owner(data_dir, dispatcher.lease_owner_ref())
        .map_err(|error| error.to_string())?;
    let mut gates = ProductionGateResolver { adapters };
    let mut executor = ProductionAutomationExecutor { adapters };
    dispatcher
        .dispatch_due_automation(
            &state.work,
            &admission,
            now_millis(),
            &mut gates,
            &mut executor,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

struct ProductionGateResolver<'a> {
    adapters: &'a dyn AutomationOwnerAdapters,
}

impl AutomationGateResolver for ProductionGateResolver<'_> {
    fn resolve(&mut self, request: &AutomationDispatchRequest) -> AutomationGateResolution {
        let adapter_supported = matches!(
            request.run.execution_mode,
            AutomationExecutionMode::SkillBackedAgent | AutomationExecutionMode::NoAgentCheck
        );
        let provider = ProviderRequest {
            messages: vec![serde_json::json!({
                "role": "user",
                "content": request.instruction.as_deref().unwrap_or_default(),
            })],
            tools: Vec::new(),
            model: "automation".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        };
        let hooks = self.adapters.hooks(request);
        AutomationGateResolution {
            execution_snapshot: self.adapters.snapshot(&provider).ok(),
            hook_evidence: hooks.records,
            hook_denial: hooks.denial,
            adapter_supported,
            requirements: shacs_core::runtime::AutomationExecutionRequirements {
                confirmation: match request.requirements.confirmation {
                    AutomationConfirmationFact::NotRequired => {
                        AutomationConfirmationFact::NotRequired
                    }
                    AutomationConfirmationFact::Confirmed
                    | AutomationConfirmationFact::Denied
                    | AutomationConfirmationFact::HeadlessDenied => {
                        AutomationConfirmationFact::HeadlessDenied
                    }
                },
                ..request.requirements.clone()
            },
        }
    }
}

struct ProductionAutomationExecutor<'a> {
    adapters: &'a dyn AutomationOwnerAdapters,
}

impl AutomationExecutor for ProductionAutomationExecutor<'_> {
    fn execute(
        &mut self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        self.adapters.execute(request, control)
    }
}

impl AgentLoopChatCompletionAdapter {
    pub(super) fn automation_snapshot(
        &self,
        instruction: &str,
    ) -> Result<ExecutionSnapshot, String> {
        self.snapshot(&ProviderRequest {
            messages: vec![serde_json::json!({"role": "user", "content": instruction})],
            tools: Vec::new(),
            model: "automation".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        })
    }

    fn execute_agent_automation(
        &self,
        request: &AutomationDispatchRequest,
        instruction: &str,
        control: AutomationExecutionControl,
    ) -> Result<String, String> {
        let provider_request = ProviderRequest {
            messages: vec![serde_json::json!({"role": "user", "content": instruction})],
            tools: Vec::new(),
            model: self.resolved_model.clone(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        };
        let invocation = shacs_api::ChatCompletionInvocation {
            provider_request,
            requested_model: Some(self.configured_model.clone()),
            session_key: request.session_key.clone(),
            media_data_urls: Vec::new(),
            media_paths: Vec::new(),
            temperature: None,
            max_tokens: None,
        };
        self.run_agent_loop_turn_with_origin_control(
            super::ProviderTurnInvocation {
                chat: invocation,
                runtime_override: None,
            },
            None,
            super::ProviderTurnOrigin {
                channel: "automation",
                sender_id: "system",
                chat_id: "automation",
            },
            &[],
            Some(control),
        )
        .map_err(|error| format!("automation:agent:{}", error.error_type))
        .map(|_| format!("automation:agent:{}", request.work_id))
    }
}

fn automation_tool_name(mode: &AutomationExecutionMode) -> &'static str {
    match mode {
        AutomationExecutionMode::SkillBackedAgent => "automation_agent",
        AutomationExecutionMode::ScriptOnly => "automation_script",
        AutomationExecutionMode::NoAgentCheck => "automation_check",
        AutomationExecutionMode::AppTask => "automation_app",
    }
}

#[cfg(test)]
mod tests {
    use super::current_hook_denial;
    use shacs_projection::{
        HookDenialProjection, HookDenialReason, HookDiagnosticProjection, HookRuntimeProjection,
        HookRuntimeStatus, Spec030Availability,
    };

    fn projection(denials: Vec<HookDenialProjection>) -> HookRuntimeProjection {
        HookRuntimeProjection {
            availability: Spec030Availability::Available,
            status: HookRuntimeStatus::Active,
            registered_handlers: 1,
            diagnostics: Vec::<HookDiagnosticProjection>::new(),
            recent_denials: denials,
        }
    }

    #[test]
    fn stale_denial_does_not_block_current_allowed_call() {
        let projection = projection(vec![HookDenialProjection {
            hook_ref: "hook:old".to_owned(),
            call_ref: "work:old".to_owned(),
            reason: HookDenialReason::ExtensionBlocked,
        }]);

        assert_eq!(current_hook_denial(&projection, "work:current"), None);
    }

    #[test]
    fn current_denial_blocks_and_preserves_reason() {
        let projection = projection(vec![HookDenialProjection {
            hook_ref: "hook:current".to_owned(),
            call_ref: "work:current".to_owned(),
            reason: HookDenialReason::HeadlessConfirmationDenied,
        }]);

        assert_eq!(
            current_hook_denial(&projection, "work:current"),
            Some(HookDenialReason::HeadlessConfirmationDenied)
        );
    }
}
