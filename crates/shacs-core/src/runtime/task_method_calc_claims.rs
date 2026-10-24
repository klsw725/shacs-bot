use super::task_method::{ClaimState, ValidationClaim};
use super::task_method_calc::{CalcState, CalcVerdict, CalculationEvidence};

pub(crate) fn claims(
    verdict: &CalcVerdict,
    evidence: &CalculationEvidence,
) -> Vec<ValidationClaim> {
    let primary = verdict
        .primary
        .and_then(|seq| evidence.find(seq))
        .and_then(|run| run.result.as_ref());
    let reference = |seq: &u64| format!("run #{seq}");
    let primary_ref: Vec<String> = verdict.primary.iter().map(reference).collect();
    let all_refs: Vec<String> = verdict
        .primary
        .iter()
        .chain(&verdict.checks)
        .map(reference)
        .collect();
    let recorded = |present: bool| {
        if present {
            primary_ref.clone()
        } else {
            Vec::new()
        }
    };
    let unit_state = match verdict.state {
        CalcState::Verified => ClaimState::Verified,
        CalcState::Conflict if verdict.reason.starts_with("unit mismatch") => ClaimState::Failed,
        _ => ClaimState::NotVerified,
    };
    let check_state = match verdict.state {
        CalcState::Verified => ClaimState::Verified,
        CalcState::Conflict => ClaimState::Failed,
        CalcState::NotRun
        | CalcState::Unavailable
        | CalcState::Failed
        | CalcState::Unrecorded
        | CalcState::Unverified => ClaimState::NotVerified,
    };
    vec![
        claim(
            "inputs",
            ClaimState::NotVerified,
            recorded(primary.is_some_and(|result| !result.inputs.is_empty())),
        ),
        claim(
            "units",
            unit_state,
            if unit_state == ClaimState::NotVerified {
                recorded(primary.is_some_and(|result| !result.unit.is_empty()))
            } else {
                all_refs.clone()
            },
        ),
        claim(
            "assumptions",
            ClaimState::NotVerified,
            recorded(primary.is_some_and(|result| !result.assumptions.is_empty())),
        ),
        claim(
            "independent check",
            check_state,
            if check_state == ClaimState::NotVerified {
                Vec::new()
            } else {
                all_refs
            },
        ),
    ]
}

fn claim(check: &str, state: ClaimState, evidence_refs: Vec<String>) -> ValidationClaim {
    ValidationClaim {
        check: check.to_owned(),
        state,
        evidence_refs,
    }
}
