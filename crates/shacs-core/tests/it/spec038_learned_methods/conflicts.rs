use super::checks::{redone_in, run_candidate, REQUEST};
use super::*;

#[test]
fn a_contradiction_keeps_checks_from_confirming_and_retires_a_confirmed_method(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let contradicted = |path: &str| {
        vec![
            read(path),
            text("Done."),
            text("Okay."),
            sorted(&[("e1", "request"), ("e4", "feedback")]),
            task_check(
                true,
                json!([{"step": 1, "evidence": ["e2"]}]),
                json!([{"step": 1, "evidence": ["e4"]}]),
            ),
        ]
    };
    let mut responses = run_candidate();
    responses.extend(contradicted("downloads/one.md"));
    responses.extend(redone_in("downloads/two.md"));
    let mut confirmed = contradicted("downloads/three.md");
    confirmed.insert(0, fitting("method-1"));
    confirmed.insert(3, fitting("method-1"));
    responses.extend(confirmed);
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let contradict = |agent: &mut AgentLoop<'_>, chat: &str| -> Result<String, Box<dyn Error>> {
        say_in(
            agent,
            chat,
            &format!("Summarize the changelog in downloads/{chat}.md"),
        )?;
        say_in(agent, chat, "Next time do not read the notes first.")?;
        say_in(agent, chat, "/learn verify method-1")
    };
    // When
    let first = contradict(&mut agent, "one")?;
    say_in(
        &mut agent,
        "two",
        "Summarize the changelog in downloads/two.md",
    )?;
    let redone = say_in(&mut agent, "two", "/learn verify method-1")?;
    let state_after_redone = stored(dir.path())?["methods"][0]["state"].clone();
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    let second = contradict(&mut agent, "three")?;
    let matched = say(
        &mut agent,
        "/learn match Summarize the changelog in changes.md",
    )?;
    // Then
    assert!(
        first.contains("contradicts method-1 v1; recorded")
            && first.contains("will not be confirmed by later checks"),
        "{first}"
    );
    assert!(
        redone.contains("Recorded; method-1 stays candidate"),
        "{redone}"
    );
    assert_eq!(state_after_redone, "candidate");
    assert!(
        review.contains("- contradicted by your feedback: v1 on"),
        "{review}"
    );
    assert!(
        second.contains("method-1 is now out of use until you confirm it again"),
        "{second}"
    );
    assert_eq!(stored(dir.path())?["methods"][0]["state"], "inactive");
    assert!(
        matched.contains(
            "Offered as the default (confirmed methods that fit, as the model judges it):\n- none"
        ) && matched.contains("method-1 v1 · Summaries of saved notes · out of use"),
        "{matched}"
    );
    Ok(())
}

#[test]
fn feedback_in_the_task_a_method_came_from_can_contradict_it() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let feedback = [("e1", "request"), ("e4", "feedback")];
    let provider = Provider::new(vec![
        read("downloads/notes.md"),
        text("Done."),
        text("Okay."),
        sorted(&feedback),
        draft(json!([{"text": "Read the notes", "evidence": ["e2"]}])),
        sorted(&feedback),
        task_check(
            true,
            json!([{"step": 1, "evidence": ["e2"]}]),
            json!([{"step": 1, "evidence": ["e4"]}]),
        ),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "Next time do not read the notes first.")?;
    say(&mut agent, "/learn")?;
    // When
    let contradicted = say(&mut agent, "/learn verify method-1")?;
    // Then
    assert!(
        contradicted.contains("Your feedback in this task contradicts method-1 v1; recorded."),
        "{contradicted}"
    );
    Ok(())
}

#[test]
fn feedback_that_contradicts_a_method_is_recorded_even_in_a_task_judged_not_to_fit(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let mut responses = run_candidate();
    for (fits, unmet) in [(false, json!([])), (true, json!([1]))] {
        responses.extend([
            text("Done."),
            text("Okay."),
            sorted(&[("e1", "request"), ("e3", "feedback")]),
            text(
                &json!({"fits": fits, "unmet": unmet, "reason": "not a summary", "steps": [],
                        "conflicts": [{"step": 1, "evidence": ["e3"]}]})
                .to_string(),
            ),
        ]);
    }
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    // When
    let mut replies = Vec::new();
    for chat in ["unfit", "unmet"] {
        say_in(&mut agent, chat, &format!("Translate downloads/{chat}.md"))?;
        say_in(&mut agent, chat, "Next time do not read the notes first.")?;
        replies.push(say_in(&mut agent, chat, "/learn verify method-1")?);
    }
    // Then
    for reply in &replies {
        assert!(
            reply.contains("Your feedback in this task contradicts method-1 v1; recorded."),
            "{reply}"
        );
    }
    let method = &stored(dir.path())?["methods"][0];
    assert_eq!(method["decisions"][0]["kind"], "conflicted");
    assert_eq!(method["decisions"][1]["kind"], "conflicted");
    assert_eq!(method["state"], "candidate");
    Ok(())
}
