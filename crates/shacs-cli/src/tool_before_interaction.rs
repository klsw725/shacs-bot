use shacs_core::runtime::{
    HeadlessToolBeforeInteraction, ToolBeforeConfirmRequest, ToolBeforeConfirmation,
    ToolBeforeInteraction, ToolBeforeNotifyRequest, ToolBeforeSelectRequest,
};
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;

pub(crate) trait ToolBeforePromptIo: Send + Sync {
    fn prompt(&self, prompt: &str) -> String;

    fn notify(&self, message: &str);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolBeforeInvocation {
    pub channel: String,
    pub chat_id: String,
    pub interactive: bool,
}

#[derive(Debug, Default)]
struct StdioToolBeforePromptIo;

impl ToolBeforePromptIo for StdioToolBeforePromptIo {
    fn prompt(&self, prompt: &str) -> String {
        eprint!("{prompt} ");
        let _ = io::stderr().flush();
        let mut reply = String::new();
        let _ = io::stdin().read_line(&mut reply);
        reply.trim().to_owned()
    }

    fn notify(&self, message: &str) {
        eprintln!("{message}");
    }
}

struct InteractiveToolBeforeInteraction {
    invocation: ToolBeforeInvocation,
    io: Arc<dyn ToolBeforePromptIo>,
}

impl ToolBeforeInteraction for InteractiveToolBeforeInteraction {
    fn confirm(&self, request: &ToolBeforeConfirmRequest) -> ToolBeforeConfirmation {
        let reply = self.io.prompt(&format!(
            "[{}:{}] {} [y/N]",
            self.invocation.channel, self.invocation.chat_id, request.prompt
        ));
        if matches!(reply.to_ascii_lowercase().as_str(), "y" | "yes") {
            ToolBeforeConfirmation::Confirmed
        } else {
            ToolBeforeConfirmation::Denied
        }
    }

    fn select(&self, request: &ToolBeforeSelectRequest) -> Option<String> {
        let options = request
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| format!("{}. {option}", index + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let reply = self.io.prompt(&format!(
            "[{}:{}] {}\n{options}\nSelection:",
            self.invocation.channel, self.invocation.chat_id, request.prompt
        ));
        let index = reply.parse::<usize>().ok()?.checked_sub(1)?;
        request.options.get(index).cloned()
    }

    fn notify(&self, request: &ToolBeforeNotifyRequest) {
        self.io.notify(&request.message);
    }
}

pub(crate) fn interaction_for_invocation(
    invocation: &ToolBeforeInvocation,
    io: Arc<dyn ToolBeforePromptIo>,
) -> Arc<dyn ToolBeforeInteraction> {
    if invocation.interactive {
        Arc::new(InteractiveToolBeforeInteraction {
            invocation: invocation.clone(),
            io,
        })
    } else {
        Arc::new(HeadlessToolBeforeInteraction)
    }
}

pub(crate) fn production_interaction(
    channel: &str,
    chat_id: &str,
) -> Arc<dyn ToolBeforeInteraction> {
    interaction_for_invocation(
        &ToolBeforeInvocation {
            channel: channel.to_owned(),
            chat_id: chat_id.to_owned(),
            interactive: channel == "cli"
                && io::stdin().is_terminal()
                && io::stderr().is_terminal(),
        },
        Arc::new(StdioToolBeforePromptIo),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct FakePromptIo {
        replies: Mutex<VecDeque<String>>,
        notifications: Mutex<Vec<String>>,
    }

    impl FakePromptIo {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: Mutex::new(replies.iter().map(|reply| (*reply).to_owned()).collect()),
                notifications: Mutex::new(Vec::new()),
            }
        }
    }

    impl ToolBeforePromptIo for FakePromptIo {
        fn prompt(&self, _prompt: &str) -> String {
            self.replies
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .pop_front()
                .unwrap_or_default()
        }

        fn notify(&self, message: &str) {
            self.notifications
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(message.to_owned());
        }
    }

    fn invocation(interactive: bool, channel: &str) -> ToolBeforeInvocation {
        ToolBeforeInvocation {
            channel: channel.to_owned(),
            chat_id: "direct".to_owned(),
            interactive,
        }
    }

    #[test]
    fn spec030_tool_before_production_interactive_port_allows_current_call() {
        let interaction = interaction_for_invocation(
            &invocation(true, "cli"),
            Arc::new(FakePromptIo::new(&["yes"])),
        );

        let decision = interaction.confirm(&ToolBeforeConfirmRequest {
            call_id: "allow-call".to_owned(),
            prompt: "continue?".to_owned(),
        });

        assert_eq!(decision, ToolBeforeConfirmation::Confirmed);
    }

    #[test]
    fn spec030_tool_before_production_interactive_port_denies_current_call() {
        let interaction = interaction_for_invocation(
            &invocation(true, "cli"),
            Arc::new(FakePromptIo::new(&["no"])),
        );

        let decision = interaction.confirm(&ToolBeforeConfirmRequest {
            call_id: "deny-call".to_owned(),
            prompt: "continue?".to_owned(),
        });

        assert_eq!(decision, ToolBeforeConfirmation::Denied);
    }

    #[test]
    fn spec030_tool_before_production_headless_port_denies_without_prompt() {
        let interaction = interaction_for_invocation(
            &invocation(false, "api"),
            Arc::new(FakePromptIo::new(&["yes"])),
        );

        let decision = interaction.confirm(&ToolBeforeConfirmRequest {
            call_id: "headless-call".to_owned(),
            prompt: "continue?".to_owned(),
        });

        assert_eq!(decision, ToolBeforeConfirmation::HeadlessDenied);
    }

    #[test]
    fn spec030_tool_before_production_interaction_selects_and_notifies() {
        let io = Arc::new(FakePromptIo::new(&["2"]));
        let interaction = interaction_for_invocation(&invocation(true, "cli"), io.clone());

        let selected = interaction.select(&ToolBeforeSelectRequest {
            call_id: "select-call".to_owned(),
            prompt: "choose".to_owned(),
            options: vec!["one".to_owned(), "two".to_owned()],
        });
        interaction.notify(&ToolBeforeNotifyRequest {
            call_id: "notify-call".to_owned(),
            message: "ready".to_owned(),
        });

        assert_eq!(selected.as_deref(), Some("two"));
        assert_eq!(
            *io.notifications
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            vec!["ready"]
        );
    }
}
