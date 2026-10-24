use super::*;

#[test]
fn short_requests_and_their_specifics_are_never_copied() -> Result<(), Box<dyn Error>> {
    let plain = "Next time verify twice.";
    for (request, feedback, purpose) in [
        ("zephyrcase", plain, "zephyrcase"),
        ("보고서요약해줘", plain, "보고서요약해줘"),
        ("\u{1F512}", plain, "\u{1F512}"),
        (
            "Summarize project 123",
            "Next time compare version 1234 and verify twice.",
            "Handle project 123",
        ),
    ] {
        // Given
        let dir = tempfile::tempdir()?;
        let tools = tools(&Arc::new(AtomicUsize::new(0)));
        let quote = json!({"text": "verify twice", "evidence": ["e3"]});
        let provider = Provider::new(vec![
            text("Done."),
            text("Noted."),
            sorted(&[("e1", "request"), ("e3", "feedback")]),
            candidate(purpose, json!([]), json!([quote])),
        ]);
        let mut agent = agent(dir.path(), &tools, &provider)?;
        say(&mut agent, request)?;
        say(&mut agent, feedback)?;
        // When
        let refused = say(&mut agent, "/learn")?;
        // Then
        assert!(
            refused.contains("Nothing was saved."),
            "{request}: {refused}"
        );
        assert!(!dir.path().join(".shacs-methods/methods.json").exists());
    }
    Ok(())
}

#[test]
fn a_request_is_never_copied_into_the_candidate() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let request =
        "summarize the cypress dossier. It is at /private/cypress.txt for PROJECT-CYPRESS";
    let quote = json!({"text": "check the dossier date first", "evidence": ["e3"]});
    let provider = Provider::new(vec![
        text("Summary: two open issues."),
        text("Noted."),
        sorted(&[("e1", "request"), ("e3", "feedback")]),
        candidate("Summarize the cypress dossier", json!([]), json!([quote])),
        sorted(&[("e1", "request"), ("e3", "feedback")]),
        candidate(
            "Summarize a customer dossier",
            json!([{"text": "Files are under /private/cypress.txt", "evidence": ["e1"]}]),
            json!([
                quote,
                {"text": request, "evidence": ["e1"]},
                {"text": "Ask the PROJECT-CYPRESS team", "evidence": ["e1"]}
            ]),
        ),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, request)?;
    say(&mut agent, "Good. Next time check the dossier date first.")?;
    // When
    let refused = say(&mut agent, "/learn")?;
    let saved = say(&mut agent, "/learn")?;
    let path = dir.path().join(".shacs-methods/methods.json");
    let file = std::fs::read_to_string(&path)?;
    let mut edited = stored(dir.path())?;
    edited["methods"][0]["versions"][0]["purpose"]["text"] = json!("summarize the cypress dossier");
    std::fs::write(&path, serde_json::to_vec(&edited)?)?;
    let copied_back = say(&mut agent, "/learn show method-1")?;
    // Then
    assert!(refused.contains("Nothing was saved."), "{refused}");
    assert!(!file.to_lowercase().contains("cypress"), "{file}");
    assert!(saved.contains("check the dossier date first — your own words (e3)"));
    assert!(saved.contains("3 proposed text(s) were dropped"), "{saved}");
    assert!(
        copied_back.contains("a method does not match the messages it cites"),
        "{copied_back}"
    );
    Ok(())
}
