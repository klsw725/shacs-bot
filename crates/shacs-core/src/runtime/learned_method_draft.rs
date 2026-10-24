use super::learned_method::{EvidenceKind, SourceTrust, UserStatement};
use super::learned_method_evidence::EvidenceSet;
use serde::de::DeserializeOwned;
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Classification {
    #[serde(default)]
    classified: Vec<Classified>,
}

#[derive(Deserialize)]
struct Classified {
    evidence: String,
    #[serde(rename = "as")]
    statement: UserStatement,
}

#[derive(Deserialize)]
pub(super) struct Draft {
    #[serde(default)]
    pub candidate: Option<CandidateDraft>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Deserialize)]
pub(super) struct CandidateDraft {
    #[serde(default)]
    pub purpose: CitedDraft,
    #[serde(default)]
    pub applies_to: CitedDraft,
    #[serde(default)]
    pub premises: Vec<CitedDraft>,
    #[serde(default)]
    pub steps: Vec<CitedDraft>,
    #[serde(default)]
    pub unverified: Vec<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct CitedDraft {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

pub(super) fn parse<T: DeserializeOwned>(content: &str) -> Option<T> {
    let trimmed = content.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|rest| rest.trim_end().trim_end_matches("```"))
        .unwrap_or(trimmed);
    serde_json::from_str(body.trim()).ok()
}

/// Labels the user's own messages; an unlabelled one counts as `other`.
pub(super) fn classify(evidence: &mut EvidenceSet, sorted: Classification) {
    for item in &mut evidence.items {
        if item.record.kind == EvidenceKind::UserMessage && item.record.trust == SourceTrust::Owner
        {
            let label = sorted
                .classified
                .iter()
                .find(|classified| classified.evidence == item.record.id);
            item.record.classified_as =
                Some(label.map_or(UserStatement::Other, |label| label.statement));
        }
    }
}
