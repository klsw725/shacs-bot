use super::*;

fn change(
    workspace: &Path,
    work: &EntrustedWorkSnapshot,
    operation: &str,
) -> Result<(), Box<dyn Error>> {
    match operation {
        "revision" => {
            revise_entrusted_work(
                workspace,
                EntrustedWorkReviseRequest {
                    owner_session_id: "chat:room".to_owned(),
                    requester_target: "chat:room".to_owned(),
                    work_id: work.work.work_id.clone(),
                    expected_revision: 1,
                    conditions: EntrustedWorkConditions {
                        instructions: "now below 50".to_owned(),
                        budget: None,
                        prior_decisions: Vec::new(),
                        remaining_questions: Vec::new(),
                    },
                    linked_targets: None,
                    changed_at: "2026-10-19T01:00:00Z".to_owned(),
                },
            )?;
        }
        "pause" | "done" | "clear" | "resume" => {
            let action = match operation {
                "done" => EntrustedWorkLifecycleAction::Done,
                "clear" => EntrustedWorkLifecycleAction::Clear,
                _ => EntrustedWorkLifecycleAction::Pause,
            };
            let request = EntrustedWorkLifecycleRequest {
                owner_session_id: "chat:room".to_owned(),
                requester_target: "chat:room".to_owned(),
                work_id: work.work.work_id.clone(),
                action,
                observed_at: "2026-10-19T01:00:00Z".to_owned(),
            };
            transition_entrusted_work(workspace, request.clone())?;
            if operation == "resume" {
                transition_entrusted_work(
                    workspace,
                    EntrustedWorkLifecycleRequest {
                        action: EntrustedWorkLifecycleAction::Resume,
                        observed_at: "2026-10-19T02:00:00Z".to_owned(),
                        ..request
                    },
                )?;
            }
        }
        "goal" => {
            apply_goal_surface_action(
                workspace,
                "chat:room",
                GoalSurfaceAction::Clear,
                "2026-10-19T01:00:00Z",
            )?;
        }
        _ => return Err("unknown test operation".into()),
    }
    Ok(())
}

#[test]
fn stale_revision_or_goal_closes_pending_and_accepted_continuations() -> Result<(), Box<dyn Error>>
{
    for operation in ["revision", "pause", "done", "clear", "resume", "goal"] {
        for accepted in [false, true] {
            let workspace = tempfile::tempdir()?;
            let work = setup(workspace.path())?;
            let counter = Arc::new(AtomicUsize::new(0));
            let tools = registry(&counter);
            let provider = Provider::new(vec![
                response(vec![proposal(&work.work.work_id)]),
                response(vec![send_call()]),
            ]);
            let mut agent = runtime(workspace.path(), &tools, &provider)?;
            let id = proposal_id(&agent.process_message(inbound("suggest action"))?)?;
            if accepted {
                agent.process_message(inbound(&format!("/action accept {id}")))?;
            }
            change(workspace.path(), &work, operation)?;
            let calls_before = provider.calls();

            let result = agent.process_message(inbound(&if accepted {
                "approve".to_owned()
            } else {
                format!("/action accept {id}")
            }))?;

            assert_eq!(
                result.stop_reason, "entrusted_action_closed",
                "{operation}, accepted={accepted}"
            );
            assert_eq!(provider.calls(), calls_before);
            assert_eq!(counter.load(Ordering::SeqCst), 0);
            let saved = agent
                .session_manager()
                .read_session_file("chat:room")
                .ok_or("missing session")?;
            assert!(saved["metadata"]
                .get("pending_permission_approval")
                .is_none());
        }
    }
    Ok(())
}

#[test]
fn process_restart_closes_pending_and_accepted_decisions() -> Result<(), Box<dyn Error>> {
    for accepted in [false, true] {
        let workspace = tempfile::tempdir()?;
        let work = setup(workspace.path())?;
        let counter = Arc::new(AtomicUsize::new(0));
        let tools = registry(&counter);
        let provider = Provider::new(vec![
            response(vec![proposal(&work.work.work_id)]),
            response(vec![send_call()]),
        ]);
        let mut agent = runtime(workspace.path(), &tools, &provider)?;
        let id = proposal_id(&agent.process_message(inbound("suggest action"))?)?;
        if accepted {
            agent.process_message(inbound(&format!("/action accept {id}")))?;
        }
        drop(agent);

        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "lineage::restart_probe",
                "--ignored",
                "--nocapture",
            ])
            .env("SHACS_ACTION_QA_WORKSPACE", workspace.path())
            .env(
                "SHACS_ACTION_QA_COMMAND",
                if accepted {
                    "approve".to_owned()
                } else {
                    format!("/action accept {id}")
                },
            )
            .output()?;

        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!(
            "RESTART accepted={accepted}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}

#[test]
#[ignore = "subprocess probe invoked by process_restart_closes_pending_and_accepted_decisions"]
fn restart_probe() -> Result<(), Box<dyn Error>> {
    let workspace = std::env::var("SHACS_ACTION_QA_WORKSPACE")?;
    let command = std::env::var("SHACS_ACTION_QA_COMMAND")?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(Path::new(&workspace), &tools, &provider)?;

    let result = agent.process_message(inbound(&command))?;

    println!(
        "USER: {command}\nASSISTANT: {}\nSIDE_EFFECTS: {}",
        result.final_content.as_deref().unwrap_or_default(),
        counter.load(Ordering::SeqCst)
    );
    assert_eq!(result.stop_reason, "entrusted_action_closed");
    assert_eq!(provider.calls(), 0);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn stop_cancels_the_action_before_an_acceptance_can_resume() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(vec![response(vec![proposal(&work.work.work_id)])]);
    let mut agent = runtime(workspace.path(), &tools, &provider)?;
    let id = proposal_id(&agent.process_message(inbound("suggest action"))?)?;

    agent.process_message(inbound("/stop"))?;
    let result = agent.process_message(inbound(&format!("/action accept {id}")))?;

    assert_eq!(result.stop_reason, "entrusted_action_rejected");
    assert_eq!(provider.calls(), 1);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    Ok(())
}
