use std::collections::VecDeque;

use crate::agent_repl::input::{is_priority_command, is_stop_command, ReplInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplAction {
    None,
    StartTurn(String),
    RunPriority(String),
    QueueFollowUp(String),
    Malformed(String),
    RequestStop,
    RequestStopAndExit,
    Exit,
}

#[derive(Debug, Default)]
pub struct ReplState {
    active: bool,
    pending: VecDeque<String>,
    interrupted: bool,
    close_after_active: bool,
    stop_requested: bool,
}

impl ReplState {
    pub fn handle_input(&mut self, input: ReplInput) -> ReplAction {
        match input {
            ReplInput::Empty => ReplAction::None,
            ReplInput::Eof => self.handle_eof(),
            ReplInput::Interrupt => self.handle_interrupt(),
            ReplInput::MalformedSlash(raw) => ReplAction::Malformed(raw),
            ReplInput::Turn(raw) | ReplInput::Command(raw) => self.handle_text(raw),
        }
    }

    pub fn finish_turn(&mut self) -> Option<String> {
        self.active = false;
        self.interrupted = false;
        self.stop_requested = false;
        if self.close_after_active {
            self.pending.clear();
            return None;
        }
        let next = self.pending.pop_front();
        if next.is_some() {
            self.active = true;
        }
        next
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn should_exit_after_completion(&self) -> bool {
        self.close_after_active && !self.active
    }

    fn handle_text(&mut self, raw: String) -> ReplAction {
        self.interrupted = false;
        if self.active {
            if is_priority_command(&raw) {
                if is_stop_command(&raw) {
                    self.stop_requested = true;
                }
                ReplAction::RunPriority(raw)
            } else {
                self.pending.push_back(raw);
                ReplAction::QueueFollowUp("queued for the current session".to_owned())
            }
        } else {
            self.active = true;
            ReplAction::StartTurn(raw)
        }
    }

    fn handle_interrupt(&mut self) -> ReplAction {
        if self.interrupted {
            if self.active {
                self.close_after_active = true;
                if self.stop_requested {
                    return ReplAction::None;
                }
                self.stop_requested = true;
                return ReplAction::RequestStopAndExit;
            }
            return ReplAction::Exit;
        }
        self.interrupted = true;
        if self.active {
            if self.stop_requested {
                return ReplAction::None;
            }
            self.stop_requested = true;
            ReplAction::RequestStop
        } else {
            ReplAction::Malformed(
                "interrupted input; press Ctrl-C again or send EOF to exit".to_owned(),
            )
        }
    }

    fn handle_eof(&mut self) -> ReplAction {
        if self.active {
            self.close_after_active = true;
            if self.stop_requested {
                return ReplAction::None;
            }
            self.stop_requested = true;
            ReplAction::RequestStopAndExit
        } else {
            ReplAction::Exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_commands_run_while_followups_queue_during_active_turn() {
        let mut state = ReplState::default();
        assert_eq!(
            state.handle_input(ReplInput::Turn("hello".to_owned())),
            ReplAction::StartTurn("hello".to_owned())
        );
        assert_eq!(
            state.handle_input(ReplInput::Turn("next".to_owned())),
            ReplAction::QueueFollowUp("queued for the current session".to_owned())
        );
        assert_eq!(
            state.handle_input(ReplInput::Command("/status".to_owned())),
            ReplAction::RunPriority("/status".to_owned())
        );
        assert_eq!(state.finish_turn(), Some("next".to_owned()));
    }

    #[test]
    fn repeated_interrupt_exits_deterministically() {
        let mut state = ReplState::default();
        assert!(matches!(
            state.handle_input(ReplInput::Interrupt),
            ReplAction::Malformed(_)
        ));
        assert_eq!(state.handle_input(ReplInput::Interrupt), ReplAction::Exit);
    }

    #[test]
    fn eof_during_active_turn_requests_stop_before_exit() {
        let mut state = ReplState::default();
        assert!(matches!(
            state.handle_input(ReplInput::Turn("hello".to_owned())),
            ReplAction::StartTurn(_)
        ));
        assert_eq!(
            state.handle_input(ReplInput::Eof),
            ReplAction::RequestStopAndExit
        );
        assert_eq!(state.finish_turn(), None);
        assert!(state.should_exit_after_completion());
    }
}
