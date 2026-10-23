use super::task_method::MethodRequest;
use super::task_method_compose_quote::{withheld_claim, withholds};
use super::task_method_report_render::carried;

pub(super) fn details(request: &MethodRequest) -> String {
    let evidence = &request.report;
    if evidence.sources.is_empty() && evidence.verdict.is_none() {
        return String::new();
    }
    let mut text = "\nReport sources:".to_owned();
    for source in &evidence.sources {
        let earlier = !source.carried && source.selection_revision != request.selection.revision;
        text.push_str(&format!(
            "\n- #{} {} [{:?}]{} {}{}",
            source.seq,
            source.locator,
            source.status,
            if earlier {
                format!(
                    " revision {}; superseded_not_inherited",
                    source.selection_revision
                )
            } else {
                String::new()
            },
            source.read_at,
            carried(source, evidence.version)
        ));
        if let Some(digest) = source
            .file_digest
            .as_ref()
            .or(source.content_digest.as_ref())
        {
            text.push_str(&format!(" {digest}"));
        }
        if let Some(error) = &source.error {
            text.push_str(&format!("\n  error: {error}"));
        }
    }
    if evidence.omitted_sources > 0 {
        text.push_str(&format!(
            "\nSources omitted by retention limit: {}",
            evidence.omitted_sources
        ));
    }
    if let Some(verdict) = &evidence.verdict {
        text.push_str("\nReport conclusions:");
        for claim in &verdict.claims {
            let citations: Vec<String> = claim
                .citations
                .iter()
                .map(|citation| format!("{} ({})", citation.locator, citation.status.label()))
                .collect();
            let citations = if citations.is_empty() {
                "no source cited".to_owned()
            } else {
                citations.join(", ")
            };
            let quoted = if withheld_claim(request, claim) {
                "(withheld)".to_owned()
            } else {
                format!("\"{}\"", claim.text)
            };
            text.push_str(&format!("\n- {quoted} -> {citations}"));
            if claim.unresolved {
                text.push_str(" [unresolved]");
            }
            if !claim.conflict.is_empty() {
                text.push_str(&format!(" [conflict: {}]", claim.conflict.join("; ")));
            }
        }
        for line in &verdict.uncited {
            if withholds(request) {
                text.push_str("\n- (withheld) -> no source marker");
            } else {
                text.push_str(&format!("\n- \"{line}\" -> no source marker"));
            }
        }
        if !verdict.removed_claims.is_empty() && !withholds(request) {
            text.push_str("\nRemoved or reworded since the earlier version:");
            for line in &verdict.removed_claims {
                text.push_str(&format!("\n- \"{line}\""));
            }
        }
    }
    text
}
