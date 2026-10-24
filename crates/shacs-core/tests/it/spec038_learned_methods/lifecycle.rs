use super::checks::{run_candidate, REQUEST};
use super::*;
use shacs_session::SessionManager;

/// The code a reply shows for confirming method-1 at `version`.
fn code(reply: &str, version: &str) -> String {
    reply
        .split(&format!("/learn confirm method-1 {version} "))
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn edits_rejections_and_restores_decide_what_later_requests_use_across_a_restart(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let mut responses = run_candidate();
    responses.extend([
        text("Done."),
        fitting("method-1"),
        text("Done."),
        text("Done."),
    ]);
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    say(
        &mut agent,
        &format!("/learn confirm method-1 v1 {}", code(&review, "v1")),
    )?;
    // When
    let edited = say(
        &mut agent,
        "/learn edit method-1 v1 step 2 Check the release date first",
    )?;
    let stale = say(&mut agent, "/learn edit method-1 v1 purpose Anything else")?;
    let unconfirmed = say(&mut agent, "Summarize the changelog in downloads/a.md")?;
    let unconfirmed_prompt = system_prompt(&provider.last());
    say(
        &mut agent,
        &format!("/learn confirm method-1 v2 {}", code(&edited, "v2")),
    )?;
    let with_edit = say(&mut agent, "Summarize the changelog in downloads/b.md")?;
    let edit_prompt = system_prompt(&provider.last());
    say(&mut agent, "/learn reject method-1 v2")?;
    let after_rejection = say(&mut agent, "Summarize the changelog in downloads/c.md")?;
    let restored = say(&mut agent, "/learn restore method-1 v1")?;
    let rejected_back = say(&mut agent, "/learn restore method-1 v2")?;
    drop(agent);
    let restart = Provider::new(vec![fitting("method-1"), text("Done."), text("Done.")]);
    let mut agent = super::agent(dir.path(), &tools, &restart)?;
    let with_restored = say(&mut agent, "Summarize the changelog in downloads/d.md")?;
    let restored_prompt = system_prompt(&restart.last());
    let deactivated = say(&mut agent, "/learn deactivate method-1 v1")?;
    let after_deactivation = say(&mut agent, "Summarize the changelog in downloads/e.md")?;
    let old_version = say(&mut agent, "/learn show method-1 v2")?;
    // Then
    assert!(
        edited.contains("Saved your edit as method-1 v2, made from v1")
            && edited.contains("2. Check the release date first — your edit in v2"),
        "{edited}"
    );
    assert!(
        stale.contains("method-1 is at v2, not v1; nothing was changed."),
        "{stale}"
    );
    assert_eq!(unconfirmed, "Done.");
    assert!(
        !unconfirmed_prompt.contains("learned method"),
        "{unconfirmed_prompt}"
    );
    assert!(
        with_edit.contains("Method used: method-1 v2 — confirmed by you on")
            && edit_prompt.contains("2. Check the release date first (the user's edit)"),
        "{with_edit}\n{edit_prompt}"
    );
    assert_eq!(after_rejection, "Done.");
    assert!(
        restored.contains("Made method-1 v1 current again; it is confirmed by you on")
            && restored.contains("what they did (files written, messages sent) is not undone"),
        "{restored}"
    );
    assert!(
        rejected_back.contains(
            "Only a version that was confirmed and not rejected can be made current again"
        ),
        "{rejected_back}"
    );
    assert!(
        with_restored.contains("Method used: method-1 v1 — confirmed by you on")
            && restored_prompt.contains("use learned method method-1 v1 as a guide")
            && !restored_prompt.contains("Check the release date first"),
        "{with_restored}\n{restored_prompt}"
    );
    assert!(
        deactivated.contains("Put method-1 v1 out of use."),
        "{deactivated}"
    );
    assert_eq!(after_deactivation, "Done.");
    assert!(
        old_version.starts_with("method-1 v2 — rejected by you on")
            && old_version.contains("· not the current version (v1 is)"),
        "{old_version}"
    );
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let used: Vec<&Value> = session
        .messages
        .iter()
        .filter_map(|message| message.get("learned_method"))
        .map(|record| &record["version"])
        .collect();
    assert_eq!(used, [&json!(2), &json!(1)]);
    assert_eq!(restart.calls(), 3);
    Ok(())
}

#[test]
fn two_edits_of_the_same_version_at_once_save_only_one() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(run_candidate());
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    drop(agent);
    let start = std::sync::Barrier::new(2);
    // When
    let replies: Vec<String> = std::thread::scope(|scope| {
        let edits: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|chat| {
                let (start, workspace) = (&start, dir.path());
                scope.spawn(move || -> Result<String, String> {
                    let tools = super::tools(&Arc::new(AtomicUsize::new(0)));
                    let provider = Provider::new(Vec::new());
                    let mut agent =
                        super::agent(workspace, &tools, &provider).map_err(|e| e.to_string())?;
                    start.wait();
                    say_in(
                        &mut agent,
                        chat,
                        &format!("/learn edit method-1 v1 step 2 Check it in chat {chat}"),
                    )
                    .map_err(|e| e.to_string())
                })
            })
            .collect();
        edits
            .into_iter()
            .map(|edit| {
                edit.join()
                    .map_err(|_| "panicked".to_owned())
                    .and_then(|r| r)
            })
            .collect::<Result<_, _>>()
    })?;
    // Then
    let saved = replies
        .iter()
        .filter(|reply| reply.contains("Saved your edit as method-1 v2"))
        .count();
    let stale = replies
        .iter()
        .filter(|reply| reply.contains("method-1 is at v2, not v1; nothing was changed."))
        .count();
    assert_eq!((saved, stale), (1, 1), "{replies:?}");
    let method = &stored(dir.path())?["methods"][0];
    assert_eq!(method["versions"].as_array().map(Vec::len), Some(2));
    Ok(())
}

#[test]
fn a_step_you_wrote_is_redone_only_by_your_words_in_another_task() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let mut responses = run_candidate();
    responses.extend([
        read("downloads/one.md"),
        text("Done."),
        sorted(&[("e1", "request")]),
        task_check(
            true,
            json!([{"step": 1, "evidence": ["e2"]}, {"step": 2, "evidence": ["e2"]}]),
            json!([]),
        ),
        read("downloads/two.md"),
        text("Done."),
        text("Okay."),
        sorted(&[("e1", "request"), ("e4", "feedback")]),
        task_check(
            true,
            json!([{"step": 1, "evidence": ["e2"]}, {"step": 2, "evidence": ["e4"]}]),
            json!([]),
        ),
    ]);
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    say(
        &mut agent,
        "/learn edit method-1 v1 step 2 Check the release date first",
    )?;
    // When
    say_in(
        &mut agent,
        "one",
        "Summarize the changelog in downloads/one.md",
    )?;
    let by_a_run = say_in(&mut agent, "one", "/learn verify method-1")?;
    say_in(
        &mut agent,
        "two",
        "Summarize the changelog in downloads/two.md",
    )?;
    say_in(&mut agent, "two", "Check the release date first.")?;
    let by_your_words = say_in(&mut agent, "two", "/learn verify method-1")?;
    // Then
    assert!(
        by_a_run.contains("step 2 was not redone with evidence in this task"),
        "{by_a_run}"
    );
    assert!(
        by_your_words.contains("This is a second, different task, so method-1 v2 is now confirmed"),
        "{by_your_words}"
    );
    let method = &stored(dir.path())?["methods"][0];
    assert_eq!(method["current_version"], 2);
    assert_eq!(method["state"], "confirmed");
    Ok(())
}
