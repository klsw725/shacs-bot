use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use shacs_security::NetworkGuard;
use shacs_session::Session;

use super::web_watch_fetch::{display_url, WebFetchLimits};
use super::web_watch_redact::{safe_text, MAX_ANCHOR_CHARS};
use super::{InboundMessage, WebCondition, WebDeadlineInput, WebObservedValue};

/// How long a shown summary waits for its confirmation.
pub const WEB_WATCH_SUMMARY_TTL_MS: u64 = 30 * 60 * 1_000;
pub(super) const PENDING_WEB_WATCH_SUMMARY_KEY: &str = "pending_web_watch_summary";
/// Plain replies that confirm the latest summary, after `normalized_reply`.
pub const WEB_WATCH_CONFIRM_REPLIES: [&str; 11] = [
    "응",
    "네",
    "예",
    "맞아",
    "맞아요",
    "그렇게 해",
    "그렇게 해줘",
    "좋아",
    "yes",
    "ok",
    "okay",
];
/// Plain replies that drop the pending summary.
pub const WEB_WATCH_CANCEL_REPLIES: [&str; 6] =
    ["아니", "아니요", "아니오", "취소", "no", "cancel"];

/// Replies other than a shown summary sent to each conversation by this process. A summary keeps
/// the count it was shown at and a plain reply confirms it only while the count is unchanged, so
/// any later reply counts even when it is not saved in the session (a command answered during
/// another turn, a notification). Like the summary, the count does not survive a restart.
static OTHER_REPLIES: Mutex<BTreeMap<(PathBuf, String), u64>> = Mutex::new(BTreeMap::new());

fn reply_key(workspace: &Path, session_key: &str) -> (PathBuf, String) {
    (workspace.to_path_buf(), session_key.to_owned())
}

pub(super) fn note_other_reply(workspace: &Path, session_key: &str) {
    if let Ok(mut replies) = OTHER_REPLIES.lock() {
        *replies
            .entry(reply_key(workspace, session_key))
            .or_default() += 1;
    }
}

/// `u64::MAX` when the count cannot be read, which no summary holds.
pub(super) fn other_replies(workspace: &Path, session_key: &str) -> u64 {
    OTHER_REPLIES.lock().map_or(u64::MAX, |replies| {
        replies
            .get(&reply_key(workspace, session_key))
            .copied()
            .unwrap_or(0)
    })
}

/// What the agent loop needs to read pages and check targets for conversational web watches.
#[derive(Debug, Clone)]
pub struct WebWatchConversationConfig {
    pub guard: NetworkGuard,
    pub limits: WebFetchLimits,
    pub data_dir: PathBuf,
    /// Whether the configured Email channel can send (enabled, consented, SMTP settings).
    pub email_ready: bool,
}

/// A summary shown to one sender, waiting for that sender's confirmation. It is bound to the
/// process that showed it, so a restart closes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingWebWatchSummary {
    pub summary_id: String,
    pub revision: u32,
    pub epoch: String,
    pub session_key: String,
    pub source_session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub sender_id: String,
    pub shown_at_ms: u64,
    pub expires_at_ms: u64,
    /// Index in the session messages of the assistant message that shows this revision.
    pub message_index: usize,
    /// `other_replies` of the conversation when this revision was shown.
    pub other_replies: u64,
    pub evidence_ref: String,
    pub setup: WebWatchSetup,
    pub text: String,
}

/// Everything the confirmation sets up, already checked when the summary was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebWatchSetup {
    pub goal_text: String,
    pub url: String,
    /// What is watched, as shown (safe text).
    pub value_text: String,
    /// The phrase of a phrase condition, as shown (safe text).
    pub phrase: Option<String>,
    pub condition: WebCondition,
    pub cadence_seconds: u64,
    pub notify_target: Option<String>,
    pub failure_threshold_seconds: Option<u64>,
    pub deadline: Option<WebDeadlineInput>,
    /// The page read the summary showed; the confirmation reads the page again and needs the
    /// same value. `None` when a change keeps the observation as it is.
    pub read: Option<SummaryRead>,
    /// For a change that keeps the observation: the watched value as the page shows it now,
    /// read with the stored criterion. It is only shown; the confirmation does not read the
    /// page again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<PageNow>,
    pub change: Option<WebWatchChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SummaryRead {
    pub candidate_token: String,
    pub shown: WebObservedValue,
    pub shown_text: String,
}

