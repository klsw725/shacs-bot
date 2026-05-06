use crate::runtime::{Session, SessionManager};
use chrono::{DateTime, Local};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const RECENT_SUFFIX_MESSAGES: usize = 8;
const LAST_SUMMARY_KEY: &str = "_last_summary";

#[derive(Debug, Clone, PartialEq)]
pub struct AutoCompactArchiveOutcome {
    pub key: String,
    pub archived_messages: Vec<Value>,
    pub kept_messages: Vec<Value>,
    pub summary_stored: bool,
}

#[derive(Debug, Clone)]
pub struct AutoCompact {
    ttl_minutes: u64,
    archiving: BTreeSet<String>,
    summaries: BTreeMap<String, (String, String)>,
}

impl AutoCompact {
    pub fn new(session_ttl_minutes: u64) -> Self {
        Self {
            ttl_minutes: session_ttl_minutes,
            archiving: BTreeSet::new(),
            summaries: BTreeMap::new(),
        }
    }

    pub fn ttl_minutes(&self) -> u64 {
        self.ttl_minutes
    }

    pub fn is_archiving(&self, key: &str) -> bool {
        self.archiving.contains(key)
    }

    pub fn release_archiving(&mut self, key: &str) {
        self.archiving.remove(key);
    }

    pub fn is_expired(&self, timestamp: Option<&str>, now: DateTime<Local>) -> bool {
        if self.ttl_minutes == 0 {
            return false;
        }
        let Some(timestamp) = timestamp else {
            return false;
        };
        let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
            return false;
        };
        let age = now.signed_duration_since(parsed.with_timezone(&Local));
        age.num_seconds() >= (self.ttl_minutes * 60) as i64
    }

    pub fn split_unconsolidated(&self, session: &Session) -> (Vec<Value>, Vec<Value>) {
        let start = session.last_consolidated.min(session.messages.len());
        let tail = session.messages[start..].to_vec();
        if tail.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let mut probe = Session {
            key: session.key.clone(),
            messages: tail.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            metadata: Map::new(),
            last_consolidated: 0,
        };
        probe.retain_recent_legal_suffix(RECENT_SUFFIX_MESSAGES);
        let kept = probe.messages;
        let cut = tail.len().saturating_sub(kept.len());
        (tail[..cut].to_vec(), kept)
    }

    pub fn mark_expired_sessions(
        &mut self,
        sessions: &SessionManager,
        active_session_keys: impl IntoIterator<Item = String>,
    ) -> std::io::Result<Vec<String>> {
        let active = active_session_keys.into_iter().collect::<BTreeSet<_>>();
        let now = Local::now();
        let mut keys = Vec::new();
        for info in sessions.list_sessions()? {
            if info.key.is_empty()
                || self.archiving.contains(&info.key)
                || active.contains(&info.key)
            {
                continue;
            }
            if self.is_expired(info.updated_at.as_deref(), now) {
                self.archiving.insert(info.key.clone());
                keys.push(info.key);
            }
        }
        Ok(keys)
    }

    pub fn archive_session_with_summary(
        &mut self,
        sessions: &mut SessionManager,
        key: &str,
        summary: Option<&str>,
    ) -> std::io::Result<AutoCompactArchiveOutcome> {
        sessions.invalidate(key);
        let mut session = sessions.get_or_create(key);
        let last_active = session.updated_at.clone();
        let (archive_msgs, kept_msgs) = self.split_unconsolidated(&session);
        let summary_text = summary.unwrap_or_default().trim();
        let summary_stored = !summary_text.is_empty() && summary_text != "(nothing)";
        if summary_stored {
            self.summaries.insert(
                key.to_owned(),
                (summary_text.to_owned(), last_active.clone()),
            );
            session.metadata.insert(
                LAST_SUMMARY_KEY.to_owned(),
                json!({"text": summary_text, "last_active": last_active}),
            );
        }
        session.messages = kept_msgs.clone();
        session.last_consolidated = 0;
        session.updated_at = Local::now().to_rfc3339();
        sessions.save(&session)?;
        self.archiving.remove(key);
        Ok(AutoCompactArchiveOutcome {
            key: key.to_owned(),
            archived_messages: archive_msgs,
            kept_messages: kept_msgs,
            summary_stored,
        })
    }

    pub fn prepare_session(
        &mut self,
        sessions: &mut SessionManager,
        mut session: Session,
        key: &str,
    ) -> std::io::Result<(Session, Option<String>)> {
        if self.archiving.contains(key) || self.is_expired(Some(&session.updated_at), Local::now())
        {
            sessions.invalidate(key);
            session = sessions.get_or_create(key);
        }

        if let Some((summary, last_active)) = self.summaries.remove(key) {
            session.metadata.remove(LAST_SUMMARY_KEY);
            sessions.save(&session)?;
            return Ok((session, Some(format_summary(&summary, &last_active))));
        }

        if let Some(metadata) = session.metadata.remove(LAST_SUMMARY_KEY) {
            sessions.save(&session)?;
            if let Some((summary, last_active)) = parse_summary_metadata(&metadata) {
                return Ok((session, Some(format_summary(&summary, &last_active))));
            }
        }
        Ok((session, None))
    }
}

fn parse_summary_metadata(metadata: &Value) -> Option<(String, String)> {
    let object = metadata.as_object()?;
    let text = object.get("text")?.as_str()?.to_owned();
    let last_active = object.get("last_active")?.as_str()?.to_owned();
    Some((text, last_active))
}

fn format_summary(summary: &str, last_active: &str) -> String {
    let idle_min = DateTime::parse_from_rfc3339(last_active)
        .ok()
        .map(|parsed| {
            Local::now()
                .signed_duration_since(parsed.with_timezone(&Local))
                .num_minutes()
        })
        .unwrap_or(0)
        .max(0);
    format!("Inactive for {idle_min} minutes.\nPrevious conversation summary: {summary}")
}
