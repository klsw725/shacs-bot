use super::*;

impl AgentLoop<'_> {
    pub(super) fn cancel_method_pending(
        &mut self,
        session: &mut Session,
        request: &mut MethodRequest,
    ) -> Result<Vec<&'static str>, AgentLoopError> {
        let mut cancelled = Vec::new();
        if self.cancel_pending_workflow(session, request, ("method_changed", "method-change"))? {
            cancelled.push("workflow");
        }
        if let Some(id) = pending_ask_user_id(&session.messages) {
            if let Some(question) = pending_question(session, &id) {
                request.remaining_questions.push(preview(&question));
            }
            append_ask_user_resume(session, &id, "{\"decision\":\"cancelled\",\"reason\":\"method_changed\",\"permission_granted\":false}");
            cancelled.push("question");
        }
        if let Some(approval) = pending_permission_approval(session) {
            record_permission_approval_terminal(
                session,
                &approval,
                shacs_session::PermissionApprovalTerminalState::Rejected,
            );
            // The call never runs: its part records that, before the selection moves on.
            super::super::method_record::record_denied(
                request,
                &approval.tool_call,
                "Error: tool call cancelled before it ran (the method changed).",
            );
            append_session_message(
                session,
                RuntimeToolMessage {
                    tool_call_id: approval.tool_call.id,
                    name: approval.tool_call.name,
                    content: "Permission approval Rejected: method_changed; tool not executed."
                        .to_owned(),
                }
                .to_json(),
            );
            session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
            cancelled.push("approval");
        }
        if let Some(approval) = pending_recent_retry_approval(session) {
            self.recent_retry_tokens.invalidate(&approval.denial_id);
            session.record_permission_approval_receipt(shacs_session::PermissionApprovalReceipt {
                approval_request_id: approval.approval_request_id,
                action_digest: approval.action_digest,
                snapshot_digest: approval.snapshot_digest,
                state: shacs_session::PermissionApprovalTerminalState::Rejected,
                recorded_at_unix_ms: now_unix_ms(),
            });
            session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
            cancelled.push("retry approval");
        }
        action::cancel(session);
        clear_runtime_markers(session);
        Ok(cancelled)
    }
}

impl AgentLoop<'_> {
    /// Records a pending workflow as cancelled for `reason`, durably first,
    /// under a turn id starting with `turn`; the caller removes the pending
    /// marker.
    pub(super) fn cancel_pending_workflow(
        &mut self,
        session: &mut Session,
        request: &MethodRequest,
        (reason, turn): (&str, &str),
    ) -> Result<bool, AgentLoopError> {
        let Some(pending) = pending_workflow(session) else {
            return Ok(false);
        };
        let provenance =
            durable_event_provenance(&self.context_builder, &RuntimeExecutionLedger::default());
        let turn_id = format!(
            "{turn}:{}:{}",
            request.request_id, request.selection.revision
        );
        let state = self.ensure_durable_workflow_planned(
            &session.key,
            turn_id.clone(),
            &pending.plan,
            provenance.clone(),
        )?;
        if state != WorkflowDurableFactState::Terminal {
            self.append_durable_fact(&session.key, Some(turn_id), WORKFLOW_FAILED,
                json!({"workflow_id":pending.plan.workflow_id,"state":WorkflowRunState::Cancelled,"reason":reason}), Some(provenance))?;
        }
        session.metadata.insert("runtime_workflow".to_owned(), json!({
            "workflow_id":pending.plan.workflow_id,"state":WorkflowRunState::Cancelled,"reason":reason,
            "method_request_id":request.request_id,"selection_revision":request.selection.revision
        }));
        Ok(true)
    }
}

pub(crate) fn pending_question(session: &Session, id: &str) -> Option<String> {
    for message in session.messages.iter().rev() {
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if call.get("id").and_then(Value::as_str) == Some(id) {
                let args = call.get("function")?.get("arguments")?;
                let args = match args {
                    Value::String(text) => serde_json::from_str(text).ok()?,
                    value => value.clone(),
                };
                return args
                    .get("question")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        }
    }
    None
}
