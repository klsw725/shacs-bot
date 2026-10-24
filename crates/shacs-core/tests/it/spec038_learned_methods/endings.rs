use super::checks::{run_candidate, REQUEST};
use super::*;
use shacs_config::WorkspacePermissionId;
use shacs_session::SessionManager;

#[test]
fn a_request_that_ends_without_an_answer_still_says_which_method_it_used(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let writes = Arc::new(AtomicUsize::new(0));
    let tools = tools(&writes);
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        call("write_file", json!({"path": "summary.txt", "content": "x"})),
        fitting("method-1"),
        text(FAILED),
    ]);
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    say(&mut agent, "Write the summary to summary.txt")?;
    // When
    let denied = say(&mut agent, "2")?;
    let failed = say(
        &mut agent,
        "Summarize the changelog in downloads/changes.md",
    )?;
    // Then
    assert!(
        denied.starts_with("Tool execution cancelled.\n\n—\nMethod used: method-1 v1"),
        "{denied}"
    );
    assert!(
        failed.contains("\n\n—\nMethod used: method-1 v1"),
        "{failed}"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let ended: Vec<(&Value, &Value)> = session
        .messages
        .iter()
        .filter_map(|message| {
            message
                .get("learned_method")
                .map(|record| (&message["content"], &record["ended"]))
        })
        .collect();
    assert_eq!(
        ended,
        [
            (&json!(denied), &json!("permission_denied_by_user")),
            (&json!(failed), &json!("provider_error")),
        ]
    );
    assert!(!session.metadata.contains_key("learned_method_turn"));
    Ok(())
}

#[test]
fn a_request_ended_by_a_failed_project_permission_save_says_which_method_it_used(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let writes = Arc::new(AtomicUsize::new(0));
    let tools = tools(&writes);
    let refused = "Project permission could not be saved; the action was not run.";
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        LlmResponse {
            content: Some(refused.to_owned()),
            ..call("write_file", json!({"path": "summary.txt", "content": "x"}))
        },
    ]);
    let provider = Provider::new(responses);
    let store_path = dir.path().join("data/permissions.json");
    let mut config = AgentLoopConfig::new(dir.path(), "test-model");
    config.permission_interactive = true;
    config.project_permission_store = Some(ProjectPermissionStoreConfig {
        store_path: store_path.clone(),
        workspace_id: WorkspacePermissionId::from_canonical_workspace_path(
            dir.path().canonicalize()?.to_string_lossy().as_ref(),
        ),
    });
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        config,
    );
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    say(&mut agent, "Write the summary to summary.txt")?;
    std::fs::create_dir_all(&store_path)?;
    // When
    let blocked = say(&mut agent, "4")?;
    // Then
    assert!(
        blocked.starts_with(
            "Project permission could not be saved; the action was not run.\n\n—\nMethod used: method-1 v1"
        ),
        "{blocked}"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let recorded: Vec<&Value> = session
        .messages
        .iter()
        .filter(|message| message.get("learned_method").is_some())
        .collect();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(
        recorded[0]["learned_method"]["ended"],
        "permission_project_store_unavailable"
    );
    assert!(
        recorded[0].get("tool_calls").is_none()
            && recorded[0]["content"].as_str().is_some_and(
                |content| content.starts_with(refused) && content.len() > refused.len()
            ),
        "{recorded:?}"
    );
    assert!(!session.metadata.contains_key("learned_method_turn"));
    Ok(())
}

#[test]
fn a_request_ended_by_a_failed_approved_tool_keeps_one_answer_for_it() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        call("write_file", json!({"path": "full.txt", "content": "x"})),
    ]);
    let provider = Provider::new(responses);
    let mut config = AgentLoopConfig::new(dir.path(), "test-model");
    config.permission_interactive = true;
    config.fail_on_tool_error = true;
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        config,
    );
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    say(&mut agent, "Write the summary to full.txt")?;
    // When
    let failed = say(&mut agent, "1")?;
    // Then
    assert!(
        failed.contains("disk full") && failed.contains("\n\n—\nMethod used: method-1 v1"),
        "{failed}"
    );
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let answers: Vec<&Value> = session
        .messages
        .iter()
        .filter(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("disk full"))
        })
        .collect();
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0]["learned_method"]["ended"], "tool_error");
    Ok(())
}
