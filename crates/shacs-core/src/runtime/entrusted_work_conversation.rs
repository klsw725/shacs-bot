use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_session::Session;

use crate::tools::{proposal_draft_from_tool_content, EntrustedWorkProposalDraft};

use super::InboundMessage;

pub(super) const PENDING_ENTRUSTED_WORK_PROPOSAL_KEY: &str = "pending_entrusted_work_proposal";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingEntrustedWorkProposal {
    pub proposal_id: String,
    pub draft: EntrustedWorkProposalDraft,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub sender_id: String,
    pub evidence_ref: String,
    pub proposed_at: String,
}

impl PendingEntrustedWorkProposal {
    pub fn matches_request(&self, message: &InboundMessage, session_key: &str) -> bool {
        self.session_key == session_key
            && self.channel == message.channel
            && self.chat_id == message.chat_id
            && self.sender_id == message.sender_id
    }

    pub fn render_prompt(&self) -> String {
        format!(
            "Entrusted work proposal `{}`: {}\nAccept exactly with `/work accept {}` or decline with `/work decline {}`.",
            self.proposal_id, self.draft.goal_text, self.proposal_id, self.proposal_id
        )
    }
}

pub(super) fn proposal_from_runner_messages(
    returned: &[Value],
    message: &InboundMessage,
    session_key: &str,
    proposed_at: String,
) -> Option<PendingEntrustedWorkProposal> {
    returned.iter().rev().find_map(|tool_message| {
        if tool_message.get("role").and_then(Value::as_str) != Some("tool")
            || tool_message.get("name").and_then(Value::as_str)
                != Some(crate::tools::PROPOSE_ENTRUSTED_WORK_TOOL_NAME)
        {
            return None;
        }
        let tool_call_id = tool_message.get("tool_call_id").and_then(Value::as_str)?;
        let content = tool_message.get("content").and_then(Value::as_str)?;
        let draft = proposal_draft_from_tool_content(content)?;
        let proposal_id = proposal_id(session_key, tool_call_id, &draft.goal_text);
        Some(PendingEntrustedWorkProposal {
            proposal_id,
            draft,
            session_key: session_key.to_owned(),
            channel: message.channel.clone(),
            chat_id: message.chat_id.clone(),
            sender_id: message.sender_id.clone(),
            evidence_ref: format!("tool_call:{tool_call_id}"),
            proposed_at: proposed_at.clone(),
        })
    })
}

pub(super) fn pending_proposal(session: &Session) -> Option<PendingEntrustedWorkProposal> {
    session
        .metadata
        .get(PENDING_ENTRUSTED_WORK_PROPOSAL_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(super) fn store_pending_proposal(
    session: &mut Session,
    proposal: &PendingEntrustedWorkProposal,
) {
    if let Ok(value) = serde_json::to_value(proposal) {
        session
            .metadata
            .insert(PENDING_ENTRUSTED_WORK_PROPOSAL_KEY.to_owned(), value);
    }
}

pub(super) fn clear_pending_proposal(session: &mut Session) {
    session.metadata.remove(PENDING_ENTRUSTED_WORK_PROPOSAL_KEY);
}

fn proposal_id(session_key: &str, tool_call_id: &str, goal_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_key.as_bytes());
    hasher.update([0]);
    hasher.update(tool_call_id.as_bytes());
    hasher.update([0]);
    hasher.update(goal_text.as_bytes());
    let digest = hasher.finalize();
    format!(
        "proposal-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
    )
}
