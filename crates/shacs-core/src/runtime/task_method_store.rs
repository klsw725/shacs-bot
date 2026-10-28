use super::task_method::{Gate, MethodId, MethodRequest, Reason, RequestState, SelectionSource};
use super::task_method_classify::{classify, intents, selection};
use super::task_method_image::ImageEvidence;
use super::task_method_report::ReportEvidence;
use crate::runtime::Session;
use crate::tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_command::MethodName;
use std::io;

pub const KEY: &str = "task_method_requests_v1";
/// Session metadata with an open request's tool outputs as written (see
/// `agent_loop_method_hold`); they go as soon as that request is not active.
pub(crate) const HELD_RESULTS_KEY: &str = "task_method_held_results_v1";

pub(super) fn preview(text: &str) -> String {
    let window: String = text
        .chars()
        .take(512 + super::task_method_calc::REDACTION_MARGIN)
        .collect();
    shacs_redaction::redact_string(&window)
        .chars()
        .take(512)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MethodStore {
    pub schema_version: u32,
    pub next_request: u64,
    pub pending_override: Option<MethodName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_revision: Option<String>,
    pub active: Option<MethodRequest>,
    pub closed: Vec<MethodRequest>,
}

impl Default for MethodStore {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_request: 1,
            pending_override: None,
            pending_revision: None,
            active: None,
            closed: Vec::new(),
        }
    }
}

impl MethodStore {
    pub fn load(session: &Session) -> io::Result<Self> {
        let store = session
            .metadata
            .get(KEY)
            .cloned()
            .map(serde_json::from_value::<Self>)
            .transpose()
            .map_err(io::Error::other)?
            .unwrap_or_default();
        if store.schema_version != 1 || store.closed.len() > 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid task method metadata",
            ));
        }
        Ok(store)
    }

    pub fn save(&self, session: &mut Session) -> io::Result<()> {
        let mut bounded = self.clone();
        for request in bounded.closed.iter_mut().chain(bounded.active.iter_mut()) {
            bound_request(request);
        }
        session.metadata.insert(
            KEY.to_owned(),
            serde_json::to_value(bounded).map_err(io::Error::other)?,
        );
        let held = session
            .metadata
            .get(HELD_RESULTS_KEY)
            .and_then(|held| held.get("request_id"))
            .and_then(serde_json::Value::as_str);
        let active = self
            .active
            .as_ref()
            .map(|request| request.request_id.as_str());
        if held.is_some() && held != active {
            session.metadata.remove(HELD_RESULTS_KEY);
        }
        Ok(())
    }

    pub fn start(&mut self, goal: &str, tools: &ToolRegistry) -> io::Result<()> {
        if let Some(active) = &self.active {
            match active.state {
                RequestState::InProgress | RequestState::AwaitingContinuation => return Ok(()),
                RequestState::Blocked => self.close(RequestState::Abandoned),
                RequestState::Answered
                | RequestState::Failed
                | RequestState::Cancelled
                | RequestState::Abandoned => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "closed task method request is active",
                    ))
                }
            }
        }
        let next = self
            .next_request
            .checked_add(1)
            .ok_or_else(|| io::Error::other("task method request sequence exhausted"))?;
        let revision_of = self
            .pending_revision
            .take()
            .and_then(|id| {
                self.closed
                    .iter()
                    .rev()
                    .find(|request| request.request_id == id)
            })
            .map(|request| {
                let (id, goal) = (&request.request_id, &request.goal_preview);
                if request.selection.method == MethodId::ImageGenerationReview {
                    Revision::Image(ImageEvidence::next_version(&request.image, id, goal))
                } else {
                    Revision::Report(ReportEvidence::next_version(&request.report, id, goal))
                }
            });
        let intent = match (&revision_of, self.pending_override.take()) {
            (Some(Revision::Report(_)), _) => (
                MethodId::SourceGroundedReport,
                SelectionSource::Explicit,
                Reason::ReportRevision,
            ),
            (Some(Revision::Image(_)), _) => (
                MethodId::ImageGenerationReview,
                SelectionSource::Explicit,
                Reason::ImageRevision,
            ),
            (None, Some(method)) => (
                method.into(),
                SelectionSource::OneShot,
                Reason::OneShotOverride,
            ),
            (None, None) => {
                let (method, reason) = classify(goal);
                (method, SelectionSource::Automatic, reason)
            }
        };
        let (automatic_method, automatic_reason) = classify(goal);
        let parts = intents(goal);
        let mut selected = selection(intent, &parts, 1, tools);
        let state = if selected.availability.blocked() {
            selected.gate = Gate::Blocked;
            RequestState::Blocked
        } else {
            RequestState::InProgress
        };
        self.active = Some(MethodRequest {
            request_id: format!("method-request-{}", self.next_request),
            goal_preview: preview(goal),
            goal_digest: format!("sha256:{:x}", Sha256::digest(goal.as_bytes())),
            state,
            selection: selected,
            automatic_method,
            automatic_reason,
            automatic_components: if automatic_method == MethodId::Composed {
                parts
            } else {
                Vec::new()
            },
            superseded_selections: Vec::new(),
            evidence_facts: Vec::new(),
            user_decisions: Vec::new(),
            remaining_questions: Vec::new(),
            external_effect_facts: Vec::new(),
            omitted_observation_count: 0,
            calculation: Default::default(),
            report: match &revision_of {
                Some(Revision::Report(report)) => report.clone(),
                _ => ReportEvidence::default(),
            },
            image: match revision_of {
                Some(Revision::Image(image)) => image,
                _ => ImageEvidence::default(),
            },
            user_acceptance: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
        });
        self.next_request = next;
        Ok(())
    }

    pub fn close(&mut self, state: RequestState) {
        if let Some(mut request) = self.active.take() {
            request.state = state;
            request.calculation.keep_verdict_runs();
            self.closed.push(request);
            if self.closed.len() > 32 {
                self.closed.remove(0);
            }
        }
    }
}

enum Revision {
    Report(ReportEvidence),
    Image(ImageEvidence),
}

fn bound_request(request: &mut MethodRequest) {
    request.goal_preview = preview(&request.goal_preview);
    for selection in request
        .superseded_selections
        .iter_mut()
        .chain(std::iter::once(&mut request.selection))
    {
        selection.validation_claims.truncate(32);
        for claim in &mut selection.validation_claims {
            claim.check = preview(&claim.check);
            claim.evidence_refs.truncate(32);
            for reference in &mut claim.evidence_refs {
                *reference = preview(reference);
            }
        }
    }
    let removed = request.external_effect_facts.len().saturating_sub(128);
    request.external_effect_facts.drain(..removed);
    request.omitted_observation_count = request.omitted_observation_count.saturating_add(removed);
    for fact in &mut request.external_effect_facts {
        fact.tool_name = preview(&fact.tool_name);
        fact.turn_id = preview(&fact.turn_id);
        fact.observed_at = preview(&fact.observed_at);
    }
    request.evidence_facts.truncate(128);
    request.user_decisions.truncate(128);
    request.remaining_questions.truncate(128);
    for decision in &mut request.user_decisions {
        decision.question = preview(&decision.question);
        decision.answer = preview(&decision.answer);
    }
    for question in &mut request.remaining_questions {
        *question = preview(question);
    }
    request.calculation.bound();
}
