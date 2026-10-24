use super::checks::{run_candidate, REQUEST};
use super::*;
use shacs_core::tools::AskUserTool;
use shacs_session::SessionManager;

/// An agent with method-1 learned in chat `room` and confirmed.
fn confirmed<'a>(
    dir: &Path,
    tools: &'a ToolRegistry,
    provider: &'a Provider,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut agent = agent(dir, tools, provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    Ok(agent)
}

fn asking_tools() -> ToolRegistry {
    let mut tools = tools(&Arc::new(AtomicUsize::new(0)));
    tools.register(AskUserTool::new());
    tools
}

fn records(dir: &Path) -> Result<(Vec<Value>, bool), Box<dyn Error>> {
    let session = SessionManager::new(dir)?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let records = session
        .messages
        .iter()
        .filter_map(|message| message.get("learned_method").cloned())
        .collect();
    Ok((
        records,
        session.metadata.contains_key("learned_method_turn"),
    ))
}

#[test]
fn stopping_a_pending_action_decision_ends_the_request_and_says_which_method_it_used(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut manager = SessionManager::new(dir.path())?;
    let room = manager.get_or_create("chat:room");
    manager.save(&room)?;
    let work = create_entrusted_work(
        dir.path(),
        EntrustedWorkCreateRequest {
            owner_session_id: "chat:room".to_owned(),
            goal_text: "monitor offer".to_owned(),
            turn_budget: 100,
            consent: EntrustedWorkConsentProof {
                kind: EntrustedWorkConsentKind::ExplicitRequest,
                evidence_ref: "user:request".to_owned(),
                recorded_at: "2026-10-19T00:00:00Z".to_owned(),
            },
            conditions: EntrustedWorkConditions {
                instructions: "notify below 100".to_owned(),
                budget: None,
                prior_decisions: Vec::new(),
                remaining_questions: Vec::new(),
            },
            linked_targets: Vec::new(),
            observed_at: "2026-10-19T00:00:00Z".to_owned(),
        },
    )?;
    let tools = asking_tools();
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        call(
            "ask_user",
            json!({"question": "Send the prepared request?",
                   "external_action": {"work_id": work.work.work_id, "category": "send",
                                       "summary": "Send the prepared inquiry", "reference": "draft:inquiry-1"}}),
        ),
    ]);
    let provider = Provider::new(responses);
    let mut agent = confirmed(dir.path(), &tools, &provider)?;
    let asked = say(
        &mut agent,
        "Summarize the changelog in downloads/changes.md",
    )?;
    // When
    let stopped = say(&mut agent, "/stop")?;
    // Then
    assert!(
        asked.contains("/action accept") && !asked.contains("Method used"),
        "{asked}"
    );
    assert!(
        stopped.starts_with("Stop requested.")
            && stopped.contains("\n\n—\nMethod used: method-1 v1"),
        "{stopped}"
    );
    let (records, lingering) = records(dir.path())?;
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["ended"], "stop_requested");
    assert!(!lingering);
    Ok(())
}

#[test]
fn a_command_that_ends_something_else_leaves_the_request_waiting_for_its_answer(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = asking_tools();
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        call("ask_user", json!({"question": "Which changelog?"})),
        text("Done."),
    ]);
    let provider = Provider::new(responses);
    let mut agent = confirmed(dir.path(), &tools, &provider)?;
    say(&mut agent, "Summarize the changelog")?;
    let mut sessions = SessionManager::new(dir.path())?;
    let mut session = sessions.load_existing("chat:room").ok_or("saved session")?;
    let denial = RecentAutoModeDenial {
        denial_id: "auto_denial_earlier".to_owned(),
        created_at_unix_ms: 1,
        session_digest: "session-digest".to_owned(),
        turn_digest: "turn-digest".to_owned(),
        tool_name: "exec".to_owned(),
        capabilities: Vec::new(),
        target_summary: vec!["target:test".to_owned()],
        action_digest: "action".to_owned(),
        argument_digest: "argument".to_owned(),
        snapshot_digest: "snapshot".to_owned(),
        policy_safety_snapshot_ref: None,
        secret_ref_evidence: Vec::new(),
        decision_reason: PermissionPolicyReason::EvaluatorUncertain,
        classifier_verdict: AutoEvaluatorVerdictKind::DenyCandidate,
        classifier_confidence: EvaluatorConfidence::High,
        classifier_scope_match: EvaluatorScopeMatch::Requested,
        retryable: true,
    };
    session.metadata.insert(
        "recent_auto_mode_denials".to_owned(),
        serde_json::to_value(vec![denial])?,
    );
    sessions.save(&session)?;
    // When
    let closed = say(&mut agent, "/permission recent retry auto_denial_earlier")?;
    let answered = say(&mut agent, "The one in downloads/changes.md")?;
    let resumed = system_prompt(&provider.last());
    // Then
    assert!(
        closed.contains("not run") && !closed.contains("Method used"),
        "{closed}"
    );
    assert!(
        answered.starts_with("Done.\n\n—\nMethod used: method-1 v1")
            && resumed.contains("use learned method method-1 v1 as a guide"),
        "{answered}\n{resumed}"
    );
    let (records, lingering) = records(dir.path())?;
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["ended"], "completed");
    assert!(!lingering);
    Ok(())
}

#[test]
fn a_new_session_drops_the_request_waiting_with_the_conversation() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = asking_tools();
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        call("write_file", json!({"path": "summary.txt", "content": "x"})),
    ]);
    let provider = Provider::new(responses);
    let mut agent = confirmed(dir.path(), &tools, &provider)?;
    say(&mut agent, "Write the summary to summary.txt")?;
    // When
    let started = say(&mut agent, "/new")?;
    let (_, lingering) = records(dir.path())?;
    let listed = say(&mut agent, "/learn list")?;
    // Then
    assert_eq!(started, "Started a new session.");
    assert!(!lingering);
    assert!(!listed.contains("Method used"), "{listed}");
    Ok(())
}