/// The watched value a change summary shows while the change keeps the observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(super) enum PageNow {
    Read {
        shown: WebObservedValue,
        shown_text: String,
    },
    Unreadable {
        reason: String,
    },
}

/// A change of an existing watch: the revisions it was made against and what it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebWatchChange {
    pub work_id: String,
    pub condition_revision: u64,
    pub notify_revision: u64,
    pub deadline_revision: u64,
    pub observation: bool,
    pub notify: bool,
    pub deadline: bool,
}

impl PendingWebWatchSummary {
    pub fn matches_sender(&self, message: &InboundMessage, session: &Session) -> bool {
        self.session_key == session.key
            && self.source_session_key == message.session_key()
            && self.channel == message.channel
            && self.chat_id == message.chat_id
            && self.sender_id == message.sender_id
    }

    /// Why the summary can no longer be confirmed, if it cannot.
    pub fn closed_reason(&self, epoch: &str, now_ms: u64) -> Option<&'static str> {
        if self.epoch != epoch {
            Some("the assistant restarted after it was shown")
        } else if now_ms >= self.expires_at_ms {
            Some("it waited 30 minutes without a confirmation")
        } else {
            None
        }
    }

    pub fn is_latest_response(&self, session: &Session, workspace: &Path) -> bool {
        let replies = other_replies(workspace, &self.session_key);
        last_assistant_index(session) == Some(self.message_index)
            && replies != u64::MAX
            && replies == self.other_replies
    }

    /// Records that this revision is shown now as the message at `message_index`.
    pub fn shown_at(&mut self, message_index: usize, workspace: &Path) {
        self.message_index = message_index;
        self.other_replies = other_replies(workspace, &self.session_key);
    }

    pub fn buttons(&self) -> Vec<String> {
        vec![
            format!("/watch confirm {} {}", self.summary_id, self.revision),
            format!("/watch cancel {}", self.summary_id),
        ]
    }
}

/// How a message answers a pending summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WebWatchReply {
    /// `/watch confirm <id> <revision>` (a button) or, without them, a plain confirming reply.
    Confirm(Option<(String, u32)>),
    Cancel(Option<String>),
    /// A malformed `/watch` command.
    Invalid,
    Other,
}

pub(super) fn classify_reply(content: &str) -> WebWatchReply {
    let words = content.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        ["/watch", "confirm", id, revision] => {
            revision.parse().map_or(WebWatchReply::Invalid, |revision| {
                WebWatchReply::Confirm(Some(((*id).to_owned(), revision)))
            })
        }
        ["/watch", "cancel", id] => WebWatchReply::Cancel(Some((*id).to_owned())),
        ["/watch", ..] => WebWatchReply::Invalid,
        _ => {
            let reply = normalized_reply(content);
            if WEB_WATCH_CONFIRM_REPLIES.contains(&reply.as_str()) {
                WebWatchReply::Confirm(None)
            } else if WEB_WATCH_CANCEL_REPLIES.contains(&reply.as_str()) {
                WebWatchReply::Cancel(None)
            } else {
                WebWatchReply::Other
            }
        }
    }
}

/// Trimmed, ASCII lowercase, inner whitespace as single spaces, trailing `.`, `!`, `~` dropped.
pub(super) fn normalized_reply(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '!', '~'])
        .trim_end()
        .to_ascii_lowercase()
}

