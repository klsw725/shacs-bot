use super::checks::{run_candidate, REQUEST};
use super::*;
use shacs_session::SessionManager;

/// A workflow admitted from the message's metadata: one child and its
/// verifier, as Spec024's runtime tests run it.
fn workflow_message(content: &str) -> Result<InboundMessage, Box<dyn Error>> {
    let admission = WorkflowAdmissionInput {
        objective_complexity: 8,
        estimated_item_count: 8,
        requires_parallelism: true,
        requires_independent_verification: true,
        requires_adversarial_review: false,
        requires_large_context_partitioning: false,
        requires_write_isolation: false,
        requires_recurring_loop: false,
        risk_level: 4,
        user_requested_workflow: true,
        available_budget_tokens: Some(10_000),
        blocking_reasons: Vec::new(),
        missing_scope_questions: Vec::new(),
    };
    let plan = json!({
        "workflow_id": "workflow-1", "origin_session_id": "chat:room", "origin_turn_id": "turn-1",
        "objective": "verify every claim", "constraints": [], "pattern": "fan_out_and_synthesize",
        "steps": [{"step_id": "extract-claims", "label": "Extract claims", "pattern": "classify_and_act",
                   "depends_on": [], "required": true, "expected_output_schema": {"type": "object"}}],
        "child_graph": [{"child_id": "child-1", "step_id": "extract-claims", "goal": "extract factual claims",
                         "tool_scope_ref": "scope-1", "worktree_policy": "read_only_snapshot",
                         "budget": {"max_tokens": 1000, "max_wall_clock_ms": 30000}, "verifier_required": true}],
        "verifier_graph": [{"verifier_id": "verifier-1", "target_child_id": "child-1",
                            "rubric": "claims have source evidence", "independent_evidence_required": false}],
        "context_policy": {"root_objective_snapshot": "verify every claim",
                           "include_constraints_in_children": true, "untrusted_input_labels": []},
        "tool_scope_policy": {"scope_digest": "scope-digest", "allowed_tools": ["read_file"],
                              "deferred_tool_search_allowed": false, "quarantine": "read_only_untrusted"},
        "permission_policy": {"permission_snapshot_ref": "permission-snapshot",
                              "denied_capabilities": ["proc_exec"], "approval_required_for_privileged_steps": true},
        "worktree_policy": "read_only_snapshot",
        "model_routing_policy": {"classifier_model_hint": null, "child_model_hint": null,
                                 "verifier_model_hint": null, "synthesis_model_hint": null,
                                 "fallback_model_policy": "use provider default"},
        "budget_policy": {"max_total_tokens": 10000, "max_child_tokens": 2000, "max_verifier_tokens": 2000,
                          "max_iterations": 4, "max_parallel_children": 1, "max_wall_clock_ms": 120000,
                          "max_heavy_commands": 0},
        "checkpoint_policy": {"checkpoint_required": true, "checkpoint_before_privileged_steps": true},
        "merge_policy": {"require_verifier_pass": true, "allow_partial_completion": false,
                         "surface_disagreements": true},
        "stop_condition": {"description": "all claims verified", "no_new_findings_threshold": null},
        "resume_policy": {"require_plan_digest_match": true, "allow_completed_resume": false}
    });
    let mut message = InboundMessage::new("chat", "alice", "room", content);
    message.metadata.insert(
        "workflow_admission".to_owned(),
        serde_json::to_value(admission)?,
    );
    message.metadata.insert("workflow_plan".to_owned(), plan);
    Ok(message)
}

#[test]
fn a_new_workflow_uses_the_confirmed_method_in_each_child_and_says_so() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let mut responses = run_candidate();
    responses.extend([fitting("method-1"), text("claims extracted"), text("pass")]);
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    // When
    let result = agent.process_message(workflow_message(
        "Verify every claim in the release notes in downloads/notes.md",
    )?)?;
    // Then
    let reply = result.final_content.unwrap_or_default();
    assert_eq!(result.stop_reason, "workflow_completed");
    assert!(
        reply.contains("Workflow completed") && reply.contains("\n\n—\nMethod used: method-1 v1"),
        "{reply}"
    );
    let requests = provider.requests();
    let children = &requests[requests.len() - 2..];
    for child in children {
        let prompt = system_prompt(child);
        assert!(
            prompt.contains("use learned method method-1 v1 as a guide"),
            "{prompt}"
        );
    }
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let record = session
        .messages
        .iter()
        .find_map(|message| message.get("learned_method"))
        .ok_or("a record of the method used")?;
    assert_eq!(record["version"], 1);
    assert_eq!(record["ended"], "workflow_completed");
    assert!(!session.metadata.contains_key("learned_method_turn"));
    Ok(())
}
