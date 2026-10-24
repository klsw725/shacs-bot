use super::image_support::{generate, image_runtime, image_tools, ok_images};
use super::*;

/// Reads the saved session files when tools are about to run and when an
/// iteration ends: what a crash at that moment would leave for recovery.
pub(super) struct SessionFileSpy {
    pub(super) sessions: std::path::PathBuf,
    pub(super) seen: Arc<Mutex<String>>,
}
impl SessionFileSpy {
    fn read(&self) {
        for entry in std::fs::read_dir(&self.sessions)
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                self.seen.lock().unwrap().push_str(&text);
            }
        }
    }
}
impl AgentHook for SessionFileSpy {
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        _: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.read();
        Vec::new()
    }
    fn after_iteration(&self, _: &AgentHookContext) {
        self.read();
    }
}

#[test]
fn a_crash_mid_image_turn_leaves_none_of_the_models_words_to_recover() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let mut first = generate("64x48");
    first.content = Some("Here is your blue cat!".to_owned());
    first.reasoning_content = Some("The blue cat is what they want.".to_owned());
    let provider = Provider::new(vec![first, text("The blue cat is ready.")]);
    let seen = Arc::new(Mutex::new(String::new()));
    let spy = SessionFileSpy {
        sessions: dir.path().join("sessions"),
        seen: seen.clone(),
    };
    let mut agent =
        image_runtime(dir.path(), vec![media], &tools, &provider)?.with_agent_hook(Arc::new(spy));
    // When
    agent.process_message(inbound("Generate an image of a red square, 64x48 png"))?;
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.starts_with("Image saved and checked on disk"),
        "{answer}"
    );
    let seen = seen.lock().unwrap();
    assert!(
        seen.contains("runtime_checkpoint"),
        "the spy saw no checkpoint"
    );
    let leak = seen
        .find("blue cat")
        .map(|at| &seen[at.saturating_sub(160)..at + 8]);
    assert_eq!(leak, None);
    Ok(())
}
