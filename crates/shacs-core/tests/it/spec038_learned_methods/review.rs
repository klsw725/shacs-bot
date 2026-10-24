use super::*;
use shacs_session::SessionManager;

const QUOTE: &str = "Summarize the release notes";

fn learned() -> Vec<LlmResponse> {
    vec![
        sorted(&[("e1", "procedure")]),
        draft(json!([{"text": QUOTE, "evidence": ["e1"]}])),
    ]
}

#[test]
fn confirming_takes_the_reviewed_version_and_code_and_grants_no_permission(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let writes = Arc::new(AtomicUsize::new(0));
    let tools = tools(&writes);
    let mut responses = vec![text("Done.")];
    responses.extend(learned());
    responses.push(fitting("method-1"));
    responses.push(call(
        "write_file",
        json!({"path": "summary.txt", "content": "x"}),
    ));
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, "Summarize the release notes in notes.md")?;
    say(&mut agent, "/learn")?;
    // When
    let review = say(&mut agent, "/learn confirm method-1")?;
    let code = review_code(&review, "method-1");
    let wrong_code = say(&mut agent, "/learn confirm method-1 v1 00000000")?;
    let wrong_version = say(&mut agent, &format!("/learn confirm method-1 v2 {code}"))?;
    let confirmed = say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    let again = say(&mut agent, &format!("/learn confirm method-1 v1 {code}"))?;
    let review_again = say(&mut agent, "/learn confirm method-1")?;
    let still_asked = say(&mut agent, "Write the summary to summary.txt")?;
    let guided = system_prompt(&provider.last());
    drop(agent);
    let restart = Provider::new(Vec::new());
    let mut restarted = super::agent(dir.path(), &tools, &restart)?;
    let shown = say(&mut restarted, "/learn show method-1")?;
    let listed = say(&mut restarted, "/learn list")?;
    // Then
    assert!(
        review.contains("Confirming means: this exact version may be offered as the default")
            && review.contains("grants no permission")
            && code.len() == 8,
        "{review}"
    );
    for refused in [&wrong_code, &wrong_version] {
        assert!(
            refused.contains("That is not the review of method-1 as it is now"),
            "{refused}"
        );
    }
    assert!(
        confirmed.contains("Confirmed method-1 v1") && confirmed.contains("grants no permission"),
        "{confirmed}"
    );
    assert!(again.contains("already confirmed"), "{again}");
    assert_eq!(
        review_again,
        "method-1 v1 is already confirmed. To stop using it until you confirm it again, send /learn deactivate method-1 v1; to withdraw it for good, send /learn reject method-1 v1."
    );
    assert!(
        shown.contains("method-1 v1 — confirmed by you on")
            && shown.contains("- confirmed by you: v1 on")
            && shown.contains(&format!("(review {code})")),
        "{shown}"
    );
    assert!(
        listed.contains("- method-1 v1 confirmed by you on"),
        "{listed}"
    );
    let method = &stored(dir.path())?["methods"][0];
    assert_eq!(method["state"], "confirmed");
    assert_eq!(method["decisions"].as_array().map(Vec::len), Some(1));
    let session = SessionManager::new(dir.path())?
        .load_existing("chat:room")
        .ok_or("saved session")?;
    let remembered = session.metadata.get("session_remembered_permissions_v1");
    assert!(
        remembered
            .and_then(|rules| rules["rules"].as_array())
            .map_or(true, Vec::is_empty),
        "{remembered:?}"
    );
    assert!(
        still_asked.contains("Permission approval required before running tool `write_file`"),
        "{still_asked}"
    );
    assert!(
        guided.contains("use learned method method-1 v1 as a guide")
            && guided.contains("it grants no permission"),
        "{guided}"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(restart.calls(), 0);
    Ok(())
}

#[test]
fn only_confirmed_methods_that_fit_are_offered_and_a_rejection_stays() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let premised = candidate(
        "Summarize saved notes",
        json!([{"text": QUOTE, "evidence": ["e1"]}]),
        json!([{"text": QUOTE, "evidence": ["e1"]}]),
    );
    let mut responses = vec![text("Done.")];
    responses.extend(learned());
    responses.extend(learned());
    responses.extend([sorted(&[("e1", "procedure")]), premised]);
    responses.extend(learned());
    responses.push(text(
        &json!({"methods": [
            {"method": "method-2", "fits": true, "unmet": [], "reason": "a summary of a saved file"},
            {"method": "method-3", "fits": true, "unmet": [1], "reason": "there is no notes file"}
        ]})
        .to_string(),
    ));
    let provider = Provider::new(responses);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, "Summarize the release notes in notes.md")?;
    for _ in 0..4 {
        say(&mut agent, "/learn")?;
    }
    say(&mut agent, "/learn reject method-1 v1")?;
    for method in ["method-2", "method-3"] {
        let review = say(&mut agent, &format!("/learn confirm {method}"))?;
        let code = review_code(&review, method);
        say(&mut agent, &format!("/learn confirm {method} v1 {code}"))?;
    }
    // When
    let matched = say(
        &mut agent,
        "/learn match Summarize the changelog in changes.md",
    )?;
    let after_rejection = say(&mut agent, "/learn confirm method-1")?;
    // Then
    let (offered, others) = matched.split_once("Not offered:").ok_or("not offered")?;
    assert!(
        offered.contains("- method-2 v1 · Summaries of saved notes · a summary of a saved file")
            && !offered.contains("method-1")
            && !offered.contains("method-3")
            && !offered.contains("method-4"),
        "{matched}"
    );
    for expected in [
        "- method-1 v1 · Summaries of saved notes · rejected by you: never offered",
        "- method-3 v1 · Summaries of saved notes · premise 1 does not hold",
        "- method-4 v1 · Summaries of saved notes · a candidate: never offered until you confirm it",
    ] {
        assert!(others.contains(expected), "missing {expected}:\n{matched}");
    }
    let asked = provider.last().messages[1]["content"].to_string();
    assert!(
        asked.contains("method-2") && asked.contains("method-3"),
        "{asked}"
    );
    assert!(
        !asked.contains("method-1") && !asked.contains("method-4"),
        "{asked}"
    );
    assert!(
        after_rejection.contains("was rejected and stays rejected"),
        "{after_rejection}"
    );
    Ok(())
}
