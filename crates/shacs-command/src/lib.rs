use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandId {
    Stop,
    Restart,
    Status,
    New,
    Permission,
    Goal,
    History,
    Dream,
    DreamLog,
    DreamRestore,
    Help,
}

impl CommandId {
    pub fn canonical(self) -> &'static str {
        match self {
            Self::Stop => "/stop",
            Self::Restart => "/restart",
            Self::Status => "/status",
            Self::New => "/new",
            Self::Permission => "/permission",
            Self::Goal => "/goal",
            Self::History => "/history",
            Self::Dream => "/dream",
            Self::DreamLog => "/dream-log",
            Self::DreamRestore => "/dream-restore",
            Self::Help => "/help",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Priority,
    Exact,
    Prefix,
    Intercept,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedCommand {
    pub id: CommandId,
    pub kind: CommandKind,
    pub matched: String,
    pub raw: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedLoopCommand {
    pub command: LoopCommand,
    pub parsed: ParsedCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCommandSpec {
    pub plugin_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCommandRoute {
    pub plugin_id: String,
    pub name: String,
    pub matched: String,
    pub raw: String,
    pub args: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginCommandRouter {
    routes: BTreeMap<String, PluginCommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext<M = (), S = (), L = ()> {
    pub msg: M,
    pub session: Option<S>,
    pub key: String,
    pub raw: String,
    pub args: String,
    pub loop_state: Option<L>,
}

impl<M, S, L> CommandContext<M, S, L> {
    pub fn new(msg: M, session: Option<S>, key: impl Into<String>, raw: impl Into<String>) -> Self {
        Self {
            msg,
            session,
            key: key.into(),
            raw: raw.into(),
            args: String::new(),
            loop_state: None,
        }
    }

    pub fn with_args(mut self, args: impl Into<String>) -> Self {
        self.args = args.into();
        self
    }

    pub fn with_loop_state(mut self, loop_state: L) -> Self {
        self.loop_state = Some(loop_state);
        self
    }
}

pub trait Handler<M, S, L, O>: Send + Sync {
    fn handle(&self, context: &mut CommandContext<M, S, L>) -> Option<O>;
}

impl<M, S, L, O, F> Handler<M, S, L, O> for F
where
    F: Fn(&mut CommandContext<M, S, L>) -> Option<O> + Send + Sync,
{
    fn handle(&self, context: &mut CommandContext<M, S, L>) -> Option<O> {
        self(context)
    }
}

#[derive(Debug, Clone)]
pub struct CommandRouter {
    priority: BTreeSet<CommandId>,
    exact: BTreeSet<CommandId>,
    prefixes: Vec<(String, CommandId)>,
    intercept: bool,
}

impl Default for CommandRouter {
    fn default() -> Self {
        Self::builtin()
    }
}

impl CommandRouter {
    pub fn new() -> Self {
        Self {
            priority: BTreeSet::new(),
            exact: BTreeSet::new(),
            prefixes: Vec::new(),
            intercept: false,
        }
    }

    pub fn builtin() -> Self {
        let mut router = Self::new();
        for command in [CommandId::Stop, CommandId::Restart, CommandId::Status] {
            router.priority(command);
        }
        for command in [
            CommandId::New,
            CommandId::Permission,
            CommandId::Status,
            CommandId::History,
            CommandId::Goal,
            CommandId::Dream,
            CommandId::DreamLog,
            CommandId::DreamRestore,
            CommandId::Help,
        ] {
            router.exact(command);
        }
        for command in [
            CommandId::History,
            CommandId::Goal,
            CommandId::Permission,
            CommandId::DreamLog,
            CommandId::DreamRestore,
        ] {
            router.prefix(command);
        }
        router
    }

    pub fn priority(&mut self, command: CommandId) {
        self.priority.insert(command);
    }

    pub fn exact(&mut self, command: CommandId) {
        self.exact.insert(command);
    }

    pub fn prefix(&mut self, command: CommandId) {
        self.prefixes
            .push((format!("{} ", command.canonical()), command));
        self.prefixes
            .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(&right.0)));
    }

    pub fn intercept(&mut self) {
        self.intercept = true;
    }

    pub fn is_priority(&self, text: &str) -> bool {
        self.dispatch_priority(text).is_some()
    }

    pub fn is_dispatchable_command(&self, text: &str) -> bool {
        self.dispatch(text).is_some()
    }

    pub fn is_known_command(&self, text: &str) -> bool {
        self.dispatch_priority(text).is_some() || self.dispatch(text).is_some()
    }

    pub fn dispatch_priority(&self, text: &str) -> Option<ParsedCommand> {
        let raw = text.trim();
        let match_key = raw.to_ascii_lowercase();
        let id = command_from_exact(&match_key)?;
        if self.priority.contains(&id) {
            Some(parsed(id, CommandKind::Priority, id.canonical(), raw, ""))
        } else {
            None
        }
    }

    pub fn dispatch(&self, text: &str) -> Option<ParsedCommand> {
        let raw = text.trim();
        let match_key = raw.to_ascii_lowercase();
        if let Some(id) = command_from_exact(&match_key).filter(|id| self.exact.contains(id)) {
            return Some(parsed(id, CommandKind::Exact, id.canonical(), raw, ""));
        }
        for (prefix, id) in &self.prefixes {
            if match_key.starts_with(prefix) {
                let args = raw[prefix.len()..].to_owned();
                return Some(parsed(
                    *id,
                    CommandKind::Prefix,
                    prefix.trim_end(),
                    raw,
                    &args,
                ));
            }
        }
        if self.intercept && match_key.starts_with('/') {
            return Some(parsed(
                CommandId::Help,
                CommandKind::Intercept,
                "/",
                raw,
                "",
            ));
        }
        None
    }
}

impl PluginCommandSpec {
    pub fn new(plugin_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            name: name.into(),
        }
    }

    fn route_key(&self) -> Option<String> {
        plugin_command_route_key(&self.name)
    }
}

impl PluginCommandRouter {
    pub fn new(specs: impl IntoIterator<Item = PluginCommandSpec>) -> Self {
        let mut routes = BTreeMap::new();
        for spec in specs {
            let Some(route_key) = spec.route_key() else {
                continue;
            };
            if is_builtin_command_key(&route_key) {
                continue;
            }
            routes.entry(route_key).or_insert(spec);
        }
        Self { routes }
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub fn dispatch(&self, text: &str) -> Option<PluginCommandRoute> {
        let raw = text.trim();
        let (token, args) = split_first_token(raw)?;
        let route_key = plugin_command_route_key(token)?;
        let spec = self.routes.get(&route_key)?;
        Some(PluginCommandRoute {
            plugin_id: spec.plugin_id.clone(),
            name: spec.name.clone(),
            matched: route_key,
            raw: raw.to_owned(),
            args: args.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopCommand {
    Restart,
    Status,
    New,
    Stop,
    Permission(PermissionCommandArgs),
    Goal(GoalCommandArgs),
    History(HistoryCommandArgs),
    Dream,
    DreamLog { sha: Option<String> },
    DreamRestore { sha: Option<String> },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryCommandArgs {
    Count(usize),
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCommandArgs {
    ModeWizard,
    Recent,
    RecentRetry(String),
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalCommandArgs {
    Status,
    Set(String),
    Pause,
    Resume,
    Clear,
    Done,
    Blocked(String),
    Invalid,
}

pub fn parse_loop_command(content: &str) -> Option<LoopCommand> {
    parse_loop_command_route(content).map(|route| route.command)
}

pub fn parse_loop_command_route(content: &str) -> Option<RoutedLoopCommand> {
    let router = CommandRouter::builtin();
    if let Some(command) = router.dispatch_priority(content) {
        return loop_command_from_parsed(command)
            .filter(|route| route.parsed.kind == CommandKind::Priority);
    }
    loop_command_from_parsed(router.dispatch(content)?)
}

fn loop_command_from_parsed(parsed: ParsedCommand) -> Option<RoutedLoopCommand> {
    let command = match parsed.id {
        CommandId::Stop => Some(LoopCommand::Stop),
        CommandId::Restart => Some(LoopCommand::Restart),
        CommandId::Status => Some(LoopCommand::Status),
        _ if parsed.kind == CommandKind::Priority => None,
        CommandId::New => Some(LoopCommand::New),
        CommandId::Permission => Some(LoopCommand::Permission(parse_permission_args(&parsed.args))),
        CommandId::Goal => Some(LoopCommand::Goal(parse_goal_args(&parsed.args))),
        CommandId::History => Some(LoopCommand::History(parse_history_args(&parsed.args))),
        CommandId::Dream => Some(LoopCommand::Dream),
        CommandId::DreamLog => Some(LoopCommand::DreamLog {
            sha: parse_optional_sha(&parsed.args),
        }),
        CommandId::DreamRestore => Some(LoopCommand::DreamRestore {
            sha: parse_optional_sha(&parsed.args),
        }),
        CommandId::Help => Some(LoopCommand::Help),
    }?;
    Some(RoutedLoopCommand { command, parsed })
}

fn parse_goal_args(args: &str) -> GoalCommandArgs {
    let rest = args.trim();
    if rest.is_empty() || rest.eq_ignore_ascii_case("status") {
        return GoalCommandArgs::Status;
    }
    for (keyword, command) in [
        ("pause", GoalCommandArgs::Pause),
        ("resume", GoalCommandArgs::Resume),
        ("clear", GoalCommandArgs::Clear),
        ("done", GoalCommandArgs::Done),
    ] {
        if rest.eq_ignore_ascii_case(keyword) {
            return command;
        }
    }
    let Some((first, blocked_reason)) = split_first_token(rest) else {
        return GoalCommandArgs::Invalid;
    };
    if first.eq_ignore_ascii_case("blocked") {
        if blocked_reason.trim().is_empty() {
            GoalCommandArgs::Invalid
        } else {
            GoalCommandArgs::Blocked(blocked_reason.trim().to_owned())
        }
    } else {
        GoalCommandArgs::Set(rest.to_owned())
    }
}

fn parse_history_args(args: &str) -> HistoryCommandArgs {
    let rest = args.trim();
    if rest.is_empty() {
        return HistoryCommandArgs::Count(10);
    }
    let Ok(count) = rest.parse::<usize>() else {
        return HistoryCommandArgs::Invalid;
    };
    if count == 0 {
        HistoryCommandArgs::Invalid
    } else {
        HistoryCommandArgs::Count(count.min(50))
    }
}

fn parse_permission_args(args: &str) -> PermissionCommandArgs {
    let rest = args.trim();
    if rest.is_empty() {
        return PermissionCommandArgs::ModeWizard;
    }
    if rest.eq_ignore_ascii_case("recent") {
        PermissionCommandArgs::Recent
    } else if let Some((first, after_first)) = split_first_token(rest) {
        if !first.eq_ignore_ascii_case("recent") {
            return PermissionCommandArgs::Invalid;
        }
        let Some((second, denial_id)) = split_first_token(after_first) else {
            return PermissionCommandArgs::Invalid;
        };
        if second.eq_ignore_ascii_case("retry") && denial_id.split_whitespace().count() == 1 {
            PermissionCommandArgs::RecentRetry(denial_id.to_owned())
        } else {
            PermissionCommandArgs::Invalid
        }
    } else {
        PermissionCommandArgs::Invalid
    }
}

fn parse_optional_sha(args: &str) -> Option<String> {
    args.split_whitespace().next().map(str::to_owned)
}

pub fn is_builtin_command(content: &str) -> bool {
    CommandRouter::builtin().is_known_command(content)
}

pub fn is_builtin_command_name(name: &str) -> bool {
    plugin_command_route_key(name).is_some_and(|key| is_builtin_command_key(&key))
}

pub fn normalize_channel_command(content: &str, bot_name: Option<&str>) -> String {
    let trimmed = content.trim();
    let Some((first, rest)) = split_first_token(trimmed) else {
        return String::new();
    };
    let mut token = first.to_owned();
    if let Some(at_index) = token.find('@') {
        let suffix = &token[at_index + 1..];
        if bot_name
            .map(|name| suffix.eq_ignore_ascii_case(name.trim_start_matches('@')))
            .unwrap_or(true)
        {
            token.truncate(at_index);
        }
    }
    token = match token.as_str() {
        "/dream_log" => "/dream-log".to_owned(),
        "/dream_restore" => "/dream-restore".to_owned(),
        other => other.to_owned(),
    };
    if rest.is_empty() {
        token
    } else {
        format!("{token} {rest}")
    }
}

pub fn build_help_text() -> String {
    [
        "🦈 shacs-bot commands:",
        "/new — Stop current task and start a new conversation",
        "/stop — Stop the current task",
        "/restart — Restart the bot",
        "/status — Show bot status",
        "/permission — Change permissions.mode for subsequent turns",
        "/permission recent — Show recent auto-mode classifier denials",
        "/permission recent retry <denial_id> — Request one-shot approval for a recent denial while the process-local retry token is available",
        "/goal [status|pause|resume|clear|done|blocked <reason>|<text>] — Manage the persistent goal",
        "/history [n] — Show the last N conversation messages (default 10)",
        "/dream — Manually trigger Dream consolidation",
        "/dream-log — Show what the last Dream changed",
        "/dream-restore — Revert memory to a previous state",
        "/help — Show available commands",
    ]
    .join("\n")
}

fn parsed(id: CommandId, kind: CommandKind, matched: &str, raw: &str, args: &str) -> ParsedCommand {
    ParsedCommand {
        id,
        kind,
        matched: matched.to_owned(),
        raw: raw.to_owned(),
        args: args.to_owned(),
    }
}

fn command_from_exact(raw: &str) -> Option<CommandId> {
    match raw {
        "/stop" => Some(CommandId::Stop),
        "/restart" => Some(CommandId::Restart),
        "/status" => Some(CommandId::Status),
        "/new" => Some(CommandId::New),
        "/permission" => Some(CommandId::Permission),
        "/goal" => Some(CommandId::Goal),
        "/history" => Some(CommandId::History),
        "/dream" => Some(CommandId::Dream),
        "/dream-log" => Some(CommandId::DreamLog),
        "/dream-restore" => Some(CommandId::DreamRestore),
        "/help" => Some(CommandId::Help),
        _ => None,
    }
}

fn is_builtin_command_key(key: &str) -> bool {
    command_from_exact(key).is_some()
}

fn plugin_command_route_key(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    let command = if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    };
    let command = command.to_ascii_lowercase();
    if command == "/" || command[1..].contains('/') {
        return None;
    }
    Some(command)
}

fn split_first_token(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(index) = trimmed.find(char::is_whitespace) {
        let (first, rest) = trimmed.split_at(index);
        Some((first, rest.trim_start()))
    } else {
        Some((trimmed, ""))
    }
}
