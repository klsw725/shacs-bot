use super::{AgentLoop, AgentLoopError, AgentLoopTurnResult, InboundMessage};
use crate::runtime::entrusted_work_surface::load_owner_session;
use crate::runtime::web_watch_notify_dispatch::current;
use crate::runtime::web_watch_notify_render::render_web_notification;
use crate::runtime::web_watch_notify_store::{
    append_web_notification_fact, read_web_notifications,
};
use crate::runtime::{
    WebNotifyContent, WebNotifyEnqueue, WebNotifyKind, WebNotifyStage as Stage,
    ENTRUSTED_NOTIFICATION_CORRELATION,
};
use shacs_session::durable_event::{SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED};
use shacs_session::SessionMutationGuard;

impl AgentLoop<'_> {
    /// Sends one enqueued 042 notification to its `email:` target session without a provider.
    /// The owner session is re-checked first; a recovery, or the recovery line of a change, needs
    /// its failure mail to have gone out. After the consumption claim a crash leaves the delivery
    /// `unknown` instead of sending it again.
    pub(super) fn process_web_watch_notification(
        &mut self,
        message: &InboundMessage,
        session_key: &str,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let mut result = AgentLoopTurnResult {
            session_key: session_key.to_owned(),
            final_content: None,
            stop_reason: "web_watch_notification_suppressed".to_owned(),
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
        let Some(owner) = read_web_notifications(&root)?
            .get(correlation)
            .map(|entry| entry.intent.owner_session_id.clone())
        else {
            return Ok(result);
        };
        let _guard = SessionMutationGuard::acquire(&self.config.workspace, &owner)?;
        let entries = read_web_notifications(&root)?;
        let Some(entry) = entries.get(correlation) else {
            return Ok(result);
        };
        let intent = &entry.intent;
        if intent.target != session_key
            || message.session_key() != session_key
            || format!("{}:{}", message.channel, message.chat_id) != session_key
            || message
                .metadata
                .get("durable_work_id")
                .and_then(serde_json::Value::as_str)
                != Some(correlation)
            || entry.snapshot.enqueue != WebNotifyEnqueue::Enqueued
            || entry.snapshot.consumption_started
            || entry.snapshot.suppressed_stale
        {
            return Ok(result);
        }
        let failure_went_out = |interval_id: &str| {
            entries.values().any(|failure| {
                failure.intent.kind() == WebNotifyKind::Failure
                    && failure.intent.event_id == interval_id
                    && failure.snapshot.consumption_started
                    && !failure.snapshot.suppressed_stale
            })
        };
        let still_current = match load_owner_session(&self.config.workspace, &owner) {
            Ok((_, session)) => {
                session.key == owner && current(&session, intent, self.config.now_ms())?
            }
            Err(_) => false,
        } && (intent.kind() != WebNotifyKind::Recovery
            || failure_went_out(&intent.event_id));
        if !still_current {
            append_web_notification_fact(&root, intent, Stage::SuppressedStale)?;
            return Ok(result);
        }
        let mut sent = intent.clone();
        if let WebNotifyContent::Change { recovered, .. } = &mut sent.content {
            if recovered
                .as_ref()
                .is_some_and(|interval| !failure_went_out(&interval.interval_id))
            {
                *recovered = None;
            }
        }
        let content = format!(
            "{}\nTracking: {correlation}",
            render_web_notification(&sent)
        );
        append_web_notification_fact(&root, intent, Stage::Consuming)?;
        let turn_id = format!("turn:{correlation}");
        self.append_durable_fact(
            session_key,
            Some(turn_id.clone()),
            SESSION_TURN_ACCEPTED,
            serde_json::json!({"correlation_id": correlation, "event_id": intent.event_id, "synthetic_notification": true}),
            None,
        )?;
        self.append_durable_fact(
            session_key,
            Some(turn_id),
            SESSION_TURN_COMPLETED,
            serde_json::json!({"correlation_id": correlation, "stop_reason": "web_watch_notification", "tool_count": 0}),
            None,
        )?;
        append_web_notification_fact(&root, intent, Stage::OutboundPending)?;
        let mut outbound = super::outbound_for(
            message,
            session_key,
            content.clone(),
            Vec::new(),
            "web_watch_notification",
        );
        outbound.metadata.insert(
            ENTRUSTED_NOTIFICATION_CORRELATION.to_owned(),
            serde_json::json!(correlation),
        );
        if self.bus.try_publish_outbound(outbound).is_err() {
            append_web_notification_fact(&root, intent, Stage::DeliveryFailed)?;
            result.stop_reason = "web_watch_notification_delivery_failed".to_owned();
            return Ok(result);
        }
        result.stop_reason = "web_watch_notification".to_owned();
        result.final_content = Some(content);
        result.outbound_count = 1;
        Ok(result)
    }
}
