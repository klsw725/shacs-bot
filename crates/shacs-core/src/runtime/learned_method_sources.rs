use super::learned_method::{EvidenceKind, MethodEvidence, SourceTrust};
use super::learned_method_evidence::{recorded, trusts, Evidence};
use super::learned_method_source::{call_id, source_in, timestamp};
use serde_json::Value;
use std::collections::HashMap;

/// One session's messages, indexed once however many saved methods cite it.
pub(super) struct Sources<'a> {
    pub messages: &'a [Value],
    trusts: Vec<SourceTrust>,
    by_time: HashMap<&'a str, Vec<usize>>,
    derived: HashMap<(usize, Option<String>), Evidence>,
}

impl<'a> Sources<'a> {
    pub fn new(messages: &'a [Value]) -> Self {
        let mut by_time: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, message) in messages.iter().enumerate() {
            by_time
                .entry(timestamp(message).unwrap_or_default())
                .or_default()
                .push(index);
        }
        Self {
            messages,
            trusts: trusts(messages),
            by_time,
            derived: HashMap::new(),
        }
    }

    /// Where the message `record` came from still is, as written and at the
    /// same time, and the call in it for a tool call.
    pub fn find(&self, record: &MethodEvidence) -> Option<(usize, Option<&'a Value>)> {
        let messages = self.messages;
        self.by_time
            .get(record.at.as_deref().unwrap_or_default())?
            .iter()
            .find_map(|&index| source_in(&messages[index], record).map(|call| (index, call)))
    }

    pub fn derive(&mut self, index: usize, call: Option<&Value>, kind: EvidenceKind) -> &Evidence {
        let (messages, trusts) = (self.messages, &self.trusts);
        self.derived
            .entry((index, call.map(|call| call_id(call).to_owned())))
            .or_insert_with(|| recorded(messages, index, call, kind, trusts[index]))
    }
}
