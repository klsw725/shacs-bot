use shacs_app::app::AppId;
use shacs_app::app_lifecycle::{
    AppLifecycleAction, AppLifecycleBlocker, AppLifecycleReceipt, AppProcessState,
    AppSupervisorJournal,
};
use std::error::Error;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn lifecycle_journal_records_requested_and_completed_transitions() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let app_id = AppId::parse("runner.app")?;

    let requested = journal.request(&app_id, AppLifecycleAction::Start)?;
    assert_eq!(requested.previous_state, AppProcessState::Installed);
    assert_eq!(requested.current_state, AppProcessState::Starting);
    assert!(!requested.completed);

    let running = journal.complete(
        &requested,
        AppProcessState::Running,
        "sha256:manifest",
        "trusted-runtime:local",
        Vec::new(),
    )?;
    assert_eq!(running.previous_state, AppProcessState::Starting);
    assert_eq!(running.current_state, AppProcessState::Running);
    assert!(running.completed);
    assert_eq!(journal.inspect(&app_id)?.state, AppProcessState::Running);

    let encoded = serde_json::to_string(&running)?;
    assert!(!encoded.contains("processEnv"));
    assert!(!encoded.contains("argv"));
    Ok(())
}

#[test]
fn blocked_start_is_terminal_without_creating_running_truth() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let app_id = AppId::parse("blocked.app")?;
    let requested = journal.request(&app_id, AppLifecycleAction::Start)?;

    let blocked = journal.block(
        &requested,
        vec![AppLifecycleBlocker::CredentialMissing {
            name: "OPENAI_API_KEY".to_owned(),
        }],
    )?;

    assert_eq!(blocked.current_state, AppProcessState::Failed);
    assert!(blocked.completed);
    assert_eq!(journal.inspect(&app_id)?.state, AppProcessState::Failed);
    assert_eq!(blocked.blockers.len(), 1);
    Ok(())
}

#[test]
fn replay_reads_receipts_without_dispatching_actions() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let app_id = AppId::parse("replay.app")?;
    let requested = journal.request(&app_id, AppLifecycleAction::Recover)?;
    journal.complete(
        &requested,
        AppProcessState::Stopped,
        "sha256:manifest",
        "trusted-runtime:local",
        Vec::new(),
    )?;

    let replay = journal.replay(&app_id)?;
    assert_eq!(replay.dispatch_count, 0);
    assert_eq!(replay.receipts.len(), 2);
    assert!(replay
        .receipts
        .iter()
        .all(|receipt: &AppLifecycleReceipt| receipt.app_id == app_id));
    Ok(())
}

#[test]
fn concurrent_start_requests_are_fenced_to_one_owner() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let root = root.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let journal = AppSupervisorJournal::new(root);
                let app_id = AppId::parse("fenced.app").map_err(|error| error.to_string())?;
                barrier.wait();
                journal
                    .request(&app_id, AppLifecycleAction::Start)
                    .map_err(|error| error.to_string())
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().map_err(|_| "thread panicked"))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    Ok(())
}

#[test]
fn duplicate_completion_is_idempotent() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let app_id = AppId::parse("idempotent.app")?;
    let requested = journal.request(&app_id, AppLifecycleAction::Start)?;
    let first = journal.complete(
        &requested,
        AppProcessState::Running,
        "digest",
        "runtime",
        Vec::new(),
    )?;
    let second = journal.complete(
        &requested,
        AppProcessState::Running,
        "digest",
        "runtime",
        Vec::new(),
    )?;
    assert_eq!(first.receipt_id, second.receipt_id);
    Ok(())
}
