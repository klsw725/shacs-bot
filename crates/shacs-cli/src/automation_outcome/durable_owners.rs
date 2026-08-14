use super::owner_support::{
    delivery_target, evidence_ref, observed_at, open_dispatcher, route_evidence, route_item,
    route_work_id, ROUTE_PAYLOAD_TYPE, ROUTE_WORK_KIND,
};
use shacs_channels::InboundMessage;
use shacs_core::runtime::{
    apply_completion_verdict, inline_control_payload, persistent_goal_from_session,
    store_persistent_goal, AutomationRouteEvidence, AutomationRouteOwners,
    AutomationTaskOutcomeInput, DurableWorkDispatcher, DurableWorkEnqueueInput,
    GoalCompletionVerdict,
};
use shacs_eval::completion_boundary::EvaluatorRoute;
use shacs_session::durable_work::WorkTerminalKind;
use shacs_session::{SessionManager, SessionMutationGuard};
use std::path::{Path, PathBuf};

pub(super) struct DurableRouteOwners {
    data_dir: PathBuf,
    workspace: PathBuf,
}

impl DurableRouteOwners {
    pub(super) fn new(data_dir: &Path, workspace: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
        }
    }

    fn enqueue_visible(
        &self,
        route: EvaluatorRoute,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        let target = input
            .target_surface
            .as_ref()
            .ok_or_else(|| "automation route target surface is unavailable".to_owned())?;
        let (channel, chat_id) = delivery_target(target, &input.session_key)?;
        let evidence = evidence_ref(route, input);
        let work_id = route_work_id(route, input);
        let mut metadata = serde_json::Map::new();
        metadata.insert("automation_route".to_owned(), serde_json::json!(route));
        metadata.insert("correlation_id".to_owned(), serde_json::json!(evidence));
        metadata.insert("result_ref".to_owned(), serde_json::json!(input.result_ref));
        let content = match route {
            EvaluatorRoute::Notify => "Report the completed automation result to the user.",
            EvaluatorRoute::Escalate => "Escalate the automation result to the user for action.",
            EvaluatorRoute::Continue => "Continue the current persistent goal once.",
            EvaluatorRoute::Verify => {
                "Verify the correlated automation result and report evidence."
            }
            EvaluatorRoute::Suppress | EvaluatorRoute::RollbackCandidate => {
                return Err("unsupported visible automation route".to_owned())
            }
        };
        let message = InboundMessage::new(channel, "automation", chat_id, content)
            .with_metadata(metadata)
            .with_session_key_override(&input.session_key);
        self.with_session_lock(input, |dispatcher| {
            if route_item(&self.data_dir, &work_id)?.is_none() {
                dispatcher
                    .enqueue_inbound(work_id.clone(), &message, Some(evidence.clone()), None)
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })?;
        Ok(route_evidence(route, evidence))
    }

    fn record_terminal(
        &self,
        route: EvaluatorRoute,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        let evidence = evidence_ref(route, input);
        let work_id = route_work_id(route, input);
        self.with_session_lock(input, |dispatcher| {
            if route_item(&self.data_dir, &work_id)?.is_none() {
                dispatcher
                    .enqueue_work(DurableWorkEnqueueInput {
                        work_id: work_id.clone(),
                        work_kind: ROUTE_WORK_KIND.to_owned(),
                        session_key: input.session_key.clone(),
                        turn_id: None,
                        effect_id: Some(evidence.clone()),
                        payload_ref: inline_control_payload(
                            ROUTE_PAYLOAD_TYPE,
                            serde_json::json!({
                                "route": route,
                                "result_ref": input.result_ref,
                                "correlation_id": evidence,
                            }),
                        )
                        .map_err(|error| error.to_string())?,
                        dedupe_hint: Some(evidence.clone()),
                        next_wake_at_ms: None,
                    })
                    .map_err(|error| error.to_string())?;
            }
            let item = route_item(&self.data_dir, &work_id)?
                .ok_or_else(|| "automation owner request is missing".to_owned())?;
            if !item.state.is_terminal() {
                dispatcher
                    .lease_work(&item, 0)
                    .map_err(|error| error.to_string())?;
                let leased = route_item(&self.data_dir, &work_id)?
                    .ok_or_else(|| "leased automation owner request is missing".to_owned())?;
                dispatcher
                    .record_terminal(&leased, WorkTerminalKind::Succeeded, "no_notification")
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })?;
        Ok(route_evidence(route, evidence))
    }

    fn enqueue_verification(
        &self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        let target = input
            .target_surface
            .as_ref()
            .ok_or_else(|| "automation route target surface is unavailable".to_owned())?;
        delivery_target(target, &input.session_key)?;
        let route = EvaluatorRoute::Verify;
        let evidence = evidence_ref(route, input);
        let work_id = route_work_id(route, input);
        self.with_session_lock(input, |dispatcher| {
            if route_item(&self.data_dir, &work_id)?.is_none() {
                dispatcher
                    .enqueue_work(DurableWorkEnqueueInput {
                        work_id,
                        work_kind: ROUTE_WORK_KIND.to_owned(),
                        session_key: input.session_key.clone(),
                        turn_id: None,
                        effect_id: Some(evidence.clone()),
                        payload_ref: inline_control_payload(
                            ROUTE_PAYLOAD_TYPE,
                            serde_json::json!({
                                "route": route,
                                "result_ref": input.result_ref,
                                "correlation_id": evidence,
                            }),
                        )
                        .map_err(|error| error.to_string())?,
                        dedupe_hint: Some(evidence.clone()),
                        next_wake_at_ms: None,
                    })
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })?;
        Ok(route_evidence(route, evidence))
    }

    fn continue_once(
        &self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        let evidence = evidence_ref(EvaluatorRoute::Continue, input);
        let work_id = route_work_id(EvaluatorRoute::Continue, input);
        let _guard = SessionMutationGuard::acquire(&self.workspace, &input.session_key)
            .map_err(|error| error.to_string())?;
        if route_item(&self.data_dir, &work_id)?.is_some() {
            return Ok(route_evidence(EvaluatorRoute::Continue, evidence));
        }
        let mut manager = SessionManager::open_existing(&self.workspace)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "automation continuation session is missing".to_owned())?;
        let mut session = manager
            .load_existing(&input.session_key)
            .ok_or_else(|| "automation continuation session is missing".to_owned())?;
        let goal = persistent_goal_from_session(&session)
            .ok_or_else(|| "automation continuation goal is missing".to_owned())?;
        if goal.id != input.goal_id.as_deref().unwrap_or_default()
            || goal
                .last_transition
                .as_ref()
                .is_some_and(|fact| fact.user_interrupted)
            || goal.turns_used >= goal.turn_budget
        {
            return Err("automation continuation facts changed before enqueue".to_owned());
        }
        let next =
            apply_completion_verdict(&goal, GoalCompletionVerdict::Continue, None, observed_at())
                .map_err(|error| error.to_string())?;
        store_persistent_goal(&mut session, &next).map_err(|error| error.to_string())?;
        let target = input
            .target_surface
            .as_ref()
            .ok_or_else(|| "automation route target surface is unavailable".to_owned())?;
        let (channel, chat_id) = delivery_target(target, &input.session_key)?;
        let mut metadata = serde_json::Map::new();
        metadata.insert("automation_route".to_owned(), serde_json::json!("continue"));
        metadata.insert("correlation_id".to_owned(), serde_json::json!(evidence));
        metadata.insert("goal_id".to_owned(), serde_json::json!(goal.id));
        let message = InboundMessage::new(
            channel,
            "automation",
            chat_id,
            "Continue the current persistent goal once.",
        )
        .with_metadata(metadata)
        .with_session_key_override(&input.session_key);
        open_dispatcher(&self.data_dir)?
            .enqueue_inbound(work_id, &message, Some(evidence.clone()), None)
            .map_err(|error| error.to_string())?;
        manager.save(&session).map_err(|error| error.to_string())?;
        Ok(route_evidence(EvaluatorRoute::Continue, evidence))
    }

    fn with_session_lock<T>(
        &self,
        input: &AutomationTaskOutcomeInput,
        action: impl FnOnce(&mut DurableWorkDispatcher) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = SessionMutationGuard::acquire(&self.workspace, &input.session_key)
            .map_err(|error| error.to_string())?;
        action(&mut open_dispatcher(&self.data_dir)?)
    }
}

impl AutomationRouteOwners for DurableRouteOwners {
    fn notify(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.enqueue_visible(EvaluatorRoute::Notify, input)
    }
    fn suppress(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record_terminal(EvaluatorRoute::Suppress, input)
    }
    fn continue_task(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.continue_once(input)
    }
    fn escalate(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.enqueue_visible(EvaluatorRoute::Escalate, input)
    }
    fn verify(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.enqueue_verification(input)
    }
    fn rollback_candidate(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        super::improvement_owner::record_candidate(&self.workspace, input)
    }
}
