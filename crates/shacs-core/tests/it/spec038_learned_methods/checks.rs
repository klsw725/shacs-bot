use super::*;

pub(super) const REQUEST: &str = "Summarize the release notes in downloads/notes.md";

/// A candidate, learned in chat `room`, whose one step reruns `read_file`.
pub(super) fn run_candidate() -> Vec<LlmResponse> {
    vec![
        read("downloads/notes.md"),
        text("Done."),
        sorted(&[("e1", "request")]),
        draft(json!([{"text": "Read the notes", "evidence": ["e2"]}])),
    ]
}

/// Another task that reads `path` and is then checked; the check reply
/// says that read redid step 1.
pub(super) fn redone_in(path: &str) -> Vec<LlmResponse> {
    vec![
        read(path),
        text("Done."),
        sorted(&[("e1", "request")]),
        task_check(true, json!([{"step": 1, "evidence": ["e2"]}]), json!([])),
    ]
}

#[test]
fn a_different_task_that_redoes_every_supported_step_confirms_the_candidate(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let mut responses = run_candidate();
    responses.push(sorted(&[("e1", "request")]));
    responses.push(task_check(
        true,
        json!([{"step": 1, "evidence": ["e2"]}]),
        json!([]),
    ));
    responses.extend(redone_in("downloads/changes.md"));
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    // When
    let same_task = say(&mut agent, "/learn verify method-1")?;
    say_in(
        &mut agent,
        "other",
        "Summarize the changelog in downloads/changes.md",
    )?;
    let other_task = say_in(&mut agent, "other", "/learn verify method-1")?;
    let shown = say_in(&mut agent, "other", "/learn show method-1")?;
    say_in(&mut agent, "other", "/new")?;
    let cleared = say_in(&mut agent, "other", "/learn show method-1")?;
    // Then
    assert!(
        same_task.contains("not counted for method-1: it repeats a request of a task already recorded for this method, and what it shows was already recorded"),
        "{same_task}"
    );
    assert!(
        cleared.contains("confirmed: redone in a second, different task on")
            && cleared
                .contains("(the task it was redone in is no longer in its session, not rechecked)"),
        "{cleared}"
    );
    assert!(
        other_task.contains("This is a second, different task, so method-1 v1 is now confirmed")
            && other_task.contains("grants no permission"),
        "{other_task}"
    );
    let method = &stored(dir.path())?["methods"][0];
    assert_eq!(method["state"], "confirmed");
    let decision = &method["decisions"][0];
    assert_eq!(decision["kind"], "verified");
    let origin = &method["versions"][0];
    assert_ne!(
        decision["check"]["requests"][0],
        evidence(origin, "e1")["digest"]
    );
    for expected in [
        "method-1 v1 — confirmed: redone in a second, different task on",
        "- redone in a different task: v1 on",
        "step 1 by e2",
        "  - e2 tool call read_file (ran without a reported error)",
    ] {
        assert!(shown.contains(expected), "missing {expected}:\n{shown}");
    }
    Ok(())
}

#[test]
fn the_task_a_method_came_from_cannot_confirm_it_under_other_labels() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        text("Done."),
        text("Okay."),
        sorted(&[("e1", "request"), ("e3", "feedback")]),
        draft(json!([{"text": "check the date first", "evidence": ["e3"]}])),
        text("You're welcome."),
        sorted(&[("e1", "other"), ("e3", "feedback"), ("e5", "request")]),
        task_check(true, json!([{"step": 1, "evidence": ["e3"]}]), json!([])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, "Summarize the release notes in notes.md")?;
    say(&mut agent, "Next time check the date first.")?;
    say(&mut agent, "/learn")?;
    say(&mut agent, "Thanks.")?;
    // When
    let relabelled = say(&mut agent, "/learn verify method-1")?;
    // Then
    assert!(
        relabelled.contains("it shares your messages with a task already recorded for this method, and what it shows was already recorded. Nothing was saved."),
        "{relabelled}"
    );
    assert_eq!(stored(dir.path())?["methods"][0]["state"], "candidate");
    Ok(())
}
