use super::task_method_report::{
    Citation, CitationStatus, ReportEvidence, ReportSource, ReportState, ReportVerdict, SourceKind,
    SourceStatus, CLAIM_LIMIT,
};

#[cfg(test)]
#[path = "task_method_report_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freshness {
    Unchanged,
    Changed,
    Unknown,
}

#[cfg(test)]
pub(crate) fn evaluate(
    evidence: &ReportEvidence,
    revision: u64,
    fresh: &dyn Fn(&ReportSource) -> Freshness,
) -> ReportVerdict {
    evaluate_with(evidence, revision, fresh, &|_, _| None)
}

/// Like `evaluate`, with `results` judging, from a claim's text, its
/// citations of another part of a composed request (`[calculation]`);
/// unknown parts are missing.
pub(crate) fn evaluate_with(
    evidence: &ReportEvidence,
    revision: u64,
    fresh: &dyn Fn(&ReportSource) -> Freshness,
    results: &dyn Fn(&str, &str) -> Option<CitationStatus>,
) -> ReportVerdict {
    let current: Vec<&ReportSource> = evidence
        .sources
        .iter()
        .filter(|source| !source.carried && source.selection_revision == revision)
        .collect();
    let carried: Vec<&ReportSource> = evidence
        .sources
        .iter()
        .filter(|source| source.carried)
        .collect();
    let mut claims = Vec::new();
    let mut uncited_lines = 0;
    let mut uncited = Vec::new();
    let mut unchecked = evidence
        .dropped_outputs
        .iter()
        .filter(|dropped| **dropped == revision)
        .count();
    for output in evidence
        .outputs
        .iter()
        .filter(|output| output.selection_revision == revision)
    {
        uncited_lines += output.uncited_lines;
        unchecked += output.unchecked;
        claims.extend(output.claims.iter().cloned());
        uncited.extend(output.uncited.iter().cloned());
    }
    unchecked += claims.len().saturating_sub(CLAIM_LIMIT * 2);
    claims.truncate(CLAIM_LIMIT * 2);
    uncited.truncate(CLAIM_LIMIT);
    for claim in &mut claims {
        let text = claim.text.clone();
        for citation in &mut claim.citations {
            let (status, source) = if citation.key.starts_with("result:") {
                let status = results(&citation.key, &text).unwrap_or(CitationStatus::ResultMissing);
                (status, None)
            } else {
                resolve(citation, &current, &carried, fresh)
            };
            citation.status = status;
            citation.source = source;
        }
    }
    let written = claims.iter().any(|claim| claim.statement) || uncited_lines > 0 || unchecked > 0;
    let (changed_claims, removed_claims) = if evidence.version > 1 && written {
        let mut texts: Vec<&String> = Vec::new();
        let stated = claims.iter().filter(|claim| claim.statement);
        for text in stated.map(|claim| &claim.text).chain(&uncited) {
            if !texts.contains(&text) {
                texts.push(text);
            }
        }
        (
            texts
                .iter()
                .filter(|text| !evidence.previous_claims.contains(text))
                .count(),
            evidence
                .previous_claims
                .iter()
                .filter(|text| !texts.contains(text))
                .cloned()
                .collect(),
        )
    } else {
        (0, Vec::new())
    };
    let read = current
        .iter()
        .chain(&carried)
        .any(|source| source.status == SourceStatus::Read);
    let cited: Vec<_> = claims.iter().flat_map(|claim| &claim.citations).collect();
    let broken = cited
        .iter()
        .filter(|citation| !citation.status.linked())
        .count();
    let (state, reason) = if !written {
        (
            ReportState::NotWritten,
            "no report text has been produced for this method revision yet".to_owned(),
        )
    } else if !read
        && current
            .iter()
            .any(|source| source.status == SourceStatus::Failed)
    {
        (
            ReportState::Unavailable,
            "every source read failed, so no conclusion is backed by a read source".to_owned(),
        )
    } else if !read {
        (
            ReportState::NoSources,
            "no source was read, so the report is not source-grounded".to_owned(),
        )
    } else if cited.is_empty() {
        (
            ReportState::Uncited,
            "the report cites no source, so its conclusions are not linked to what was read"
                .to_owned(),
        )
    } else if broken > 0 {
        let unmarked = match uncited_lines {
            0 => String::new(),
            1 => "; 1 report line also carries no source marker".to_owned(),
            count => format!("; {count} report lines also carry no source marker"),
        };
        let backing = if cited
            .iter()
            .any(|citation| citation.key.starts_with("result:"))
        {
            "a current read of that source or a verified part of this request"
        } else {
            "a current read of that source"
        };
        (
            ReportState::Problems,
            format!(
                "{broken} of {} citations are not backed by {backing}{unmarked}{}",
                cited.len(),
                beyond_limits(unchecked)
            ),
        )
    } else if uncited_lines > 0 {
        let (lines, them) = if uncited_lines == 1 {
            ("line carries", "it is")
        } else {
            ("lines carry", "they are")
        };
        (
            ReportState::PartlyCited,
            format!("{uncited_lines} report {lines} no source marker, so {them} not source-checked; every citation links to a source read for this report{}", beyond_limits(unchecked)),
        )
    } else if unchecked > 0 {
        (
            ReportState::PartlyCited,
            format!(
                "every checked citation links to a source read for this report{}",
                beyond_limits(unchecked)
            ),
        )
    } else {
        (
            ReportState::Linked,
            "every citation links to a source read for this report; this checks linkage, not truth"
                .to_owned(),
        )
    };
    ReportVerdict {
        selection_revision: revision,
        state,
        reason,
        claims,
        uncited_lines,
        uncited,
        unchecked,
        changed_claims,
        removed_claims,
    }
}

fn beyond_limits(unchecked: usize) -> String {
    match unchecked {
        0 => String::new(),
        count => format!(
            "; {count} parts of the report were beyond the checking limits and were not checked"
        ),
    }
}

fn resolve(
    citation: &Citation,
    current: &[&ReportSource],
    carried: &[&ReportSource],
    fresh: &dyn Fn(&ReportSource) -> Freshness,
) -> (CitationStatus, Option<u64>) {
    let same = |source: &&&ReportSource| source.match_keys.contains(&citation.key);
    if let Some(source) = current.iter().rev().find(same) {
        let status = match (source.status, fresh(source)) {
            (SourceStatus::Failed, _) => CitationStatus::ReadFailed,
            (SourceStatus::Read, Freshness::Changed) => CitationStatus::Stale,
            (SourceStatus::Read, Freshness::Unchanged | Freshness::Unknown) => {
                CitationStatus::Linked
            }
        };
        return (status, Some(source.seq));
    }
    if let Some(source) = carried.iter().rev().find(same) {
        let status = match (source.kind, fresh(source)) {
            (SourceKind::File, Freshness::Unchanged) => CitationStatus::LinkedUnchanged,
            (SourceKind::File, Freshness::Changed) => CitationStatus::Stale,
            (SourceKind::File | SourceKind::Web, _) => CitationStatus::CarriedUnchecked,
        };
        return (status, Some(source.seq));
    }
    (CitationStatus::Unread, None)
}
