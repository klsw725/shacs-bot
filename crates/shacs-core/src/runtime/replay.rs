use shacs_eval::evaluator::{
    compare_replay_dataset_item, AuxiliaryJudgeRoute, EvidenceKind, EvidenceRef, RedactionStatus,
    ReplayCaseResult, ReplayComparisonSeverity, ReplayComparisonStatus, ReplayDatasetItem,
    ReplayRunRecord, ReplayRunStatus, ReplayToolOutcomePolicy,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeReplayInput<'a> {
    pub run_id: String,
    pub dataset_id: String,
    pub dataset: &'a [ReplayDatasetItem],
    pub selected_case_ids: &'a [String],
    pub started_at_ms: u64,
    pub completed_at_ms: u64,
    pub diagnostics_ref: EvidenceRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeReplayOutcome {
    pub run_record: ReplayRunRecord,
    pub replayed_tool_policy_count: usize,
    pub live_tool_dispatch_count: usize,
    pub auxiliary_judge_routes: Vec<AuxiliaryJudgeRoute>,
}

pub fn run_local_replay(input: RuntimeReplayInput<'_>) -> RuntimeReplayOutcome {
    let selected_items: Vec<&ReplayDatasetItem> = input
        .dataset
        .iter()
        .filter(|item| {
            input
                .selected_case_ids
                .iter()
                .any(|case_id| case_id == &item.case_id)
        })
        .collect();
    let selected_dataset_ids: BTreeSet<&str> = selected_items
        .iter()
        .map(|item| item.case_id.as_str())
        .collect();
    let mut case_results: Vec<ReplayCaseResult> = selected_items
        .iter()
        .map(|item| replay_case(item))
        .collect();
    if input.selected_case_ids.is_empty() {
        case_results.push(blocked_selection_result(
            "__selection__",
            "blocked_empty_replay_selection",
            input.diagnostics_ref.clone(),
        ));
    } else {
        case_results.extend(
            input
                .selected_case_ids
                .iter()
                .filter(|case_id| !selected_dataset_ids.contains(case_id.as_str()))
                .map(|case_id| {
                    blocked_selection_result(
                        case_id,
                        "blocked_unknown_selected_replay_case",
                        input.diagnostics_ref.clone(),
                    )
                }),
        );
    }
    let status = replay_run_status(&case_results);
    let replayed_tool_policy_count = selected_items
        .iter()
        .map(|item| item.tool_outcome_policies.len())
        .sum();
    let auxiliary_judge_routes = selected_items
        .iter()
        .flat_map(|item| item.auxiliary_judge_routes.clone())
        .collect();

    RuntimeReplayOutcome {
        run_record: ReplayRunRecord {
            run_id: input.run_id,
            dataset_id: input.dataset_id,
            selected_cases: input.selected_case_ids.to_vec(),
            started_at_ms: input.started_at_ms,
            completed_at_ms: input.completed_at_ms,
            status,
            diagnostics_ref: input.diagnostics_ref,
            case_results,
        },
        replayed_tool_policy_count,
        live_tool_dispatch_count: 0,
        auxiliary_judge_routes,
    }
}

fn replay_case(item: &ReplayDatasetItem) -> ReplayCaseResult {
    if let Some(route) = item
        .auxiliary_judge_routes
        .iter()
        .find(|route| !item.allowed_judge_roles.contains(&route.judge_role))
    {
        return ReplayCaseResult {
            case_id: item.case_id.clone(),
            actual_verdict: item.actual_verdict.clone(),
            comparison_status: ReplayComparisonStatus::BlockedMissingReplayOutcome,
            severity: ReplayComparisonSeverity::Blocked,
            diff_summary: format!("disallowed auxiliary judge role: {:?}", route.judge_role),
            judge_route_refs: judge_route_refs(&item.auxiliary_judge_routes),
            blocked_reason: Some("blocked_disallowed_judge_role".to_owned()),
            diagnostics_refs: item.diagnostics_refs.clone(),
            coverage_refs: item.coverage_refs.clone(),
        };
    }

    if let Some((status, reason)) = first_blocking_tool_policy(&item.tool_outcome_policies) {
        return ReplayCaseResult {
            case_id: item.case_id.clone(),
            actual_verdict: item.actual_verdict.clone(),
            comparison_status: status,
            severity: ReplayComparisonSeverity::Blocked,
            diff_summary: reason.clone(),
            judge_route_refs: judge_route_refs(&item.auxiliary_judge_routes),
            blocked_reason: Some(reason),
            diagnostics_refs: item.diagnostics_refs.clone(),
            coverage_refs: item.coverage_refs.clone(),
        };
    }

    compare_replay_dataset_item(item)
}

fn blocked_selection_result(
    case_id: impl Into<String>,
    reason: impl Into<String>,
    diagnostics_ref: EvidenceRef,
) -> ReplayCaseResult {
    let reason = reason.into();
    ReplayCaseResult {
        case_id: case_id.into(),
        actual_verdict: None,
        comparison_status: ReplayComparisonStatus::BlockedMissingReplayOutcome,
        severity: ReplayComparisonSeverity::Blocked,
        diff_summary: reason.clone(),
        judge_route_refs: Vec::new(),
        blocked_reason: Some(reason),
        diagnostics_refs: vec![diagnostics_ref],
        coverage_refs: Vec::new(),
    }
}

fn first_blocking_tool_policy(
    policies: &[ReplayToolOutcomePolicy],
) -> Option<(ReplayComparisonStatus, String)> {
    policies.iter().find_map(|policy| {
        if policy.recorded_outcome_ref.is_some() {
            return None;
        }

        let Some(safe_mock) = &policy.safe_mock_outcome else {
            return Some((
                ReplayComparisonStatus::BlockedMissingReplayOutcome,
                "blocked_missing_replay_outcome".to_owned(),
            ));
        };

        if safe_mock.expected_schema_digest != policy.expected_schema_digest {
            return Some((
                ReplayComparisonStatus::SchemaMismatch,
                "safe mock schema digest mismatch".to_owned(),
            ));
        }

        None
    })
}

fn replay_run_status(case_results: &[ReplayCaseResult]) -> ReplayRunStatus {
    if case_results.iter().any(|result| {
        matches!(
            result.comparison_status,
            ReplayComparisonStatus::BlockedMissingReplayOutcome
                | ReplayComparisonStatus::SchemaMismatch
        )
    }) {
        return ReplayRunStatus::Blocked;
    }

    if case_results
        .iter()
        .any(|result| result.comparison_status != ReplayComparisonStatus::Match)
    {
        return ReplayRunStatus::Failed;
    }

    ReplayRunStatus::Passed
}

fn judge_route_refs(routes: &[AuxiliaryJudgeRoute]) -> Vec<EvidenceRef> {
    routes
        .iter()
        .map(|route| EvidenceRef {
            kind: EvidenceKind::JudgeRoutingDecision,
            id: route.route_id.clone(),
            digest: route.provider_snapshot.snapshot_id.clone(),
            summary: route.routing_reason.clone(),
            redaction_status: RedactionStatus::Redacted,
            owner_spec: Some("018".to_owned()),
            locator: Some(format!("replay://judge-route/{}", route.route_id)),
            retention_hint: Some("local".to_owned()),
        })
        .collect()
}
