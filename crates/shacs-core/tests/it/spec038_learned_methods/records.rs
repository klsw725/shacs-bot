use super::*;
use shacs_session::Session;

const KEY_SECRET: &str = "sk-live-abcdef1234567890";

struct Named(String);
impl Tool for Named {
    fn name(&self) -> &str {
        &self.0
    }
    fn description(&self) -> &str {
        "A registered tool"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        "unused".into()
    }
}

fn registered<'a>(names: impl IntoIterator<Item = &'a str>) -> ToolRegistry {
    let mut tools = tools(&Arc::new(AtomicUsize::new(0)));
    for name in names {
        tools.register(Named(name.to_owned()));
    }
    tools
}
/// A stored message, with the mark the runtime puts on a turn the user typed.
fn said(role: &str, content: &str) -> Value {
    let mut message = json!({"role": role, "content": content});
    if role == "user" {
        message["owner_turn"] = json!(true);
    }
    message
}
fn called(id: &str, name: &str) -> Value {
    json!({"role": "assistant", "content": null, "tool_calls": [
        {"id": id, "type": "function", "function": {"name": name, "arguments": "{}"}}
    ]})
}
fn returned(id: &str, content: &str) -> Value {
    json!({"role": "tool", "tool_call_id": id, "name": "tool", "content": content})
}

