use super::{
    action, append_ask_user_resume, append_session_message, append_user_turn,
    clear_runtime_markers, durable_event_provenance, json, now_unix_ms, pending_ask_user_id,
    pending_permission_approval, pending_recent_retry_approval, pending_workflow,
    record_permission_approval_terminal, turn_id_for_message, AgentHook, AgentLoop, AgentLoopError,
    AgentLoopTurnResult, AgentRunSpec, InboundMessage, RuntimeExecutionLedger, RuntimeToolCall,
    RuntimeToolMessage, Session, ToolRegistry, WorkflowDurableFactState, WorkflowRunState,
    PENDING_PERMISSION_APPROVAL_KEY, PENDING_RECENT_RETRY_APPROVAL_KEY, PENDING_WORKFLOW_KEY,
    RUNTIME_EXECUTION_KEY,
};
use crate::runtime::task_method::{
    Gate, MethodId, MethodRequest, Reason, RequestState, Selection, SelectionSource,
};
use crate::runtime::task_method_apply::refresh;
use crate::runtime::task_method_classify::{availability, classify, intents, selection};
use crate::runtime::task_method_render::{
    availability_text, blocked, next_steps, projection, request_text, selection_text,
};
use crate::runtime::task_method_store::{preview, MethodStore};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use shacs_command::MethodCommandArgs;
use shacs_session::durable_event::{SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED};

#[path = "agent_loop_method_cancel.rs"]
pub(super) mod cancel;
#[path = "agent_loop_method_image.rs"]
pub(super) mod image;

pub(super) fn user_turn(message: &InboundMessage) -> bool {
    message.owner_accepted_notification().is_none()
        && message.owner_accepted_automation_result().is_none()
        && !matches!(
            message.channel.as_str(),
            "system" | "heartbeat" | "cron" | "automation"
        )
        && !message.metadata.contains_key("goal_continuation")
        && !message.metadata.contains_key("automation_route")
}

pub(super) fn project_context(session: &Session, messages: &mut [Value]) -> std::io::Result<()> {
    let mut store = MethodStore::load(session)?;
    if let Some(request) = store.active.as_mut() {
        refresh(request);
    }
    if let (Some(request), Some(system)) = (store.active.as_ref(), messages.first_mut()) {
        if system.get("role").and_then(Value::as_str) == Some("system") {
            if let Some(content) = system
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                system["content"] = Value::String(format!("{content}{}", projection(request)));
            }
        }
    }
    Ok(())
}

pub(super) fn request_in_progress(session: &Session) -> std::io::Result<bool> {
    Ok(MethodStore::load(session)?
        .active
        .is_some_and(|request| request.state == RequestState::InProgress))
}

pub(super) fn close_request(session: &mut Session, only_if_idle: bool) -> std::io::Result<()> {
    let mut store = MethodStore::load(session)?;
    let has_pending = pending_workflow(session).is_some()
        || pending_ask_user_id(&session.messages).is_some()
        || pending_permission_approval(session).is_some()
        || pending_recent_retry_approval(session).is_some();
    if store.active.is_none() || (only_if_idle && has_pending) {
        return Ok(());
    }
    store.close(RequestState::Cancelled);
    store.save(session)
}

