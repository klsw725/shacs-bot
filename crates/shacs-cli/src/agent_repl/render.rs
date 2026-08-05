use shacs_core::runtime::AgentLoopCommandResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplTurnOutcome {
    pub content: String,
    pub stop_reason: String,
    pub command: Option<AgentLoopCommandResult>,
}

pub fn welcome() -> &'static str {
    "shacs-bot agent REPL\nType /help for commands. Ctrl-D exits. Ctrl-C interrupts; repeat to exit."
}

pub fn prompt(active: bool) -> &'static str {
    if active {
        "shacs-bot busy> "
    } else {
        "shacs-bot> "
    }
}

pub fn malformed(raw: &str) -> String {
    if raw.starts_with('/') {
        format!("Command not recognized: {raw}\nType /help for commands.")
    } else {
        raw.to_owned()
    }
}

pub fn queued(message: &str) -> String {
    format!("Follow-up pending: {message}")
}

pub fn stop_requested() -> &'static str {
    "Interrupt received: /stop requested for the active turn."
}

pub fn eof() -> &'static str {
    "REPL closed."
}

pub fn turn(outcome: &ReplTurnOutcome) -> String {
    let state = if outcome.command.is_some() {
        "command"
    } else {
        "turn"
    };
    let mut lines = vec![format!(
        "Projection: kind={state} status={} ",
        outcome.stop_reason
    )];
    if let Some(command) = &outcome.command {
        lines.push(format!("Command: {command:?}"));
    }
    if !outcome.content.trim().is_empty() {
        lines.push(outcome.content.trim().to_owned());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_uses_projection_vocabulary_without_session_ids() {
        let output = turn(&ReplTurnOutcome {
            content: "hello".to_owned(),
            stop_reason: "completed".to_owned(),
            command: None,
        });
        assert!(output.contains("kind=turn"));
        assert!(output.contains("status=completed"));
        assert!(!output.contains("cli:direct"));
    }
}
