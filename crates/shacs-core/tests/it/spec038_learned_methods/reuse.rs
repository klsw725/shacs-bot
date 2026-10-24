use super::checks::{run_candidate, REQUEST};
use super::*;
use shacs_session::SessionManager;

/// The model's judgement of method-1 for a request.
fn judged(request_sets_format: bool, unmet: Value, format_steps: Value) -> LlmResponse {
    text(
        &json!({"request_sets_format": request_sets_format, "methods": [
            {"method": "method-1", "fits": true, "unmet": unmet, "reason": "the notes are pasted, not saved",
             "format_steps": format_steps}
        ]})
        .to_string(),
    )
}

#[test]
fn a_confirmed_method_guides_only_requests_it_fits_and_leaves_the_requested_format(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        read("downloads/notes.md"),
        text("Done."),
        text("Okay."),
        sorted(&[("e1", "request"), ("e4", "feedback")]),
        candidate(
            "Summarize saved notes",
            json!([{"text": "The notes are saved as files", "evidence": ["e4"]}]),
            json!([{"text": "Read the notes", "evidence": ["e2"]},
                   {"text": "List the changes as a table", "evidence": ["e4"]}]),
        ),
        fitting("method-1"),
        read("downloads/changes.md"),
        text("Done."),
        judged(false, json!([1]), json!([])),
        text("Done."),
        judged(true, json!([]), json!([2])),
        read("downloads/format.md"),
        text("Done."),
        fitting("method-1"),
        text("Done."),
        text("Done."),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(
        &mut agent,
        "The notes are saved as files. List the changes as a table.",
    )?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    // When
    let used = say(
        &mut agent,
        "Summarize the changelog in downloads/changes.md",
    )?;
    let guided = system_prompt(&provider.last());
    let unmet = say(&mut agent, "Summarize the changelog I paste below")?;
    let unguided = system_prompt(&provider.last());
    let formatted = say(
        &mut agent,
        "Summarize the changelog in downloads/format.md as three bullet points",
    )?;
    let format_guided = system_prompt(&provider.last());
    say(&mut agent, "/new")?;
    let cleared = say(
        &mut agent,
        "Summarize the changelog in downloads/changes.md",
    )?;
    let mut store = stored(dir.path())?;
    store["methods"][0]["versions"][0]["purpose"]["edited_in"] = json!(1);
    std::fs::write(
        dir.path().join(".shacs-methods/methods.json"),
        serde_json::to_vec(&store)?,
    )?;
    let tampered = say(
        &mut agent,
        "Summarize the changelog in downloads/changes.md",
    )?;
    let tampered_prompt = system_prompt(&provider.last());
    // Then
    assert!(
        used.starts_with("Done.\n\n—\nMethod used: method-1 v1 — confirmed by you on")
            && used.contains("it grants no permission. Applies to: Summaries of saved notes. Premises the model judged to hold: The notes are saved as files."),
        "{used}"
    );
    for expected in [
        "use learned method method-1 v1 as a guide",
        "- The notes are saved as files",
        "1. Run read_file (a tool run that went through when the method was learned; the model's description then: Read the notes)",
        "2. List the changes as a table (the user's own words)",
    ] {
        assert!(guided.contains(expected), "missing {expected}:\n{guided}");
    }
    assert!(
        unmet.ends_with("No learned method was used for this request:\n- method-1 v1 · Summaries of saved notes · premise 1 does not hold, as the model judges it (the notes are pasted, not saved)"),
        "{unmet}"
    );
    assert!(!unguided.contains("learned method"), "{unguided}");
    assert!(
        format_guided.contains("1. Run read_file")
            && !format_guided.contains("List the changes as a table")
            && format_guided
                .contains("Left out, because the request says how the result should look: step 2."),
        "{format_guided}"
    );
    assert!(
        formatted.contains("Left out: step 2 (your request says how the result should look)."),
        "{formatted}"
    );
    assert!(
        cleared.ends_with("Some sources it rests on changed or were cleared since, so they were not rechecked; /learn show method-1 shows which."),
        "{cleared}"
    );
    assert!(
        tampered.ends_with("No learned method was used: learned method store is invalid (malformed method record) and was left unchanged.")
            && !tampered_prompt.contains("learned method"),
        "{tampered}"
    );
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let records: Vec<&Value> = session
        .messages
        .iter()
        .filter_map(|message| message.get("learned_method"))
        .collect();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["method"], "method-1");
    assert_eq!(records[0]["version"], 1);
    assert_eq!(records[0]["state"], "confirmed");
    assert!(!session.metadata.contains_key("learned_method_turn"));
    Ok(())
}

#[test]
fn a_request_already_started_keeps_its_version_when_the_method_changes(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let writes = Arc::new(AtomicUsize::new(0));
    let tools = tools(&writes);
    let mut responses = run_candidate();
    responses.extend([
        fitting("method-1"),
        call("write_file", json!({"path": "summary.txt", "content": "x"})),
        text("Wrote it."),
    ]);
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, REQUEST)?;
    say(&mut agent, "/learn")?;
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    let asked = say(&mut agent, "Write the summary to summary.txt")?;
    // When
    let edited = say(
        &mut agent,
        "/learn edit method-1 v1 step 2 Check the release date first",
    )?;
    let approved = say(&mut agent, "1")?;
    let resumed = system_prompt(&provider.last());
    // Then
    assert!(
        asked.contains("Permission approval required before running tool `write_file`")
            && !asked.contains("Method used"),
        "{asked}"
    );
    assert!(
        edited.contains("Saved your edit as method-1 v2"),
        "{edited}"
    );
    assert!(
        approved.starts_with("Wrote it.\n\n—\nMethod used: method-1 v1"),
        "{approved}"
    );
    assert!(
        resumed.contains("use learned method method-1 v1 as a guide")
            && !resumed.contains("Check the release date first"),
        "{resumed}"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let record = session
        .messages
        .iter()
        .find_map(|message| message.get("learned_method"))
        .ok_or("a record of the method used")?;
    assert_eq!(record["version"], 1);
    Ok(())
}
