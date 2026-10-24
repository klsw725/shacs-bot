use super::task_method::{ClaimState, ValidationClaim};
use super::task_method_report::{CitationStatus, ReportState, ReportVerdict};

pub(crate) fn report_claims(verdict: &ReportVerdict) -> Vec<ValidationClaim> {
    let cited: Vec<_> = verdict
        .claims
        .iter()
        .flat_map(|claim| &claim.citations)
        .collect();
    let refs = |filter: &dyn Fn(CitationStatus) -> bool| -> Vec<String> {
        let mut refs: Vec<String> = cited
            .iter()
            .filter(|citation| filter(citation.status))
            .map(|citation| format!("source {}", citation.locator))
            .collect();
        refs.sort();
        refs.dedup();
        refs.truncate(32);
        refs
    };
    let unread = |status: CitationStatus| {
        matches!(
            status,
            CitationStatus::Unread | CitationStatus::ReadFailed | CitationStatus::ResultMissing
        )
    };
    let read_state = if cited.iter().any(|citation| unread(citation.status)) {
        ClaimState::Failed
    } else if !cited.is_empty() && verdict.state != ReportState::NoSources {
        ClaimState::Verified
    } else {
        ClaimState::NotVerified
    };
    let link_state = match verdict.state {
        ReportState::Linked => ClaimState::Verified,
        ReportState::Problems => ClaimState::Failed,
        _ => ClaimState::NotVerified,
    };
    let mut unlinked = refs(&|status| !status.linked());
    unlinked.extend(
        verdict
            .uncited
            .iter()
            .map(|line| format!("no source marker: {line}")),
    );
    if verdict.unchecked > 0 {
        unlinked.push(format!(
            "not checked (beyond the checking limits): {} parts",
            verdict.unchecked
        ));
    }
    unlinked.truncate(32);
    let flagged: Vec<String> = verdict
        .claims
        .iter()
        .filter(|claim| claim.unresolved || !claim.conflict.is_empty())
        .map(|claim| claim.text.clone())
        .take(32)
        .collect();
    vec![
        ValidationClaim {
            check: "sources actually read".to_owned(),
            state: read_state,
            evidence_refs: if read_state == ClaimState::Failed {
                refs(&unread)
            } else {
                refs(&|status| status.linked())
            },
        },
        ValidationClaim {
            check: "claim-source linkage".to_owned(),
            state: link_state,
            evidence_refs: unlinked,
        },
        ValidationClaim {
            check: "unresolved claims".to_owned(),
            state: ClaimState::NotVerified,
            evidence_refs: flagged,
        },
    ]
}
