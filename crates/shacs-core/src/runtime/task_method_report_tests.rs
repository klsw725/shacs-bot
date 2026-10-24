use super::*;
use crate::runtime::task_method_report::{
    CitationStatus, OutputKind, ReportEvidence, ReportSource, ReportState, SourceKind, SourceStatus,
};
use crate::runtime::task_method_report_claims::report_claims;
use crate::runtime::task_method_report_parse::{locator_key, output};
use crate::runtime::task_method_report_source::{source, FileRead};

fn read(evidence: &mut ReportEvidence, locator: &str, result: &str) {
    let file = Some(FileRead {
        requested: format!("/ws/{locator}"),
        path: format!("/ws/{locator}"),
        relative: locator.to_owned(),
        digest: "sha256:fixed".to_owned(),
    });
    evidence.push_source(source("call", SourceKind::File, locator, result, file), 1);
}

fn answer(evidence: &mut ReportEvidence, text: &str) {
    evidence.push_output(output(OutputKind::Answer, "answer", text), 1);
}

fn same(_: &ReportSource) -> Freshness {
    Freshness::Unchanged
}

#[test]
fn markers_become_claims_with_normalized_citations() {
    // Given
    let text = "# Summary\nRevenue grew [source: ./notes.md; `data/q3.csv`]\nCosts are unclear [unresolved: no Q4 data]\nThe two disagree [conflict: a.md; b.md] [source: a.md]\nThis plain sentence has no marker at all.\nSee [figure 1] for context [source: https://example.com/report/]";
    // When
    let parsed = output(OutputKind::Answer, "answer", text);
    // Then
    assert_eq!(parsed.claims.len(), 4);
    assert_eq!(parsed.claims[0].text, "Revenue grew");
    let locators: Vec<_> = parsed.claims[0]
        .citations
        .iter()
        .map(|c| c.locator.as_str())
        .collect();
    assert_eq!(locators, ["notes.md", "data/q3.csv"]);
    assert!(parsed.claims[1].unresolved);
    assert_eq!(parsed.claims[2].conflict, ["a.md", "b.md"]);
    assert_eq!(parsed.claims[3].text, "See [figure 1] for context");
    assert_eq!(
        parsed.claims[3].citations[0].locator,
        "https://example.com/report"
    );
    assert_eq!(parsed.uncited_lines, 1);
    assert_eq!(
        parsed.uncited,
        ["This plain sentence has no marker at all."]
    );
    assert_eq!(locator_key("./notes.md"), locator_key("`notes.md`"));
    assert_ne!(locator_key("docs/notes.md"), locator_key("notes.md"));
    assert_ne!(locator_key("otes.md"), locator_key("notes.md"));
}

#[test]
fn every_citation_must_link_to_a_source_read_for_this_report() {
    // Given
    let mut linked = ReportEvidence::default();
    read(&mut linked, "notes.md", "1| revenue grew");
    answer(&mut linked, "Revenue grew [source: notes.md]");
    let mut broken = linked.clone();
    read(
        &mut broken,
        "missing.md",
        "Error: File not found: missing.md",
    );
    answer(&mut broken, "Revenue grew [source: notes.md]\nQ4 fell [source: missing.md]\nCosts rose [source: imaginary.pdf]");
    // When
    let ok = evaluate(&linked, 1, &same);
    let bad = evaluate(&broken, 1, &same);
    // Then
    assert_eq!(ok.state, ReportState::Linked);
    assert_eq!(bad.state, ReportState::Problems);
    let statuses: Vec<_> = bad
        .claims
        .iter()
        .map(|claim| claim.citations[0].status)
        .collect();
    assert_eq!(
        statuses,
        [
            CitationStatus::Linked,
            CitationStatus::ReadFailed,
            CitationStatus::Unread
        ]
    );
    let checks = report_claims(&bad);
    assert_eq!(
        checks[0].state,
        crate::runtime::task_method::ClaimState::Failed
    );
    assert_eq!(
        checks[1].state,
        crate::runtime::task_method::ClaimState::Failed
    );
}

#[test]
fn missing_reads_citations_or_text_never_count_as_source_grounded() {
    // Given
    let mut nothing_read = ReportEvidence::default();
    answer(
        &mut nothing_read,
        "Revenue grew a lot this year overall [source: notes.md]",
    );
    let mut all_failed = ReportEvidence::default();
    read(
        &mut all_failed,
        "notes.md",
        "Error: File not found: notes.md",
    );
    answer(&mut all_failed, "Revenue grew [source: notes.md]");
    let mut uncited = ReportEvidence::default();
    read(&mut uncited, "notes.md", "1| text");
    answer(&mut uncited, "Revenue grew a lot this year overall.");
    let mut unwritten = ReportEvidence::default();
    read(&mut unwritten, "notes.md", "1| text");
    // When / Then
    assert_eq!(
        evaluate(&nothing_read, 1, &same).state,
        ReportState::NoSources
    );
    assert_eq!(
        evaluate(&all_failed, 1, &same).state,
        ReportState::Unavailable
    );
    assert_eq!(evaluate(&uncited, 1, &same).state, ReportState::Uncited);
    assert_eq!(
        evaluate(&unwritten, 1, &same).state,
        ReportState::NotWritten
    );
    assert_eq!(evaluate(&uncited, 2, &same).state, ReportState::NotWritten);
}

