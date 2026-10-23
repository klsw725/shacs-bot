use super::task_method::{MethodId, MethodRequest};
use super::task_method_calc_render::acceptance;
use super::task_method_compose_quote::withholds;
use super::task_method_report::{ReportSource, ReportState, SourceStatus};

pub(super) const REPORT_PROTOCOL: &str = "Report protocol: read every source you rely on with read_file (or web_fetch when available) in this request. End each conclusion line with [source: <path or URL exactly as you read it>]; use [source: a; b] for several (only ';' separates sources). A statement line without a marker is reported as not source-checked; if you save the report to a file, reply only with a short pointer such as \"Saved: report.md\". Mark open points with [unresolved: <what is missing>] and disagreeing sources with [conflict: a; b]. Never cite a source you did not read in this request. The runtime checks that each citation links to a source actually read; it does not certify that conclusions are true, so never call them verified.";

/// Context for the model when this request revises an earlier report.
pub(super) fn revision_context(request: &MethodRequest) -> String {
    let evidence = &request.report;
    let Some(previous) = &evidence.revision_of else {
        return String::new();
    };
    format!(
        "\nThis request is version {} revising the report from {previous}. Original request: {}. Sources read for earlier versions are carried and re-checked: a file counts only while it is unchanged; read changed files and web pages again.",
        evidence.version,
        evidence.purpose.as_deref().unwrap_or("not recorded")
    )
}

