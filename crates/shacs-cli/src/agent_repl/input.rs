use shacs_command::{parse_loop_command_route, CommandRouter, LoopCommand};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplInput {
    Empty,
    Turn(String),
    Command(String),
    MalformedSlash(String),
    Eof,
    Interrupt,
}

pub fn parse_line(line: &str) -> ReplInput {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ReplInput::Empty;
    }
    let router = CommandRouter::builtin();
    if router.dispatch_priority(trimmed).is_some() || router.dispatch(trimmed).is_some() {
        return ReplInput::Command(trimmed.to_owned());
    }
    if trimmed.starts_with('/') {
        ReplInput::MalformedSlash(trimmed.to_owned())
    } else {
        ReplInput::Turn(trimmed.to_owned())
    }
}

pub fn is_priority_command(line: &str) -> bool {
    CommandRouter::builtin().dispatch_priority(line).is_some()
}

pub fn is_stop_command(line: &str) -> bool {
    parse_loop_command_route(line).is_some_and(|route| route.command == LoopCommand::Stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_prefix_and_malformed_slash_are_classified_by_router() {
        assert_eq!(
            parse_line("/status"),
            ReplInput::Command("/status".to_owned())
        );
        assert_eq!(
            parse_line("/history 3"),
            ReplInput::Command("/history 3".to_owned())
        );
        assert_eq!(
            parse_line("/unknown command"),
            ReplInput::MalformedSlash("/unknown command".to_owned())
        );
        assert!(is_priority_command("/stop"));
        assert!(!is_priority_command("/history 3"));
    }
}