#[test]
fn changed_and_carried_sources_are_rechecked_not_inherited() {
    // Given
    let mut first = ReportEvidence::default();
    read(&mut first, "notes.md", "1| v1");
    let web = source("call", SourceKind::Web, "https://example.com", "page", None);
    first.push_source(web, 1);
    answer(
        &mut first,
        "Revenue grew [source: notes.md]\nPrices rose [source: https://example.com]",
    );
    first.verdict = Some(evaluate(&first, 1, &same));
    let mut second = ReportEvidence::next_version(&first, "method-request-1", "Write a report");
    answer(&mut second, "Revenue grew [source: notes.md]\nPrices rose [source: https://example.com]\nNew point [source: notes.md]");
    // When
    let stale = evaluate(&first, 1, &|_: &ReportSource| Freshness::Changed);
    let carried = evaluate(&second, 1, &same);
    // Then
    assert_eq!(stale.state, ReportState::Problems);
    assert_eq!(stale.claims[0].citations[0].status, CitationStatus::Stale);
    assert_eq!(second.version, 2);
    assert_eq!(
        carried.claims[0].citations[0].status,
        CitationStatus::LinkedUnchanged
    );
    assert_eq!(
        carried.claims[1].citations[0].status,
        CitationStatus::CarriedUnchecked
    );
    assert_eq!(carried.state, ReportState::Problems);
    assert_eq!(carried.changed_claims, 1);
}

#[test]
fn a_new_version_carries_only_sources_usable_in_the_last_judged_revision() {
    // Given
    let mut first = ReportEvidence::default();
    read(&mut first, "notes.md", "1| v1");
    answer(&mut first, "Revenue grew [source: notes.md]");
    // The request later moved on to revision 3, which read nothing.
    first.verdict = Some(evaluate(&first, 3, &same));
    // When
    let second = ReportEvidence::next_version(&first, "method-request-1", "Write a report");
    // Then
    assert!(second.sources.is_empty(), "{:?}", second.sources);
}

#[test]
fn json_error_results_are_failed_reads() {
    // Given
    let url = "https://example.com/x";
    let error = format!(r#"{{"error":"HTTP status 404","url":"{url}"}}"#);
    let page = format!(r#"{{"url":"{url}","status":200,"text":"error: none"}}"#);
    // When
    let failed = source("call", SourceKind::Web, url, &error, None);
    let fetched = source("call", SourceKind::Web, url, &page, None);
    // Then
    assert_eq!(failed.status, SourceStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("HTTP status 404"));
    assert!(failed.content_digest.is_none());
    assert_eq!(fetched.status, SourceStatus::Read);
}

#[test]
fn long_locators_that_share_a_prefix_stay_distinct() {
    // Given
    let base = format!("https://example.com/{}", "a".repeat(300));
    let (read_url, other_url) = (format!("{base}/one"), format!("{base}/two"));
    let mut evidence = ReportEvidence::default();
    let page = source("call", SourceKind::Web, &read_url, "page", None);
    evidence.push_source(page, 1);
    answer(
        &mut evidence,
        &format!("Prices rose [source: {other_url}]\nPrices fell [source: {read_url}]"),
    );
    // When
    let verdict = evaluate(&evidence, 1, &same);
    // Then
    assert_eq!(
        verdict.claims[0].citations[0].status,
        CitationStatus::Unread
    );
    assert_eq!(
        verdict.claims[1].citations[0].status,
        CitationStatus::Linked
    );
}

#[test]
fn only_semicolons_separate_cited_sources() {
    // Given
    let url = "https://example.com/data?bbox=1,2,3,4";
    let text = format!(
        "Data held [source: data,2026.md]\nMap shown [source: {url}]\nBoth agree [source: data,2026.md; {url}]"
    );
    let mut evidence = ReportEvidence::default();
    read(&mut evidence, "data,2026.md", "1| data");
    evidence.push_source(source("call", SourceKind::Web, url, "page", None), 1);
    answer(&mut evidence, &text);
    // When
    let verdict = evaluate(&evidence, 1, &same);
    // Then
    let cited: Vec<Vec<&str>> = verdict
        .claims
        .iter()
        .map(|claim| claim.citations.iter().map(|c| c.locator.as_str()).collect())
        .collect();
    assert_eq!(
        cited,
        [vec!["data,2026.md"], vec![url], vec!["data,2026.md", url]]
    );
    assert_eq!(verdict.state, ReportState::Linked);
}
