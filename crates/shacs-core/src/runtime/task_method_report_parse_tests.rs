use super::*;
use crate::runtime::task_method_report::{ReportEvidence, ReportSource, ReportState, SourceKind};
use crate::runtime::task_method_report_eval::{evaluate, Freshness};
use crate::runtime::task_method_report_source::{source, FileRead};

fn linked_notes() -> ReportEvidence {
    let mut evidence = ReportEvidence::default();
    let file = Some(FileRead {
        requested: "/ws/notes.md".to_owned(),
        path: "/ws/notes.md".to_owned(),
        relative: "notes.md".to_owned(),
        digest: "sha256:fixed".to_owned(),
    });
    let read = source("call", SourceKind::File, "notes.md", "1| notes", file);
    evidence.push_source(read, 1);
    evidence
}

fn state(text: &str) -> ReportState {
    let mut evidence = linked_notes();
    evidence.push_output(output(OutputKind::Answer, "answer", text), 1);
    evaluate(&evidence, 1, &|_: &ReportSource| Freshness::Unchanged).state
}

#[test]
fn short_and_korean_statements_without_a_marker_are_counted() {
    // Given
    let korean = "매출은 증가했다 [source: notes.md]\n비용은 감소했다";
    let short = "Revenue grew [source: notes.md]\nCosts fell.";
    let layout = "# 요약\nHere is the report:\n---\n| a | b |\n|---|---|\n```\nRevenue grew [source: notes.md]\n```\nSaved: report.md";
    // When
    let parsed = output(OutputKind::Answer, "answer", korean);
    // Then
    assert_eq!(parsed.uncited, ["비용은 감소했다"]);
    assert_eq!(state(korean), ReportState::PartlyCited);
    assert_eq!(state(short), ReportState::PartlyCited);
    assert_eq!(
        output(OutputKind::Answer, "answer", layout).uncited,
        ["| a | b |"]
    );
}

#[test]
fn conclusions_beyond_the_checking_limits_keep_a_report_from_being_linked() {
    // Given
    let many = "Revenue grew [source: notes.md]\n".repeat(CLAIM_LIMIT + 1);
    let long = format!(
        "{0} [source: notes.md]\n{0} [source: notes.md]\nCosts rose [source: fake.md]",
        "a".repeat(33_000)
    );
    // When / Then
    assert_eq!(
        state(&"Revenue grew [source: notes.md]\n".repeat(CLAIM_LIMIT)),
        ReportState::Linked
    );
    assert_ne!(state(&many), ReportState::Linked);
    assert_ne!(state(&long), ReportState::Linked);
}

#[test]
fn marker_only_lines_and_empty_markers_are_not_backed_conclusions() {
    // Given
    let empty_marker = "Grounded [source: notes.md]\nUnsupported claim here [source: ]";
    let marker_only = "[source: notes.md]";
    // When / Then
    assert_eq!(state(empty_marker), ReportState::PartlyCited);
    assert_eq!(
        output(OutputKind::Answer, "answer", empty_marker).uncited,
        ["Unsupported claim here"]
    );
    assert_eq!(state(marker_only), ReportState::NotWritten);
}

#[test]
fn cited_pointers_lead_ins_and_headings_are_not_report_content() {
    // Given
    let pointer = "Saved: report.md [source: notes.md]";
    let lead_in = "Here is the report: [source: notes.md]";
    let heading = "# Revenue [source: fake.md]\nRevenue grew [source: notes.md]";
    // When / Then
    assert_eq!(state(pointer), ReportState::NotWritten);
    assert_eq!(state(lead_in), ReportState::NotWritten);
    assert_eq!(
        state("Revenue grew [source: notes.md]"),
        ReportState::Linked
    );
    assert_eq!(state(heading), ReportState::Problems);
}

#[test]
fn file_reports_with_long_paths_that_share_a_prefix_are_both_checked() {
    // Given
    let base = format!("reports/{}", "a".repeat(300));
    let mut evidence = linked_notes();
    let first = output(
        OutputKind::File,
        &format!("{base}/one.md"),
        "Costs rose [source: fake.md]",
    );
    let second = output(
        OutputKind::File,
        &format!("{base}/two.md"),
        "Revenue grew [source: notes.md]",
    );
    evidence.push_output(first, 1);
    evidence.push_output(second, 1);
    // When
    let verdict = evaluate(&evidence, 1, &|_: &ReportSource| Freshness::Unchanged);
    // Then
    assert_eq!(evidence.outputs.len(), 2);
    assert_eq!(verdict.state, ReportState::Problems);
}
