use shacs_core::runtime::{
    decide_workflow_admission, WorkflowAdmissionDecision, WorkflowAdmissionInput, WorkflowRunState,
};

#[test]
fn workflow_symbols_remain_available_through_runtime_reexport() {
    assert!(WorkflowRunState::Completed.is_terminal());

    let decision = decide_workflow_admission(&WorkflowAdmissionInput {
        objective_complexity: 1,
        estimated_item_count: 1,
        requires_parallelism: false,
        requires_independent_verification: false,
        requires_adversarial_review: false,
        requires_large_context_partitioning: false,
        requires_write_isolation: false,
        requires_recurring_loop: false,
        risk_level: 1,
        user_requested_workflow: false,
        available_budget_tokens: Some(1_000),
        blocking_reasons: Vec::new(),
        missing_scope_questions: Vec::new(),
    });

    assert_eq!(decision, WorkflowAdmissionDecision::UseRegularLoop);
}
