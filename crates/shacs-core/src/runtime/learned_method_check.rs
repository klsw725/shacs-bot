use super::learned_method::{EvidenceKind, SourceTrust, UserStatement};
use super::learned_method_draft::CitedDraft;
use super::learned_method_evidence::{bounded, Evidence};
use super::learned_method_generate::shown_text;
use super::learned_method_guard::{HiddenText, Kept, RequestText};
use std::collections::BTreeSet;

/// Checks draft lines against what the draft call was and was not shown.
pub(super) struct Checker<'a> {
    items: &'a [Evidence],
    hidden: HiddenText,
    requests: RequestText,
    pub unknown_citations: usize,
    pub kept_out: usize,
}

impl<'a> Checker<'a> {
    pub fn new(items: &'a [Evidence]) -> Self {
        let shown: Vec<&str> = items.iter().filter_map(shown_text).collect();
        let kept_out = items
            .iter()
            .filter(|item| shown_text(item).is_none())
            .map(|item| (kept(item), item.text.as_str()))
            .chain(items.iter().filter_map(|item| {
                item.material
                    .as_deref()
                    .map(|material| (Kept::Hidden, material))
            }));
        let said = |statement: UserStatement| {
            items
                .iter()
                .filter(move |item| item.record.classified_as == Some(statement))
                .filter_map(shown_text)
        };
        Self {
            items,
            hidden: HiddenText::new(kept_out, shown),
            requests: RequestText::new(
                said(UserStatement::Request),
                said(UserStatement::Procedure).chain(said(UserStatement::Feedback)),
            ),
            unknown_citations: 0,
            kept_out: 0,
        }
    }

    /// The draft's text with the cited entries the model was shown. A text
    /// that repeats or cites what was kept from the model, or copies a
    /// request, is dropped whole.
    pub fn check(
        &mut self,
        draft: &CitedDraft,
        limit: usize,
    ) -> Option<(String, Vec<&'a Evidence>)> {
        let text = bounded(draft.text.trim(), limit);
        if text.is_empty() {
            return None;
        }
        let ids: BTreeSet<&str> = draft.evidence.iter().map(String::as_str).collect();
        let cited: Vec<&'a Evidence> = self
            .items
            .iter()
            .filter(|item| ids.contains(item.record.id.as_str()))
            .collect();
        if self.hidden.copied(&text)
            || self.requests.copied(&text)
            || cited.iter().any(|item| !citable(item))
        {
            self.kept_out += 1;
            return None;
        }
        self.unknown_citations += ids.len() - cited.len();
        Some((text, cited))
    }
}

/// Entries the drafting call saw: excluded messages of the user are not.
fn citable(item: &Evidence) -> bool {
    !(item.record.kind == EvidenceKind::UserMessage
        && item.record.trust == SourceTrust::Owner
        && shown_text(item).is_none())
}

fn kept(item: &Evidence) -> Kept {
    if item
        .record
        .classified_as
        .is_some_and(UserStatement::excluded)
    {
        Kept::Excluded
    } else {
        Kept::Hidden
    }
}
