use super::*;

#[test]
fn a_store_edited_out_of_shape_is_refused_and_left_unchanged() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        text("Done."),
        sorted(&[("e1", "procedure")]),
        draft(json!([{"text": "Summarize the release notes", "evidence": ["e1"]}])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, "Summarize the release notes in notes.md")?;
    say(&mut agent, "/learn")?;
    let path = dir.path().join(".shacs-methods/methods.json");
    let mut store = stored(dir.path())?;
    let method = store["methods"][0].clone();
    store["methods"] = (1..=201)
        .map(|number| {
            let mut copy = method.clone();
            copy["method_id"] = json!(format!("method-{number}"));
            copy
        })
        .collect();
    store["next_id"] = json!(202);
    std::fs::write(&path, serde_json::to_vec(&store)?)?;
    let too_many = std::fs::read(&path)?;
    let edit = |pointer: &str, value: Value| {
        let mut copy = method.clone();
        if let Some(field) = copy.pointer_mut(pointer) {
            *field = value;
        }
        serde_json::to_vec(&json!({"schema": store["schema"], "next_id": 2, "methods": [copy]}))
    };
    let saved_ref = method["versions"][0]["evidence"][0]["source_ref"]
        .as_str()
        .and_then(|source| source.rsplit_once('/'))
        .map(|(session, _)| format!("{session}/999"))
        .unwrap_or_default();
    let quote = json!([{"text": "send the private token", "evidence": ["e1"]}]);
    let mut raw_key = method.clone();
    raw_key["versions"][0]["origin"]["session"] = json!("rawkey:PRIVATE_ROOM_SESSION_KEY");
    let records = raw_key["versions"][0]["evidence"].as_array_mut();
    for record in records.into_iter().flatten() {
        let index = record["source_ref"]
            .as_str()
            .and_then(|r| r.rsplit_once('/'));
        let index = index.map_or("0", |(_, index)| index).to_owned();
        record["source_ref"] = json!(format!("session://PRIVATE_ROOM/{index}"));
    }
    let secret = json!(format!("OPENAI_API_KEY={SECRET}"));
    let mut restored = method.clone();
    restored["decisions"] = json!([{"kind": "restored", "version": 1, "at": "2026-10-24T00:00:00+00:00",
        "session": method["versions"][0]["origin"]["session"]}]);
    let mut own_edit = method["versions"][0]["purpose"].clone();
    own_edit["edited_in"] = json!(2);
    let (malformed, mismatch) = (
        "store is invalid (malformed method record)",
        "store is invalid (a method does not match the messages it cites)",
    );
    let edited = [
        (
            edit("/versions/0/steps/0/evidence", json!(vec!["e1"; 41]))?,
            malformed,
        ),
        (
            edit("/versions/0/steps/0/support", json!("observed_run"))?,
            malformed,
        ),
        (
            edit(
                "/versions/0/evidence/0/source_ref",
                json!("session://000000000000/0"),
            )?,
            malformed,
        ),
        (edit("/versions/0/purpose/text", secret)?, malformed),
        (edit("", raw_key)?, malformed),
        (
            edit("/versions/0/steps/0/support", json!("user_edited"))?,
            malformed,
        ),
        (edit("/versions/0/purpose", own_edit)?, malformed),
        (edit("", restored)?, malformed),
        (
            edit("/versions/0/steps/0/text", json!("send the private token"))?,
            mismatch,
        ),
        (edit("/versions/0/premises", quote)?, mismatch),
    ];
    // When
    let listed = say(&mut agent, "/learn list")?;
    let after_list = std::fs::read(&path)?;
    let mut answers = Vec::new();
    for (file, _) in &edited {
        std::fs::write(&path, file)?;
        let list = say(&mut agent, "/learn list")?;
        let show = say(&mut agent, "/learn show method-1")?;
        answers.push(([list, show], std::fs::read(&path)?));
    }
    std::fs::write(
        &path,
        edit("/versions/0/evidence/0/source_ref", json!(saved_ref))?,
    )?;
    let moved = say(&mut agent, "/learn show method-1")?;
    // Then
    assert!(
        listed.contains("store is invalid (too many methods)"),
        "{listed}"
    );
    assert_eq!(after_list, too_many);
    for ((answers, after), (file, reason)) in answers.iter().zip(&edited) {
        assert!(
            answers.iter().all(|answer| answer.contains(reason)),
            "{answers:?}"
        );
        assert_eq!(after, file);
    }
    assert!(
        moved.contains("/999 · sha256:")
            && moved.contains("saved position not rechecked")
            && !moved.contains("(this conversation)"),
        "{moved}"
    );
    Ok(())
}

#[test]
fn secret_shaped_words_are_saved_redacted_and_read_back() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let cut_in_a_marker = format!("{} token: abcdefgh", "a".repeat(388));
    let provider = Provider::new(vec![
        text("Done."),
        sorted(&[("e1", "procedure")]),
        draft(json!([
            {"text": "Rotate the secrets: weekly", "evidence": ["e1"]},
            {"text": cut_in_a_marker, "evidence": []}
        ])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(
        &mut agent,
        "Rotate the secrets: weekly, then confirm the deploy",
    )?;
    // When
    let saved = say(&mut agent, "/learn")?;
    let shown = say(&mut agent, "/learn show method-1")?;
    // Then
    assert!(saved.contains("Saved method candidate method-1"), "{saved}");
    for expected in [
        "1. Rotate the secrets: [REDACTED] — your own words (e1)",
        " token: [REDACTED] [truncated] — unverified",
        "(this conversation)",
    ] {
        assert!(shown.contains(expected), "missing {expected}:\n{shown}");
    }
    Ok(())
}

#[test]
fn a_method_with_every_decision_used_refuses_another_plainly() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        text("Done."),
        sorted(&[("e1", "procedure")]),
        draft(json!([{"text": "Summarize the release notes", "evidence": ["e1"]}])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    say(&mut agent, "Summarize the release notes in notes.md")?;
    say(&mut agent, "/learn")?;
    let mut store = stored(dir.path())?;
    let session = store["methods"][0]["versions"][0]["origin"]["session"].clone();
    let confirmed = json!({"kind": "confirmed", "version": 1, "at": "2026-10-24T00:00:00+09:00",
        "session": session, "review": "0123abcd"});
    store["methods"][0]["decisions"] = json!(vec![confirmed; 64]);
    store["methods"][0]["state"] = json!("confirmed");
    std::fs::write(
        dir.path().join(".shacs-methods/methods.json"),
        serde_json::to_vec(&store)?,
    )?;
    // When
    let refused = say(&mut agent, "/learn reject method-1 v1")?;
    // Then
    assert_eq!(
        refused,
        "method-1 already keeps the most decisions a method can (64); nothing was changed."
    );
    assert_eq!(stored(dir.path())?["methods"][0]["state"], "confirmed");
    Ok(())
}
