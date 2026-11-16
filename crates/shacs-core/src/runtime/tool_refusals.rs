//! Refusals admitted once, with immutable outcomes and root-scoped counts.

use super::tool_refusal::{RefusalKind, ToolRefusal};
use super::{RuntimeToolMessage, ToolFailureClass};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_session::Session;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) const ROOT_KEY: &str = "tool_refusal_root";
const COUNTS_KEY: &str = "tool_refusal_counts";
const LIMIT: u32 = 3;
const GUIDANCE: &str = "This refusal is a policy boundary; retrying the same action will not change it. Do not bypass it with symlinks, encoding, another tool, or a different working_dir. Work inside the workspace if possible; otherwise tell the user what cannot be done and ask how to proceed.";

#[derive(Clone, Default)]
pub(crate) struct ToolRefusals {
    state: Arc<Mutex<RefusalState>>,
    root: Option<String>,
}

#[derive(Default)]
struct RefusalState {
    counts: BTreeMap<String, u32>,
    decisions: BTreeMap<String, RefusalDecision>,
    classifier_blocks: BTreeSet<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct RefusalDecision {
    pub(crate) kind: RefusalKind,
    pub(crate) count: u32,
    pub(crate) fatal: bool,
}

impl ToolRefusals {
    pub(crate) fn load(session: &Session, root: &str) -> io::Result<Self> {
        let mut roots: BTreeMap<String, BTreeMap<String, u32>> =
            match session.metadata.get(COUNTS_KEY) {
                Some(roots) => serde_json::from_value(roots.clone()).map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("{COUNTS_KEY}: {error}"))
                })?,
                None => BTreeMap::new(),
            };
        let counts = roots.remove(root).unwrap_or_default();
        Ok(Self {
            state: Arc::new(Mutex::new(RefusalState {
                counts,
                ..RefusalState::default()
            })),
            root: Some(root.to_owned()),
        })
    }

    pub(crate) fn store(&self, session: &mut Session) {
        let Some(root) = &self.root else { return };
        let state = self.state();
        if state.counts.is_empty() {
            return;
        }
        let roots = session
            .metadata
            .entry(COUNTS_KEY.to_owned())
            .or_insert_with(|| json!({}));
        roots[root] = json!(state.counts);
    }

    pub(crate) fn finish(&self, session: &mut Session) {
        let Some(root) = &self.root else { return };
        finish_root(session, root);
    }

    pub(crate) fn begin_dispatch(&self) {
        let mut state = self.state();
        state.decisions.clear();
        state.classifier_blocks.clear();
    }

    pub(crate) fn mark_classifier_block(&self, message: &RuntimeToolMessage) {
        self.state().classifier_blocks.insert(message_key(message));
    }

    pub(crate) fn is_classifier_block(&self, message: &RuntimeToolMessage) -> bool {
        self.state()
            .classifier_blocks
            .contains(&message_key(message))
    }

    /// The caller admits each result once while its original action is still
    /// available. Later classification only reads `decision`.
    pub(crate) fn admit(
        &self,
        refusal: &ToolRefusal,
        message: &mut RuntimeToolMessage,
        failure_class: ToolFailureClass,
    ) -> RefusalDecision {
        let mut state = self.state();
        let count = state.counts.entry(refusal.key()).or_default();
        *count = count.saturating_add(1);
        let decision = RefusalDecision {
            kind: refusal.kind,
            count: *count,
            fatal: failure_class == ToolFailureClass::Fatal || *count >= LIMIT,
        };
        if !decision.fatal {
            message.content.push_str(&format!(
                "\n\n{GUIDANCE}\nRefusal repetition: {}/{LIMIT} in this request.",
                decision.count
            ));
            match refusal.kind {
                RefusalKind::Workspace => message.content.push_str(
                    " The user can turn off tools.restrictToWorkspace; other guards may still refuse the call.",
                ),
                RefusalKind::InternalUrl => message.content.push_str(
                    " The user can configure tools.ssrfWhitelist for an intended internal URL.",
                ),
                RefusalKind::WorkingDirectory
                | RefusalKind::FileRange
                | RefusalKind::DenyPattern
                | RefusalKind::InvalidDenyPattern
                | RefusalKind::AllowPattern
                | RefusalKind::ToolPreflight => {}
            }
        }
        state.decisions.insert(message_key(message), decision);
        decision
    }

    pub(crate) fn decision(&self, message: &RuntimeToolMessage) -> Option<RefusalDecision> {
        self.state().decisions.get(&message_key(message)).copied()
    }

    fn state(&self) -> MutexGuard<'_, RefusalState> {
        self.state
            .lock()
            .expect("tool refusal counts lock poisoned")
    }
}

pub(crate) fn clear(session: &mut Session) {
    session.metadata.remove(COUNTS_KEY);
    session.metadata.remove(ROOT_KEY);
}

pub(crate) fn root_id(value: Option<&Value>) -> io::Result<Option<&str>> {
    match value {
        Some(Value::String(root)) => Ok(Some(root)),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{ROOT_KEY}: expected a string"),
        )),
        None => Ok(None),
    }
}

fn finish_root(session: &mut Session, root: &str) {
    if let Some(Value::Object(roots)) = session.metadata.get_mut(COUNTS_KEY) {
        roots.remove(root);
    }
    if session.metadata.get(ROOT_KEY).and_then(Value::as_str) == Some(root) {
        session.metadata.remove(ROOT_KEY);
    }
}

fn message_key(message: &RuntimeToolMessage) -> String {
    // Image arrays have already been persisted and converted at admission;
    // presentation must not change between admission and classification.
    // The count in the admitted text distinguishes even identical results
    // with a repeated call id. Fatal results keep their original text; equal
    // fatal results have the same outcome. Bridge renaming changes no key.
    let value = json!([message.tool_call_id, message.content]);
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