pub(super) fn summary(request: &MethodRequest) -> String {
    let evidence = &request.report;
    let Some(verdict) = evidence.verdict.as_ref() else {
        return String::new();
    };
    if !request
        .selection
        .parts()
        .contains(&MethodId::SourceGroundedReport)
        || verdict.selection_revision != request.selection.revision
    {
        return format!(
            "\nReport evidence from revision {}: superseded_not_inherited (was {}).",
            verdict.selection_revision,
            verdict.state.label()
        );
    }
    let mut text = format!("\nReport: {} — {}", verdict.state.label(), verdict.reason);
    // Only sources the verdict can use: read in this revision, or carried.
    let usable = |source: &&ReportSource| {
        source.carried || source.selection_revision == verdict.selection_revision
    };
    // With an image part, a source shows as its number: the model chose its
    // path or address, and a read's error comes from the tool's output (a
    // file it wrote); both stay in /method status.
    let withheld = withholds(request);
    let name = |source: &ReportSource| {
        if withheld {
            format!("source #{}", source.seq)
        } else {
            source.locator.clone()
        }
    };
    let read: Vec<String> = evidence
        .sources
        .iter()
        .filter(usable)
        .filter(|source| source.status == SourceStatus::Read)
        .map(|source| format!("{}{}", name(source), carried(source, evidence.version)))
        .collect();
    let named = if withheld {
        " (names in /method status)"
    } else {
        ""
    };
    if !read.is_empty() {
        text.push_str(&format!("\nSources read: {}{named}", read.join("; ")));
    }
    let failed: Vec<String> = evidence
        .sources
        .iter()
        .filter(usable)
        .filter(|source| source.status == SourceStatus::Failed && !source.carried)
        .map(|source| match (withheld, source.error.as_deref()) {
            (true, _) => name(source),
            (false, error) => format!("{} ({})", source.locator, error.unwrap_or("failed")),
        })
        .collect();
    if !failed.is_empty() {
        let errors = if withheld {
            " (names and errors in /method status)"
        } else {
            ""
        };
        text.push_str(&format!(
            "\nSources that could not be read: {}{errors}",
            failed.join("; ")
        ));
    }
    let mut hidden = 0;
    for claim in verdict.claims.iter().filter(|claim| {
        claim
            .citations
            .iter()
            .any(|citation| !citation.status.linked())
    }) {
        if withheld {
            hidden += 1;
            continue;
        }
        let problems: Vec<String> = claim
            .citations
            .iter()
            .filter(|citation| !citation.status.linked())
            .map(|citation| format!("{} ({})", citation.locator, citation.status.label()))
            .collect();
        text.push_str(&format!(
            "\nNot backed: \"{}\" cites {}",
            claim.text,
            problems.join(", ")
        ));
    }
    if hidden > 0 {
        text.push_str(&format!(
            "\nNot backed: {hidden} cited lines (withheld from the answer)"
        ));
    }
    let conflicts = verdict
        .claims
        .iter()
        .filter(|claim| !claim.conflict.is_empty())
        .count();
    let unresolved = verdict
        .claims
        .iter()
        .filter(|claim| claim.unresolved)
        .count();
    if conflicts + unresolved > 0 {
        text.push_str(&format!(
            "\nOpen points: {conflicts} conflicting-source conclusions, {unresolved} unresolved"
        ));
    }
    if verdict.uncited_lines > 0 && withheld {
        text.push_str(&format!(
            "\nLines without a source marker (not source-checked): {} (withheld from the answer)",
            verdict.uncited_lines
        ));
    } else if verdict.uncited_lines > 0 {
        text.push_str(&format!(
            "\nLines without a source marker (not source-checked): {} — {}",
            verdict.uncited_lines,
            quoted(&verdict.uncited, verdict.uncited_lines)
        ));
    }
    if verdict.unchecked > 0 {
        text.push_str(&format!(
            "\nParts beyond the checking limits (not checked): {}",
            verdict.unchecked
        ));
    }
    if let Some(previous) = &evidence.revision_of {
        text.push_str(&format!(
            "\nVersion {} revising {previous}: {} new or changed conclusions, {} earlier conclusions removed or reworded; earlier verification is re-checked, not inherited",
            evidence.version, verdict.changed_claims, verdict.removed_claims.len()
        ));
        let mut changed: Vec<String> = Vec::new();
        let marked = verdict
            .claims
            .iter()
            .filter(|claim| claim.statement)
            .map(|claim| {
                let linked = !claim.citations.is_empty()
                    && claim
                        .citations
                        .iter()
                        .all(|citation| citation.status.linked());
                (&claim.text, if linked { "linked" } else { "not backed" })
            });
        let unmarked = verdict
            .uncited
            .iter()
            .map(|line| (line, "no source marker"));
        for (text, state) in marked.chain(unmarked) {
            let entry = format!("\"{text}\" ({state})");
            if !evidence.previous_claims.contains(text) && !changed.contains(&entry) {
                changed.push(entry);
            }
        }
        changed.truncate(5);
        if withheld {
            changed.clear();
        }
        if !changed.is_empty() {
            text.push_str(&format!("\nNew or changed: {}", changed.join("; ")));
        }
        if !verdict.removed_claims.is_empty() && !withheld {
            text.push_str(&format!(
                "\nRemoved or reworded: {}",
                quoted(&verdict.removed_claims, verdict.removed_claims.len())
            ));
        }
    } else if verdict.state != ReportState::NotWritten
        && request.selection.method == MethodId::SourceGroundedReport
    {
        text.push_str("\nTo revise this report: /method revise, then describe the change.");
    }
    if request.selection.method != MethodId::Composed {
        text.push_str(&format!(
            "\nUser acceptance: {}",
            acceptance(request.user_acceptance.as_ref())
        ));
    }
    text
}

/// Up to five quoted lines out of `total`, noting how many were left out.
fn quoted(lines: &[String], total: usize) -> String {
    let mut shown: Vec<String> = lines
        .iter()
        .take(5)
        .map(|line| format!("\"{line}\""))
        .collect();
    if total > shown.len() {
        shown.push(format!("and {} more", total - shown.len()));
    }
    shown.join("; ")
}

/// Where a carried source comes from: an earlier version, or this version
/// before a method change.
pub(super) fn carried(source: &ReportSource, version: u64) -> String {
    match (source.carried, source.version == version) {
        (false, _) => String::new(),
        (true, true) => " (read before the method change)".to_owned(),
        (true, false) => format!(" (from version {})", source.version),
    }
}