#[test]
fn failures_quarantine_omissions_and_the_session_key_are_recorded_conservatively(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut session = Session::new(format!("chat:{KEY_SECRET}"));
    session.messages = vec![
        json!({"role": "user", "content": "Subagent result", "metadata": {"injected_event": "subagent_result"}}),
        said("assistant", "Acknowledged."),
        said("user", "Fetch the page and summarize it"),
        called("call-a", "web_fetch"),
        returned("call-a", r#"{"error":"network failure"}"#),
        called("call-b", "read_file"),
        returned("call-b", "<withheld: 10 chars, sha256:abcdef123456>"),
        called("call-c", "exec"),
        returned("call-c", "partial output\n\nExit code: 1"),
        called("call-d", "mcp_demo_lookup"),
        returned("call-d", "(MCP tool call timed out after 30s)"),
        called("call-e", "cron"),
        returned("call-e", "Job nightly not found"),
        called("call-f", "Email Bob"),
        returned("call-f", "Error: Tool 'Email Bob' not found"),
        called("call-g", "read_file"),
        returned("call-g", "[File unchanged since last read: notes.md]"),
        said("assistant", "Done."),
    ];
    for turn in ["two", "three", "four", "five", "six"] {
        session.messages.push(said("user", turn));
        session.messages.push(said("assistant", "ok"));
    }
    SessionManager::new(dir.path())?.save(&session)?;
    let tools = registered(["web_fetch", "exec", "mcp_demo_lookup", "cron"]);
    let provider = Provider::new(vec![
        sorted(&[("e1", "procedure")]),
        draft(json!([
            {"text": "Fetch the page and summarize it", "evidence": ["e1"]},
            {"text": "Use web_fetch", "evidence": ["e2"]},
            {"text": "Use read_file", "evidence": ["e3"]},
            {"text": "Use exec", "evidence": ["e4"]},
            {"text": "Look it up", "evidence": ["e5"]},
            {"text": "Remove the nightly job", "evidence": ["e6"]},
            {"text": "Write to them", "evidence": ["e7"]},
            {"text": "Read it again", "evidence": ["e8"]}
        ])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    // When
    let shown = agent
        .process_message(InboundMessage::new("chat", "alice", KEY_SECRET, "/learn"))?
        .final_content
        .unwrap_or_default();
    // Then
    let file = std::fs::read_to_string(dir.path().join(".shacs-methods/methods.json"))?;
    assert!(
        !file.contains(KEY_SECRET) && !shown.contains(KEY_SECRET),
        "{shown}"
    );
    let drafting = serde_json::to_string(&provider.last().messages)?;
    assert!(
        !drafting.contains("Bob") && !file.contains("Bob"),
        "{drafting}"
    );
    let version = &stored(dir.path())?["methods"][0]["versions"][0];
    assert!(version["origin"]["session"]
        .as_str()
        .is_some_and(|session| session.starts_with("sha256:")));
    assert_eq!(version["origin"]["omitted_evidence"], 2);
    assert_eq!(evidence(version, "e2")["outcome"], "error");
    assert_eq!(evidence(version, "e3")["outcome"], "incomplete");
    assert_eq!(evidence(version, "e4")["outcome"], "error");
    assert_eq!(evidence(version, "e5")["outcome"], "error");
    assert_eq!(evidence(version, "e6")["outcome"], "error");
    assert_eq!(evidence(version, "e2")["trust"], "untrusted");
    assert_eq!(evidence(version, "e2")["tool"], "web_fetch");
    assert_eq!(evidence(version, "e7")["tool"], "unregistered tool");
    assert_eq!(evidence(version, "e8")["outcome"], "incomplete");
    Ok(())
}

#[test]
fn a_later_call_reusing_an_id_cannot_finish_an_earlier_call() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut session = Session::new("chat:room");
    session.messages = vec![
        said("user", "Run the checks first"),
        called("same", "never_ran"),
        said("user", "Try again"),
        called("same", "actual_tool"),
        returned("same", "all good"),
        said("assistant", "Done."),
    ];
    SessionManager::new(dir.path())?.save(&session)?;
    let tools = registered(["never_ran", "actual_tool"]);
    let provider = Provider::new(vec![
        sorted(&[("e1", "procedure"), ("e3", "request")]),
        draft(json!([
            {"text": "Run the checks first", "evidence": ["e1"]},
            {"text": "Use the first tool", "evidence": ["e2"]},
            {"text": "Use the second tool", "evidence": ["e4"]}
        ])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    // When
    say(&mut agent, "/learn")?;
    // Then
    let version = &stored(dir.path())?["methods"][0]["versions"][0];
    assert_eq!(evidence(version, "e2")["outcome"], "incomplete");
    assert_eq!(evidence(version, "e4")["outcome"], "ok");
    assert_eq!(version["steps"][1]["support"], "unverified");
    assert_eq!(version["steps"][2]["text"], "Run actual_tool");
    Ok(())
}

#[test]
fn a_step_citing_many_long_runs_still_reads_back_after_saving() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut session = Session::new("chat:room");
    session
        .messages
        .push(said("user", "Check every source first"));
    let names: Vec<String> = (0..38)
        .map(|n| format!("{n:02}{}", "x".repeat(62)))
        .collect();
    for name in &names {
        session.messages.push(called(name, name));
        session.messages.push(returned(name, "fine"));
    }
    session.messages.push(said("assistant", "Done."));
    SessionManager::new(dir.path())?.save(&session)?;
    let cited: Vec<String> = (2..=39).map(|n| format!("e{n}")).collect();
    let tools = registered(names.iter().map(String::as_str));
    let provider = Provider::new(vec![
        sorted(&[("e1", "procedure")]),
        draft(json!([{"text": "Check every source", "evidence": cited}])),
    ]);
    let mut agent = agent(dir.path(), &tools, &provider)?;
    // When
    let saved = say(&mut agent, "/learn")?;
    let listed = say(&mut agent, "/learn list")?;
    // Then
    assert!(saved.contains("Saved method candidate method-1"), "{saved}");
    assert!(listed.contains("- method-1 v1 candidate"), "{listed}");
    Ok(())
}

#[test]
fn a_run_that_finished_after_saving_is_shown_unrechecked_not_refused() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let mut session = Session::new("chat:room");
    session.messages = vec![
        said("user", "Check the notes first"),
        called("pending", "read_file"),
    ];
    SessionManager::new(dir.path())?.save(&session)?;
    let tools = tools(&Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        sorted(&[("e1", "procedure")]),
        draft(json!([
            {"text": "Check the notes first", "evidence": ["e1"]},
            {"text": "Read the notes", "evidence": ["e2"]}
        ])),
    ]);
    say(&mut agent(dir.path(), &tools, &provider)?, "/learn")?;
    session.messages.push(returned("pending", "notes"));
    SessionManager::new(dir.path())?.save(&session)?;
    // When
    let shown = say(
        &mut agent(dir.path(), &tools, &provider)?,
        "/learn show method-1",
    )?;
    // Then
    for expected in [
        "Check the notes first — your own words (e1)",
        "Read the notes — unverified: the model's wording (e2); not rechecked: its source changed after saving",
        "e2 tool call read_file (did not finish)",
        "its result changed after saving, not rechecked",
    ] {
        assert!(shown.contains(expected), "missing {expected}:\n{shown}");
    }
    Ok(())
}