pub(super) fn pending_summary(session: &Session) -> Option<PendingWebWatchSummary> {
    session
        .metadata
        .get(PENDING_WEB_WATCH_SUMMARY_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(super) fn store_pending_summary(
    session: &mut Session,
    summary: &PendingWebWatchSummary,
) -> std::io::Result<()> {
    let value = serde_json::to_value(summary).map_err(std::io::Error::other)?;
    session
        .metadata
        .insert(PENDING_WEB_WATCH_SUMMARY_KEY.to_owned(), value);
    Ok(())
}

pub(super) fn clear_pending_summary(session: &mut Session) {
    session.metadata.remove(PENDING_WEB_WATCH_SUMMARY_KEY);
}

/// Something else was answered after the summary, so a plain reply can no longer confirm it;
/// its button still can.
pub(super) fn demote_pending_summary(session: &mut Session) {
    let Some(mut summary) = pending_summary(session) else {
        return;
    };
    summary.message_index = usize::MAX;
    if store_pending_summary(session, &summary).is_err() {
        clear_pending_summary(session);
    }
}

pub(super) fn last_assistant_index(session: &Session) -> Option<usize> {
    session
        .messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
}

/// The pending summary as shown outside the session file: no candidate token, the page address
/// without query and fragment, user and page text made safe.
pub(super) fn mask_pending_summary(value: &mut Value) {
    let Some(summary) = value.as_object_mut() else {
        return;
    };
    let Some(setup) = summary.get_mut("setup").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(url) = setup.get("url").and_then(Value::as_str).map(display_url) {
        setup.insert("url".to_owned(), Value::String(url));
    }
    if let Some(read) = setup.get_mut("read").and_then(Value::as_object_mut) {
        read.remove("candidate_token");
        if let Some(text) = read.get("shown_text").and_then(Value::as_str) {
            let shown = safe_text(text, MAX_ANCHOR_CHARS);
            read.insert("shown_text".to_owned(), Value::String(shown));
        }
    }
    if let Some(now) = setup.get_mut("now").and_then(Value::as_object_mut) {
        for key in ["shown_text", "reason"] {
            if let Some(text) = now.get(key).and_then(Value::as_str) {
                let shown = safe_text(text, MAX_ANCHOR_CHARS);
                now.insert(key.to_owned(), Value::String(shown));
            }
        }
    }
    if let Some(goal) = setup.get("goal_text").and_then(Value::as_str) {
        let goal = safe_text(goal, MAX_ANCHOR_CHARS);
        setup.insert("goal_text".to_owned(), Value::String(goal));
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_reply, note_other_reply, other_replies, WebWatchReply};

    #[cfg(unix)]
    #[test]
    fn reply_counts_of_workspaces_shown_alike_stay_apart() {
        use std::os::unix::ffi::OsStrExt;
        let first = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/qa/ws-\xff"));
        let second = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/qa/ws-\xfe"));
        assert_eq!(first.display().to_string(), second.display().to_string());

        note_other_reply(first, "chat:room");

        assert_eq!(other_replies(first, "chat:room"), 1);
        assert_eq!(other_replies(second, "chat:room"), 0);
    }

    #[test]
    fn only_listed_replies_and_well_formed_commands_answer_a_summary() {
        for (reply, expected) in [
            ("응", WebWatchReply::Confirm(None)),
            ("  그렇게   해 !", WebWatchReply::Confirm(None)),
            ("OK.", WebWatchReply::Confirm(None)),
            ("취소", WebWatchReply::Cancel(None)),
            (
                "/watch confirm w-1 2",
                WebWatchReply::Confirm(Some(("w-1".to_owned(), 2))),
            ),
            (
                "/watch cancel w-1",
                WebWatchReply::Cancel(Some("w-1".to_owned())),
            ),
            ("/watch confirm w-1 two", WebWatchReply::Invalid),
            ("응 근데 주기는 1시간으로", WebWatchReply::Other),
            ("음...", WebWatchReply::Other),
            ("yes please", WebWatchReply::Other),
        ] {
            assert_eq!(classify_reply(reply), expected, "{reply}");
        }
    }
}
