use super::learned_method::{
    CitedText, EvidenceKind, LearnedMethodVersion, MethodEvidence, MethodStep, RunOutcome,
    SourceTrust, StepSupport, UserStatement,
};
use super::learned_method_recheck::Seen;
use super::learned_method_render::Checked;

/// One line of the evidence list: references and digests, never content.
pub(super) fn evidence(record: &MethodEvidence) -> String {
    let mut text = format!("- {} {}", record.id, kind(record));
    if let Some(tool) = &record.tool {
        text.push_str(&format!(" {tool}"));
    }
    if let Some(outcome) = record.outcome {
        text.push_str(match outcome {
            RunOutcome::Ok => " (ran without a reported error)",
            RunOutcome::Error => " (failed)",
            RunOutcome::Incomplete => " (did not finish)",
        });
    }
    text.push_str(&format!(
        " · {} · {} · {}",
        trust(record.trust),
        record.source_ref,
        short(&record.digest)
    ));
    if let Some(material) = &record.material_digest {
        text.push_str(&format!(" · material {} (untrusted)", short(material)));
    }
    text
}

fn kind(record: &MethodEvidence) -> String {
    match (record.kind, record.classified_as) {
        (EvidenceKind::UserMessage, Some(statement_kind)) => {
            format!("message: {}", statement(statement_kind))
        }
        (EvidenceKind::UserMessage, None) => "message".to_owned(),
        (EvidenceKind::ToolCall, _) => "tool call".to_owned(),
        (EvidenceKind::Answer, _) => "assistant answer".to_owned(),
    }
}

pub(super) fn statement(statement: UserStatement) -> &'static str {
    match statement {
        UserStatement::Request => "request",
        UserStatement::Feedback => "feedback",
        UserStatement::Procedure => "procedure",
        UserStatement::Preference => "preference",
        UserStatement::TaskCondition => "condition for that task only",
        UserStatement::Mixed => "mixed with a preference or one-task condition",
        UserStatement::Other => "other",
    }
}

fn trust(trust: SourceTrust) -> &'static str {
    match trust {
        SourceTrust::Owner => "from you",
        SourceTrust::Model => "assistant output",
        SourceTrust::Untrusted => "untrusted",
    }
}

fn short(digest: &str) -> &str {
    digest.get(..19).unwrap_or(digest)
}

/// How a saved record's source stands now (see `learned_method_recheck`).
pub(super) fn since(seen: Option<&Seen>) -> &'static str {
    match seen {
        Some(Seen::Present) => "",
        Some(Seen::Moved) => " · saved position not rechecked: the session changed since",
        Some(Seen::Changed) => " · its result changed after saving, not rechecked",
        None => " · no longer in that session",
    }
}

pub(super) fn cited(text: &CitedText, checked: &Checked) -> String {
    if let Some(made) = text.edited_in {
        return format!("{} (your edit in v{made})", text.text);
    }
    let note = checked
        .unchecked(&text.evidence)
        .map(|reason| format!("; {reason}"))
        .unwrap_or_default();
    format!(
        "{} (the model's wording, from {}{note})",
        text.text,
        text.evidence.join(", ")
    )
}

/// A run or a quote is shown as such only while its source can be rechecked.
pub(super) fn support(
    step: &MethodStep,
    version: &LearnedMethodVersion,
    checked: &Checked,
) -> String {
    let cited: Vec<&MethodEvidence> = step
        .evidence
        .iter()
        .filter_map(|id| version.evidence.iter().find(|record| &record.id == id))
        .collect();
    let (saved, not_rechecked) = match checked.unchecked(&step.evidence) {
        None => ("", String::new()),
        Some(reason) => (" when saved", format!("; {reason}")),
    };
    let mut text = match step.support {
        StepSupport::ObservedRun => {
            let runs: Vec<String> = cited
                .iter()
                .filter(|record| record.outcome == Some(RunOutcome::Ok))
                .map(|record| format!("{} {}", record.id, record.tool.as_deref().unwrap_or("tool")))
                .collect();
            let note = step.model_note.as_deref().unwrap_or_default();
            format!(
                "ran without a reported error{saved} ({}){not_rechecked}; the model's description, not checked: {note}",
                runs.join(", ")
            )
        }
        StepSupport::UserStated => {
            format!("your own words{saved} ({}){not_rechecked}", ids(&cited))
        }
        StepSupport::UserEdited => step.edited_in.map_or_else(
            || "your edit".to_owned(),
            |made| format!("your edit in v{made}"),
        ),
        StepSupport::Unverified if cited.is_empty() => {
            "unverified: the model's wording, no supporting evidence".to_owned()
        }
        StepSupport::Unverified => {
            format!(
                "unverified: the model's wording ({}){not_rechecked}",
                ids(&cited)
            )
        }
    };
    if cited
        .iter()
        .any(|record| record.trust == SourceTrust::Untrusted)
    {
        text.push_str("; influenced by untrusted material");
    }
    text
}

fn ids(cited: &[&MethodEvidence]) -> String {
    cited
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