impl AgentLoop<'_> {
    pub(super) fn handle_method_command(
        &mut self,
        args: MethodCommandArgs,
        message: &InboundMessage,
        mut session: Session,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        if matches!(
            args,
            MethodCommandArgs::Use(_)
                | MethodCommandArgs::Auto
                | MethodCommandArgs::Accept
                | MethodCommandArgs::Revise
        ) && !user_turn(message)
        {
            return self.publish_command_response(
                message,
                session,
                "Method changes are accepted only from user turns; method state is unchanged.",
                None,
                "method_rejected",
                true,
            );
        }
        let mut store = MethodStore::load(&session)?;
        let content = match args {
            MethodCommandArgs::Invalid => "Usage: /method [status|list|use <general|calculation|report|image>|auto|history|accept|revise]. Canonical ids: general, reproducible_calculation, source_grounded_report, image_generation_review.".to_owned(),
            MethodCommandArgs::Accept => super::method_record::accept(&mut store),
            MethodCommandArgs::List => [MethodId::General, MethodId::ReproducibleCalculation, MethodId::SourceGroundedReport, MethodId::ImageGenerationReview]
                .iter().map(|method| format!("{}: {}; planned checks (not verified): {}", method.canonical(), availability_text(&availability(*method, self.tools)), method.checks().join(", "))).collect::<Vec<_>>().join("\n"),
            MethodCommandArgs::Status => super::method_record::status(&mut store),
            MethodCommandArgs::Revise => super::method_record::revise(&mut store),
            MethodCommandArgs::History => {
                let mut entries = Vec::new();
                for request in store.closed.iter_mut().chain(store.active.iter_mut()) {
                    refresh(request);
                }
                for request in store.closed.iter().chain(store.active.iter()) {
                    entries.push(request_text(request));
                    entries.extend(request.superseded_selections.iter().map(selection_text));
                }
                if entries.is_empty() { "No method history recorded.".to_owned() } else { entries.join("\n\n") }
            }
            MethodCommandArgs::Use(name) => {
                if let Some(active) = store.active.as_ref() {
                    if active.selection.method == MethodId::from(name) {
                        format!("Method unchanged (idempotent).\n{}", request_text(active))
                    } else {
                        self.change_method(&mut session, &mut store, (name.into(), SelectionSource::Explicit, Reason::ExplicitOverride), &[])?
                    }
                } else {
                    store.pending_override = Some(name);
                    store.pending_revision = None;
                    let available = availability(name.into(), self.tools);
                    let warning = if available.blocked() { next_steps(name.into()) } else { "" };
                    format!("Next request only: {}. {}. No provider was called. {warning}", name.canonical(), availability_text(&available))
                }
            }
            MethodCommandArgs::Auto => {
                if let Some(active) = store.active.as_ref() {
                    let goal = session.messages.iter()
                        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
                        .filter_map(|message| message.get("content").and_then(Value::as_str))
                        .find(|content| format!("sha256:{:x}", Sha256::digest(content.as_bytes())) == active.goal_digest);
                    let (method, reason) = goal.map(classify).unwrap_or((active.automatic_method, active.automatic_reason));
                    let parts = goal.map(intents).unwrap_or_else(|| active.automatic_components.clone());
                    self.change_method(&mut session, &mut store, (method, SelectionSource::Auto, reason), &parts)?
                } else {
                    store.pending_override = None;
                    store.pending_revision = None;
                    "Next-request override cleared; automatic selection restored.".to_owned()
                }
            }
        };
        store.save(&mut session)?;
        self.publish_command_response(message, session, &content, None, "method", true)
    }

    fn change_method(
        &mut self,
        session: &mut Session,
        store: &mut MethodStore,
        intent: (MethodId, SelectionSource, Reason),
        parts: &[MethodId],
    ) -> Result<String, AgentLoopError> {
        let Some(active) = store.active.as_mut() else {
            return Ok("No active method request.".to_owned());
        };
        if active.superseded_selections.len() >= 128 {
            return Ok("Method revision capacity reached. Start a new request; the current selection and all retained revisions are unchanged.".to_owned());
        }
        let revision = active
            .selection
            .revision
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("method revision exhausted"))?;
        let next = selection(intent, parts, revision, self.tools);
        let Some(start) = image::reported_start(session, active, &next) else {
            return Ok(image::NOT_CLEARED.to_owned());
        };
        let cancelled = self.cancel_method_pending(session, active)?;
        let before = active.selection.parts();
        active.selection.supersede();
        active.superseded_selections.push(active.selection.clone());
        active.selection = next;
        // A part kept across the change keeps what it produced, judged again
        // under the new revision; a part added or dropped starts over.
        let kept =
            |part: MethodId| before.contains(&part) && active.selection.parts().contains(&part);
        if kept(MethodId::SourceGroundedReport) {
            active.report.carry_sources(revision - 1);
        } else {
            active.report.drop_carried();
        }
        if !kept(MethodId::ReproducibleCalculation) {
            active.calculation.since = revision;
        }
        if !kept(MethodId::ImageGenerationReview) {
            active.image.since = revision;
        }
        if let Some(from) = start {
            image::quiet_request_history(session, active, from, self.tools);
        }
        active.state = RequestState::AwaitingContinuation;
        let next = if active.selection.availability.blocked() {
            next_steps(active.selection.method)
        } else {
            "Send a message to continue."
        };
        let cancelled = if cancelled.is_empty() {
            "Nothing was pending to cancel".to_owned()
        } else {
            format!(
                "Cancelled pending {} (method_changed)",
                cancelled.join(", ")
            )
        };
        Ok(format!("Method changed. {cancelled}; previous validation superseded_not_inherited. Observed external effects are not reverted. Nothing was rerun. {next}\n{}", request_text(active)))
    }

    pub(super) fn prepare_method_request(
        &mut self,
        message: &InboundMessage,
        session: &mut Session,
    ) -> Result<Option<AgentLoopTurnResult>, AgentLoopError> {
        if !user_turn(message)
            || crate::tools::ask_action::is_command(&message.content)
            || self
                .plugin_command_dispatcher
                .as_ref()
                .is_some_and(|dispatcher| dispatcher.router().dispatch(&message.content).is_some())
        {
            return Ok(None);
        }
        let mut store = MethodStore::load(session)?;
        store.start(&message.content, self.tools)?;
        let content = store.active.as_mut().and_then(|request| {
            if request.selection.availability.blocked() {
                request.selection.gate = Gate::Blocked;
                request.state = RequestState::Blocked;
                Some(blocked(&request.selection))
            } else {
                None
            }
        });
        store.save(session)?;
        if let Some(content) = content {
            append_user_turn(session, message);
            let turn_id = turn_id_for_message(message, session);
            self.append_durable_fact(&session.key, Some(turn_id.clone()), SESSION_TURN_ACCEPTED,
                json!({"channel":message.channel,"content_hash":format!("sha256:{:x}", Sha256::digest(message.content.as_bytes())),"media_count":message.media.len()}),
                Some(durable_event_provenance(&self.context_builder, &RuntimeExecutionLedger::default())))?;
            session.add_message("assistant", content.clone(), Map::new());
            self.append_durable_fact(&session.key, Some(turn_id), SESSION_TURN_COMPLETED,
                json!({"stop_reason":"method_blocked","tool_count":0,"outcome_count":0,"pending_effect_count":0}),
                Some(durable_event_provenance(&self.context_builder, &RuntimeExecutionLedger::default())))?;
            self.sessions.save(session)?;
            return self
                .publish_command_response(
                    message,
                    session.clone(),
                    &content,
                    None,
                    "method_blocked",
                    false,
                )
                .map(Some);
        }
        Ok(None)
    }
}
