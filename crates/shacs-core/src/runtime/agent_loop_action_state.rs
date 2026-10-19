use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::entrusted_work_access::{bound_current_work, latest_transition_id};
use crate::runtime::{EntrustedWorkConsentProof, InboundMessage, PersistentGoalStatus, Session};
use crate::tools::ask_action::ExternalActionDraft;

pub(super) const KEY: &str = "entrusted_action_decision";
static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Pending,
    Continuing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lineage {
    goal_id: String,
    goal_transition_id: String,
    condition_revision: u64,
    consent: EntrustedWorkConsentProof,
}

impl Lineage {
    fn current(session: &Session, work_id: &str) -> Option<Self> {
        let (works, index, goal) = bound_current_work(session, &session.key, work_id).ok()?;
        let work = works.get(index)?;
        if work.owner_session_id != session.key
            || goal.status != PersistentGoalStatus::Active
            || work.current_conditions().is_none()
            || work.consent.evidence_ref.trim().is_empty()
            || work.consent.recorded_at.trim().is_empty()
        {
            return None;
        }
        Some(Self {
            goal_id: goal.id.clone(),
            goal_transition_id: latest_transition_id(&goal).ok()?,
            condition_revision: work.current_revision,
            consent: work.consent.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Decision {
    pub proposal_id: String,
    pub tool_call_id: String,
    pub stage: Stage,
    pub draft: ExternalActionDraft,
    epoch: String,
    session_key: String,
    source_session_key: String,
    channel: String,
    chat_id: String,
    sender_id: String,
    lineage: Lineage,
}

impl Decision {
    pub fn new(
        session: &Session,
        message: &InboundMessage,
        proposal: (String, ExternalActionDraft, &str),
    ) -> Option<Self> {
        let (tool_call_id, draft, epoch) = proposal;
        let lineage = Lineage::current(session, &draft.work_id)?;
        Some(Self {
            proposal_id: fresh_id(),
            tool_call_id,
            draft,
            lineage,
            stage: Stage::Pending,
            epoch: epoch.to_owned(),
            session_key: session.key.clone(),
            source_session_key: message.session_key(),
            channel: message.channel.clone(),
            chat_id: message.chat_id.clone(),
            sender_id: message.sender_id.clone(),
        })
    }

    pub fn matches_request(&self, message: &InboundMessage, session: &Session) -> bool {
        self.session_key == session.key
            && self.source_session_key == message.session_key()
            && self.channel == message.channel
            && self.chat_id == message.chat_id
            && self.sender_id == message.sender_id
    }

    pub fn is_current(&self, session: &Session, epoch: &str) -> bool {
        self.epoch == epoch
            && Lineage::current(session, &self.draft.work_id).as_ref() == Some(&self.lineage)
    }

    pub fn save(&self, session: &mut Session) -> std::io::Result<()> {
        session.metadata.insert(
            KEY.to_owned(),
            serde_json::to_value(self).map_err(std::io::Error::other)?,
        );
        Ok(())
    }

    pub fn prompt(&self) -> String {
        let category = match self.draft.category {
            crate::tools::ask_action::ExternalActionCategory::Reserve => "reserve",
            crate::tools::ask_action::ExternalActionCategory::Purchase => "purchase",
            crate::tools::ask_action::ExternalActionCategory::Send => "send",
        };
        format!("External action proposal `{}` for work `{}` (revision {}, {}): {}\nReference: {}\nAccept only with `/action accept {}` or decline with `/action decline {}`. Acceptance resumes this proposal once; it is not tool permission and does not mean the action succeeded.",
            self.proposal_id, self.draft.work_id, self.lineage.condition_revision, category,
            self.draft.summary, self.draft.reference, self.proposal_id, self.proposal_id)
    }

    pub fn accepted_result(&self) -> String {
        json!({"kind":"entrusted_action_decision", "decision":"accepted", "proposal_id":self.proposal_id,
            "proposal":self.draft, "condition_revision":self.lineage.condition_revision,
            "goal_id":self.lineage.goal_id, "goal_transition_id":self.lineage.goal_transition_id,
            "permission_granted":false}).to_string()
    }
}

pub(crate) fn process_epoch() -> &'static str {
    static EPOCH: OnceLock<String> = OnceLock::new();
    EPOCH.get_or_init(fresh_id)
}

fn fresh_id() -> String {
    let identity = format!(
        "{}:{:?}:{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH),
        NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
    format!("action-{}", digest.chars().take(32).collect::<String>())
}
