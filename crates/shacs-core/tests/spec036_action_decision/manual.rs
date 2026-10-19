use super::*;
use std::io::BufRead;

#[test]
#[ignore = "interactive AgentLoop driver; run alone with --nocapture"]
fn conversation_driver() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(vec![
        response(vec![proposal(&work.work.work_id)]),
        response(vec![proposal(&work.work.work_id)]),
        response(vec![send_call()]),
        LlmResponse {
            content: Some("Fake send completed; no remote transport was contacted.".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut agent = runtime(workspace.path(), &tools, &provider)?;
    println!("AgentLoop conversation driver. First proposal: decline. Second: accept, then approve the tool. /quit exits. WORK_ID={}", work.work.work_id);
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line == "/quit" {
            break;
        }
        let result = agent.process_message(inbound(&line))?;
        println!(
            "ASSISTANT: {}\nSTOP={} PROVIDER_CALLS={} SIDE_EFFECTS={}",
            result.final_content.as_deref().unwrap_or_default(),
            result.stop_reason,
            provider.calls(),
            counter.load(Ordering::SeqCst)
        );
    }
    Ok(())
}
