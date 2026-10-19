use super::{AgentLoop, AgentLoopError, AgentLoopTurnResult, InboundMessage};
use crate::runtime::entrusted_notification_render::render;
use crate::runtime::entrusted_notification_store::{append_notification_fact, read_notifications};
use crate::runtime::entrusted_observation_surface::check_wake;
use crate::runtime::entrusted_work_surface::load_owner_session;
use crate::runtime::{
    EntrustedNotificationEnqueueState, EntrustedNotificationStage as Stage,
    ENTRUSTED_NOTIFICATION_CORRELATION,
};
use shacs_session::durable_event::{SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED};
use shacs_session::SessionMutationGuard;

impl AgentLoop<'_> {
    pub(super) fn process_entrusted_notification(
        &mut self,
        message: &InboundMessage,
        session_key: &str,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let mut result = AgentLoopTurnResult {
            session_key: session_key.to_owned(),
            final_content: None,
            stop_reason: "entrusted_notification_suppressed".to_owned(),
            tools_used: Vec::new(),
            outbound_count: 0,
            had_injections: false,
            command: None,
            ask_user_options: Vec::new(),
            message_tool_delivery_configured: false,
        };
        let Some(correlation) = message.owner_accepted_notification() else {
            return Ok(result);
        };
        let Some(root) = self.config.durable_event_root.clone() else {
            return Ok(result);
        };
        let _guard = SessionMutationGuard::acquire(&self.config.workspace, session_key)?;
        let entries = read_notifications(&root)?;
        let Some(entry) = entries.get(correlation) else {
            return Ok(result);
        };
        let intent = &entry.snapshot.intent;
        if intent.owner_session_id != session_key
            || message.session_key() != session_key
            || format!("{}:{}", message.channel, message.chat_id) != session_key
            || message
                .metadata
                .get("durable_work_id")
                .and_then(serde_json::Value::as_str)
                != Some(correlation)
            || entry.snapshot.enqueue != EntrustedNotificationEnqueueState::Enqueued
        {
            return Ok(result);
        }
        if entry.snapshot.consumption_started || entry.snapshot.suppressed_stale {
            return Ok(result);
        }
        if !load_owner_session(&self.config.workspace, session_key).is_ok_and(|(_, session)| {
            session.key == intent.owner_session_id
                && check_wake(&session, &entry.observation.wake).is_ok()
        }) {
            append_notification_fact(&root, intent, Stage::SuppressedStale)?;
            return Ok(result);
        }
        let decision = entry.observation.decision.as_ref().ok_or(
            crate::runtime::ObservationError::InvalidInput("notification decision missing"),
        )?;
        let content = format!(
            "{}\nTracking: {}",
            render(decision, intent.kind)?,
            correlation
        );
        append_notification_fact(&root, intent, Stage::Consuming)?;
        let turn_id = format!("turn:{correlation}");
        self.append_durable_fact(
            session_key, Some(turn_id.clone()), SESSION_TURN_ACCEPTED,
            serde_json::json!({"correlation_id": correlation, "decision_id": intent.decision_id, "synthetic_notification": true}), None,
        )?;
        self.append_durable_fact(
            session_key, Some(turn_id), SESSION_TURN_COMPLETED,
            serde_json::json!({"correlation_id": correlation, "stop_reason": "entrusted_notification", "tool_count": 0}), None,
        )?;
        append_notification_fact(&root, intent, Stage::OutboundPending)?;
        let mut outbound = super::outbound_for(
            message,
            session_key,
            content.clone(),
            Vec::new(),
            "entrusted_notification",
        );
        outbound.metadata.insert(
            ENTRUSTED_NOTIFICATION_CORRELATION.to_owned(),
            serde_json::json!(correlation),
        );
        if self.bus.try_publish_outbound(outbound).is_err() {
            append_notification_fact(&root, intent, Stage::DeliveryFailed)?;
            result.stop_reason = "entrusted_notification_delivery_failed".to_owned();
            return Ok(result);
        }
        result.stop_reason = "entrusted_notification".to_owned();
        result.final_content = Some(content);
        result.outbound_count = 1;
        Ok(result)
    }
}
