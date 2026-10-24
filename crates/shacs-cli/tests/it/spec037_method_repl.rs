use serde_json::json;
use shacs_config::{save_config_to_path, Config};
use shacs_providers::{LlmResponse, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Fixture {
    _root: tempfile::TempDir,
    config: PathBuf,
    responses: PathBuf,
}
impl Fixture {
    fn new(responses: Vec<LlmResponse>) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let config_path = root.path().join("config.json");
        let config: Config = serde_json::from_value(json!({
            "agents":{"defaults":{"workspace":workspace,"provider":"custom","model":"test-model"}},
            "providers":{"custom":{"apiKey":"sk-isolated-fixture","apiBase":"http://127.0.0.1:1/v1"}},
            "tools":{"exec":{"enable":true},"web":{"enable":false}}
        }))?;
        save_config_to_path(&config, &config_path)?;
        let response_path = root.path().join("responses.json");
        fs::write(&response_path, serde_json::to_vec(&responses)?)?;
        Ok(Self {
            _root: root,
            config: config_path,
            responses: response_path,
        })
    }
    fn start(&self, side_effect_tools: bool) -> Result<Repl, Box<dyn Error>> {
        Repl::start(&self.config, &self.responses, side_effect_tools)
    }
}

struct Repl {
    child: Option<Child>,
    input: Option<ChildStdin>,
    output: Receiver<Vec<u8>>,
}
impl Repl {
    fn start(
        config: &Path,
        responses: &Path,
        side_effect_tools: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
        command
            .args(["--config"])
            .arg(config)
            .args(["agent", "--session", "spec037"])
            .env("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", responses)
            .env_remove("SHACS_RUNTIME_PACKAGE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !side_effect_tools {
            command.arg("--no-side-effects");
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        let mut stdout = child.stdout.take().ok_or("missing stdout")?;
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            loop {
                match stdout.read(&mut bytes) {
                    Ok(0) | Err(_) => return,
                    Ok(count) => {
                        if tx.send(bytes[..count].to_vec()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        let repl = Self {
            child: Some(child),
            input,
            output,
        };
        repl.until_prompt()?;
        Ok(repl)
    }
    fn until_prompt(&self) -> Result<String, Box<dyn Error>> {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"shacs-bot> ") {
            bytes.extend(self.output.recv_timeout(Duration::from_secs(15))?);
        }
        Ok(String::from_utf8(bytes)?)
    }
    fn send(&mut self, input: &str) -> Result<String, Box<dyn Error>> {
        let stdin = self.input.as_mut().ok_or("missing stdin")?;
        writeln!(stdin, "{input}")?;
        stdin.flush()?;
        self.until_prompt()
    }
    fn close(mut self) -> Result<(), Box<dyn Error>> {
        self.input.take();
        let result = self
            .child
            .take()
            .ok_or("missing child")?
            .wait_with_output()?;
        if !result.status.success() {
            return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
        }
        Ok(())
    }
}
impl Drop for Repl {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            if let Err(error) = child.kill() {
                eprintln!("REPL cleanup kill: {error}");
            }
            if let Err(error) = child.wait() {
                eprintln!("REPL cleanup wait: {error}");
            }
        }
    }
}

fn text(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        ..LlmResponse::default()
    }
}
fn question() -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "units",
            "ask_user",
            serde_json::Map::from_iter([("question".to_owned(), json!("Which units?"))]),
        )],
        ..LlmResponse::default()
    }
}

#[test]
fn native_calculation_denied_by_the_process_gate_is_reported_unavailable(
) -> Result<(), Box<dyn Error>> {
    // Given
    let exec = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "calc-1",
            "exec",
            serde_json::Map::from_iter([("command".to_owned(), json!("python3 -c 'print(2+3)'"))]),
        )],
        ..LlmResponse::default()
    };
    let fixture = Fixture::new(vec![exec, text("I could not run the calculation.")])?;
    let mut repl = fixture.start(true)?;
    repl.send("Calculate 2 plus 3")?;
    // When
    let answer = repl.send("approve")?;
    // Then
    assert!(
        answer.contains("Calculation: unavailable — execution was denied"),
        "{answer}"
    );
    assert!(!answer.contains("Verified answer"));
    repl.close()?;
    Ok(())
}

#[test]
fn no_side_effects_repl_blocks_calculation_without_consuming_a_response(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new(vec![text("continued request")])?;
    let before = fs::read(&fixture.responses)?;
    let mut repl = fixture.start(false)?;
    // When
    let blocked = repl.send("Calculate 2 plus 3")?;
    // Then
    assert!(blocked.contains("unavailable"));
    assert!(blocked.contains("/method use general"));
    assert_eq!(fs::read(&fixture.responses)?, before);
    repl.close()?;
    Ok(())
}

#[test]
fn repl_override_continues_the_blocked_goal_and_history_survives_restart(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new(vec![text("continued request")])?;
    let mut repl = fixture.start(false)?;
    repl.send("Calculate 2 plus 3")?;
    repl.send("/method use general")?;
    // When
    let answer = repl.send("continue")?;
    repl.close()?;
    let mut restarted = fixture.start(false)?;
    let history = restarted.send("/method history")?;
    // Then
    assert!(answer.contains("continued request"));
    assert!(history.contains("Calculate 2 plus 3"));
    assert!(history.contains("[superseded]"));
    assert!(history.contains("revision 2"));
    restarted.close()?;
    Ok(())
}

#[test]
fn default_repl_registers_exec_shows_method_checks_and_preserves_pending_question_on_change(
) -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new(vec![
        text("calculated draft"),
        question(),
        text("continued"),
        text("local report"),
    ])?;
    let mut repl = fixture.start(true)?;
    let calculation = repl.send("Calculate 2 plus 3")?;
    assert!(calculation.contains("not verified"));
    assert!(calculation.contains("reproducible_calculation"));
    let image = repl.send("Generate an image of a tree")?;
    assert!(image.contains("unavailable"));
    repl.send("Calculate a length")?;
    // When
    let changed = repl.send("/method use general")?;
    // Then
    assert!(changed.contains("Remaining question: Which units?"));
    assert!(repl.send("continue")?.contains("continued"));
    assert!(repl
        .send("Write a report with sources")?
        .contains("web research unavailable"));
    repl.close()?;
    Ok(())
}
