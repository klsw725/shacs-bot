mod driver;
mod input;
mod render;
mod state;

use std::io;
use std::sync::Arc;

use shacs_core::runtime::{InboundMessage, SessionTurnReservation};

use crate::{
    cli_session_key, direct_message_invocation, guard_runtime_durable_recovery_admission,
    invocation_text, load_runtime_config, render_direct_turn_content,
    AgentLoopChatCompletionAdapter, AgentReplOptions, AskOptions, CliError, RuntimeConfigOptions,
};

use self::driver::{ReplTurnExecutor, ReplTurnPermit};
use self::render::ReplTurnOutcome;

struct CliReplExecutor {
    adapter: AgentLoopChatCompletionAdapter,
    options: AgentReplOptions,
}

struct CliReplTurnPermit {
    reservation: SessionTurnReservation,
}

impl ReplTurnPermit for CliReplTurnPermit {
    fn bind_to_current_thread(&self) {
        self.reservation.bind_to_current_thread();
    }
}

impl ReplTurnExecutor for CliReplExecutor {
    fn reserve_turn(&self, _input: &str) -> Result<Box<dyn ReplTurnPermit>, CliError> {
        Ok(Box::new(CliReplTurnPermit {
            reservation: self
                .adapter
                .session_turn_lock
                .reserve(cli_session_key(self.options.session.as_deref())),
        }))
    }

    fn execute(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.adapter.process_cli_repl_turn(&self.options, input)
    }

    fn execute_priority(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.adapter
            .process_cli_repl_priority_turn(&self.options, input)
    }
}

pub fn run_agent_repl(options: AgentReplOptions) -> Result<String, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    guard_runtime_durable_recovery_admission(&bundle.context.data_dir)?;
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(bundle, options.allow_side_effects)?;
    let executor = Arc::new(CliReplExecutor { adapter, options });
    let mut stdout = io::stdout();
    driver::run_stdio(&mut stdout, executor)?;
    Ok(String::new())
}

pub(crate) fn repl_turn_options(options: &AgentReplOptions, input: &str) -> AskOptions {
    AskOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        message: input.to_owned(),
        session: options.session.clone(),
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        allow_side_effects: options.allow_side_effects,
        markdown: options.markdown,
    }
}

pub(crate) fn execute_repl_turn(
    adapter: &AgentLoopChatCompletionAdapter,
    options: &AgentReplOptions,
    input: &str,
) -> Result<ReplTurnOutcome, CliError> {
    let ask_options = repl_turn_options(options, input);
    let mut invocation = direct_message_invocation(&adapter.configured_model, &ask_options)?;
    invocation.session_key = cli_session_key(options.session.as_deref());
    let mut config = adapter.loop_config();
    config.permission_interactive = true;
    config.settings.temperature = invocation
        .temperature
        .unwrap_or(config.settings.temperature);
    config.settings.max_tokens = invocation.max_tokens.unwrap_or(config.settings.max_tokens);
    let message = InboundMessage::new("cli", "user", "repl", invocation_text(&invocation))
        .with_media(invocation.media_paths.clone())
        .with_session_key_override(invocation.session_key.clone());
    let (turn, outbound) = adapter.process_inbound_with_outbound(message, config, None, &[])?;
    let content = render_direct_turn_content(turn.final_content.unwrap_or_default(), outbound);
    Ok(ReplTurnOutcome {
        content,
        stop_reason: turn.stop_reason,
        command: turn.command,
    })
}

impl AgentLoopChatCompletionAdapter {
    pub(crate) fn process_cli_repl_turn(
        &self,
        options: &AgentReplOptions,
        input: &str,
    ) -> Result<ReplTurnOutcome, CliError> {
        execute_repl_turn(self, options, input)
    }

    pub(crate) fn process_cli_repl_priority_turn(
        &self,
        options: &AgentReplOptions,
        input: &str,
    ) -> Result<ReplTurnOutcome, CliError> {
        execute_repl_turn(self, options, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shacs_command::{parse_loop_command_route, CommandKind, LoopCommand};

    #[test]
    fn repl_command_classification_matches_shared_router_outcomes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for input in [
            "/status",
            "/stop",
            "/restart",
            "/history 2",
            "/permission recent",
        ] {
            let routed = parse_loop_command_route(input).ok_or("missing shared route")?;
            assert!(matches!(
                crate::agent_repl::input::parse_line(input),
                crate::agent_repl::input::ReplInput::Command(_)
            ));
            if matches!(
                routed.command,
                LoopCommand::Stop | LoopCommand::Restart | LoopCommand::Status
            ) {
                assert_eq!(routed.parsed.kind, CommandKind::Priority);
            }
        }
        Ok(())
    }

    #[test]
    fn repl_turn_options_preserve_cli_session_boundary() {
        let options = AgentReplOptions {
            session: Some("work".to_owned()),
            temperature: Some(0.4),
            max_tokens: Some(128),
            ..AgentReplOptions::default()
        };
        let turn = repl_turn_options(&options, "hello");
        assert_eq!(turn.message, "hello");
        assert_eq!(turn.session.as_deref(), Some("work"));
        assert_eq!(turn.temperature, Some(0.4));
        assert_eq!(turn.max_tokens, Some(128));
    }
}
