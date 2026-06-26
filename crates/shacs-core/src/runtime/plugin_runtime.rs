use crate::runtime::{
    plugin_hook_catalog, summarize_plugin_hook_dispatch, AgentHook, AgentHookContext,
    DiscoveredPlugin, PluginHookCallbackResult, PluginHookDispatchAttempt,
    PluginHookDispatchSummary, PluginHookEvent, PluginManifestSource, PluginState, RuntimeToolCall,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use shacs_providers::LlmResponse;
use shacs_redaction::redact_string;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

const MAX_HOOK_TIMEOUT_MS: u64 = 30_000;
const MAX_HOOK_STDIO_BYTES: usize = 16 * 1024;
const MAX_HOOK_STDIO_READS_PER_TICK: usize = 4;
const MAX_CONTEXT_PREVIEW_CHARS: usize = 240;
const HOOK_CLEANUP_WAIT_GRACE: Duration = Duration::from_millis(200);
const HOOK_STDIO_DRAIN_GRACE: Duration = Duration::from_millis(50);

pub type PluginHookDispatchSink = Arc<dyn Fn(PluginHookDispatchSummary) + Send + Sync>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRuntimeSnapshot {
    pub plugins: Vec<PluginRuntimePlugin>,
    pub diagnostics: Vec<PluginRuntimeDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRuntimePlugin {
    pub id: String,
    pub root: PathBuf,
    pub manifest_digest: Option<String>,
    pub source: PluginManifestSource,
    pub hooks: Vec<PluginRuntimeHook>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRuntimeHook {
    pub plugin_id: String,
    pub event: PluginHookEvent,
    pub event_name: String,
    pub command: PluginExecutableCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginExecutableCommand {
    pub command_path: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRuntimeDiagnostic {
    pub plugin_id: String,
    pub event: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginHookDispatchMode {
    LiveDiagnostics,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginHookCommandInvocation {
    pub plugin_id: String,
    pub event: PluginHookEvent,
    pub event_name: String,
    pub command: PluginExecutableCommand,
    pub working_dir: PathBuf,
    pub stdin_payload: Value,
}

struct PendingHookStdin {
    stdin: ChildStdin,
    payload: Vec<u8>,
    written: usize,
}

struct PendingHookOutput<R> {
    reader: Option<R>,
    output: Vec<u8>,
}

pub trait PluginHookCommandExecutor: Send + Sync {
    fn execute(&self, invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessPluginHookCommandExecutor;

impl PluginHookCommandExecutor for ProcessPluginHookCommandExecutor {
    fn execute(&self, invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        execute_process_plugin_hook(invocation)
    }
}

#[derive(Clone)]
pub struct PluginRuntimeHookAgentHook {
    snapshot: PluginRuntimeSnapshot,
    mode: PluginHookDispatchMode,
    executor: Arc<dyn PluginHookCommandExecutor>,
    sink: Option<PluginHookDispatchSink>,
}

impl PluginRuntimeHookAgentHook {
    pub fn new(snapshot: PluginRuntimeSnapshot) -> Self {
        Self::with_executor(
            snapshot,
            PluginHookDispatchMode::LiveDiagnostics,
            Arc::new(ProcessPluginHookCommandExecutor),
        )
    }

    pub fn with_executor(
        snapshot: PluginRuntimeSnapshot,
        mode: PluginHookDispatchMode,
        executor: Arc<dyn PluginHookCommandExecutor>,
    ) -> Self {
        Self {
            snapshot,
            mode,
            executor,
            sink: None,
        }
    }

    pub fn with_sink(mut self, sink: PluginHookDispatchSink) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn dispatch_llm_after(
        &self,
        context: &AgentHookContext,
        response: &LlmResponse,
    ) -> Option<PluginHookDispatchSummary> {
        self.dispatch_event(
            PluginHookEvent::LlmAfter,
            llm_after_context_payload(context, response),
        )
    }

    pub fn dispatch_tool_before(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Option<PluginHookDispatchSummary> {
        self.dispatch_event(
            PluginHookEvent::ToolBefore,
            tool_before_context_payload(context, calls),
        )
    }

    fn dispatch_event(
        &self,
        event: PluginHookEvent,
        context_payload: Value,
    ) -> Option<PluginHookDispatchSummary> {
        let mut attempts = Vec::new();
        for plugin in &self.snapshot.plugins {
            for hook in plugin.hooks.iter().filter(|hook| hook.event == event) {
                let result = match self.mode {
                    PluginHookDispatchMode::LiveDiagnostics => {
                        let invocation = hook_invocation(plugin, hook, context_payload.clone());
                        self.executor.execute(&invocation)
                    }
                    PluginHookDispatchMode::Replay => PluginHookCallbackResult::ReplayRejected(
                        "runtime replay does not execute live plugin hook commands".to_owned(),
                    ),
                };
                attempts.push(PluginHookDispatchAttempt {
                    plugin_id: hook.plugin_id.clone(),
                    event,
                    timeout_ms: hook.command.timeout_ms,
                    result,
                });
            }
        }

        if attempts.is_empty() {
            return None;
        }

        let summary = summarize_plugin_hook_dispatch(event, attempts);
        if let Some(sink) = &self.sink {
            sink(summary.clone());
        }
        Some(summary)
    }
}

impl AgentHook for PluginRuntimeHookAgentHook {
    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        let _ = self.dispatch_tool_before(context, calls);
    }

    fn after_response(&self, context: &AgentHookContext, response: &LlmResponse) {
        let _ = self.dispatch_llm_after(context, response);
    }
}

pub fn build_plugin_runtime_snapshot(plugins: &[DiscoveredPlugin]) -> PluginRuntimeSnapshot {
    let mut snapshot = PluginRuntimeSnapshot {
        plugins: Vec::new(),
        diagnostics: Vec::new(),
    };

    for plugin in plugins
        .iter()
        .filter(|plugin| plugin.state == PluginState::Enabled)
    {
        let Some(manifest) = &plugin.manifest else {
            snapshot.diagnostics.push(diagnostic(
                &plugin.id,
                None,
                "missing_manifest",
                "enabled plugin is missing a parsed manifest",
            ));
            continue;
        };

        let mut runtime_plugin = PluginRuntimePlugin {
            id: plugin.id.clone(),
            root: plugin.root.clone(),
            manifest_digest: plugin.digest.clone(),
            source: plugin.source,
            hooks: Vec::new(),
        };

        let declared_hook_events = names_from_surface(&manifest.surfaces, "hooks")
            .into_iter()
            .collect::<BTreeSet<_>>();
        let hooks = manifest.entrypoints.get("hooks");
        let Some(hooks) = hooks else {
            for event_name in &declared_hook_events {
                snapshot.diagnostics.push(diagnostic(
                    &plugin.id,
                    Some(event_name),
                    "missing_hook_entrypoint",
                    &format!("declared plugin hook `{event_name}` has no entrypoint"),
                ));
            }
            snapshot.plugins.push(runtime_plugin);
            continue;
        };

        let Some(hook_map) = hooks.as_object() else {
            snapshot.diagnostics.push(diagnostic(
                &plugin.id,
                None,
                "invalid_hook_entrypoints",
                "plugin hook entrypoints must be an object keyed by hook event",
            ));
            snapshot.plugins.push(runtime_plugin);
            continue;
        };

        for (event_name, entrypoint) in hook_map {
            let Some(event) = hook_event_from_name(event_name) else {
                snapshot.diagnostics.push(diagnostic(
                    &plugin.id,
                    Some(event_name),
                    "unsupported_hook_event",
                    &format!("unsupported plugin hook event `{event_name}`"),
                ));
                continue;
            };
            if !declared_hook_events.contains(event_name) {
                snapshot.diagnostics.push(diagnostic(
                    &plugin.id,
                    Some(event_name),
                    "undeclared_hook_entrypoint",
                    &format!(
                        "plugin hook entrypoint `{event_name}` must be declared in surfaces.hooks"
                    ),
                ));
                continue;
            }

            match parse_hook_command(plugin, event_name, entrypoint) {
                Ok(command) => runtime_plugin.hooks.push(PluginRuntimeHook {
                    plugin_id: plugin.id.clone(),
                    event,
                    event_name: event_name.clone(),
                    command,
                }),
                Err(runtime_diagnostic) => snapshot.diagnostics.push(runtime_diagnostic),
            }
        }

        runtime_plugin.hooks.sort_by(compare_hooks);
        snapshot.plugins.push(runtime_plugin);
    }

    snapshot.plugins.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.root.cmp(&right.root))
    });
    snapshot.diagnostics.sort_by(|left, right| {
        left.plugin_id
            .cmp(&right.plugin_id)
            .then_with(|| left.event.cmp(&right.event))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
    snapshot
}

fn execute_process_plugin_hook(
    invocation: &PluginHookCommandInvocation,
) -> PluginHookCallbackResult {
    let payload = match serde_json::to_vec(&invocation.stdin_payload) {
        Ok(payload) => payload,
        Err(error) => {
            return PluginHookCallbackResult::Error(format!(
                "plugin hook stdin payload serialization failed: {error}"
            ));
        }
    };
    let mut command = Command::new(&invocation.command.command_path);
    command
        .args(&invocation.command.args)
        .current_dir(&invocation.working_dir)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return PluginHookCallbackResult::Error(format!(
                "plugin hook process spawn failed: {error}"
            ));
        }
    };

    let deadline = Instant::now() + Duration::from_millis(invocation.command.timeout_ms);
    let mut stdout = match pending_hook_stdout(&mut child) {
        Ok(stdout) => stdout,
        Err(error) => {
            cleanup_plugin_hook_child(&mut child);
            return PluginHookCallbackResult::Error(error);
        }
    };
    let mut stderr = match pending_hook_stderr(&mut child) {
        Ok(stderr) => stderr,
        Err(error) => {
            cleanup_plugin_hook_child(&mut child);
            return PluginHookCallbackResult::Error(error);
        }
    };
    let (mut pending_stdin, mut stdin_write_error) = pending_hook_stdin(&mut child, payload);
    let status = loop {
        if stdin_write_error.is_none() {
            stdin_write_error = drive_pending_stdin(&mut pending_stdin);
        }
        drain_pending_output(&mut stdout);
        drain_pending_output(&mut stderr);
        match child.try_wait() {
            Ok(Some(status)) => {
                if stdin_write_error.is_none() {
                    stdin_write_error = pending_stdin_incomplete_note(&pending_stdin);
                }
                drop(pending_stdin);
                drain_pending_output(&mut stdout);
                drain_pending_output(&mut stderr);
                drain_pending_outputs_until(
                    &mut stdout,
                    &mut stderr,
                    Instant::now() + HOOK_STDIO_DRAIN_GRACE,
                );
                cleanup_plugin_hook_child_group(child.id());
                drain_pending_output(&mut stdout);
                drain_pending_output(&mut stderr);
                break status;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    if stdin_write_error.is_none() {
                        stdin_write_error = pending_stdin_incomplete_note(&pending_stdin);
                    }
                    drop(pending_stdin);
                    cleanup_plugin_hook_child(&mut child);
                    drain_pending_outputs_until(
                        &mut stdout,
                        &mut stderr,
                        Instant::now() + HOOK_STDIO_DRAIN_GRACE,
                    );
                    let stdout = take_pending_output(stdout);
                    let stderr = take_pending_output(stderr);
                    let mut message = format!(
                        "plugin hook command timed out after {}ms; stdout: {}; stderr: {}",
                        invocation.command.timeout_ms,
                        redacted_bounded_bytes(&stdout),
                        redacted_bounded_bytes(&stderr)
                    );
                    if let Some(error) = stdin_write_error.as_deref() {
                        message.push_str(&format!("; stdin write: {error}"));
                    }
                    return PluginHookCallbackResult::Timeout(message);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                if stdin_write_error.is_none() {
                    stdin_write_error = pending_stdin_incomplete_note(&pending_stdin);
                }
                drop(pending_stdin);
                cleanup_plugin_hook_child(&mut child);
                drain_pending_outputs_until(
                    &mut stdout,
                    &mut stderr,
                    Instant::now() + HOOK_STDIO_DRAIN_GRACE,
                );
                let stdout = take_pending_output(stdout);
                let stderr = take_pending_output(stderr);
                let mut message = format!(
                    "plugin hook process wait failed: {error}; stdout: {}; stderr: {}",
                    redacted_bounded_bytes(&stdout),
                    redacted_bounded_bytes(&stderr)
                );
                if let Some(error) = stdin_write_error.as_deref() {
                    message.push_str(&format!("; stdin write: {error}"));
                }
                return PluginHookCallbackResult::Error(message);
            }
        }
    };

    let stdout_bytes = take_pending_output(stdout);
    let stderr_bytes = take_pending_output(stderr);
    let stdout = redacted_bounded_bytes(&stdout_bytes);
    let stderr = redacted_bounded_bytes(&stderr_bytes);
    if !status.success() {
        let mut message = format!(
            "plugin hook process exited with status {}; stdout: {}; stderr: {}",
            status, stdout, stderr
        );
        if let Some(error) = stdin_write_error.as_deref() {
            message.push_str(&format!("; stdin write: {error}"));
        }
        return PluginHookCallbackResult::Error(message);
    }
    match serde_json::from_slice::<Value>(&stdout_bytes) {
        Ok(value) => PluginHookCallbackResult::Output(value),
        Err(error) => {
            let mut message = format!(
                "plugin hook stdout was not valid JSON: {error}; stdout: {}; stderr: {}",
                stdout, stderr
            );
            if let Some(error) = stdin_write_error.as_deref() {
                message.push_str(&format!("; stdin write: {error}"));
            }
            PluginHookCallbackResult::Error(message)
        }
    }
}

fn pending_hook_stdout(
    child: &mut Child,
) -> Result<Option<PendingHookOutput<ChildStdout>>, String> {
    let Some(stdout) = child.stdout.take() else {
        return Ok(None);
    };
    set_nonblocking_stdout(&stdout)
        .map_err(|error| format!("plugin hook stdout nonblocking setup failed: {error}"))?;
    Ok(Some(PendingHookOutput {
        reader: Some(stdout),
        output: Vec::new(),
    }))
}

fn pending_hook_stderr(
    child: &mut Child,
) -> Result<Option<PendingHookOutput<ChildStderr>>, String> {
    let Some(stderr) = child.stderr.take() else {
        return Ok(None);
    };
    set_nonblocking_stderr(&stderr)
        .map_err(|error| format!("plugin hook stderr nonblocking setup failed: {error}"))?;
    Ok(Some(PendingHookOutput {
        reader: Some(stderr),
        output: Vec::new(),
    }))
}

fn pending_hook_stdin(
    child: &mut Child,
    payload: Vec<u8>,
) -> (Option<PendingHookStdin>, Option<String>) {
    let Some(stdin) = child.stdin.take() else {
        return (None, None);
    };
    if let Err(error) = set_nonblocking_stdin(&stdin) {
        return (
            None,
            Some(format!(
                "plugin hook stdin nonblocking setup failed: {error}"
            )),
        );
    }
    (
        Some(PendingHookStdin {
            stdin,
            payload,
            written: 0,
        }),
        None,
    )
}

fn drive_pending_stdin(pending: &mut Option<PendingHookStdin>) -> Option<String> {
    let mut state = pending.take()?;
    while state.written < state.payload.len() {
        match state.stdin.write(&state.payload[state.written..]) {
            Ok(0) => {
                return Some("plugin hook stdin write made no progress".to_owned());
            }
            Ok(count) => state.written += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                *pending = Some(state);
                return None;
            }
            Err(error) if error.kind() == ErrorKind::BrokenPipe => {
                return Some(format!(
                    "plugin hook stdin closed before payload was fully written: {error}"
                ));
            }
            Err(error) => {
                return Some(format!("plugin hook stdin write failed: {error}"));
            }
        }
    }
    None
}

fn pending_stdin_incomplete_note(pending: &Option<PendingHookStdin>) -> Option<String> {
    pending.as_ref().map(|pending| {
        format!(
            "plugin hook stdin write incomplete: wrote {} of {} bytes",
            pending.written,
            pending.payload.len()
        )
    })
}

fn drain_pending_output<R: Read>(pending: &mut Option<PendingHookOutput<R>>) {
    let Some(state) = pending.as_mut() else {
        return;
    };
    let Some(reader) = state.reader.as_mut() else {
        return;
    };
    let mut buffer = [0_u8; 4096];
    for _ in 0..MAX_HOOK_STDIO_READS_PER_TICK {
        match reader.read(&mut buffer) {
            Ok(0) => {
                state.reader = None;
                return;
            }
            Ok(count) => {
                if state.output.len() < MAX_HOOK_STDIO_BYTES {
                    let remaining = MAX_HOOK_STDIO_BYTES - state.output.len();
                    state
                        .output
                        .extend_from_slice(&buffer[..count.min(remaining)]);
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => return,
            Err(_) => {
                state.reader = None;
                return;
            }
        }
    }
}

fn take_pending_output<R>(pending: Option<PendingHookOutput<R>>) -> Vec<u8> {
    pending.map(|pending| pending.output).unwrap_or_default()
}

fn drain_pending_outputs_until<L: Read, R: Read>(
    stdout: &mut Option<PendingHookOutput<L>>,
    stderr: &mut Option<PendingHookOutput<R>>,
    deadline: Instant,
) {
    loop {
        drain_pending_output(stdout);
        drain_pending_output(stderr);
        if pending_output_closed(stdout) && pending_output_closed(stderr) {
            return;
        }
        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn pending_output_closed<R>(pending: &Option<PendingHookOutput<R>>) -> bool {
    pending
        .as_ref()
        .and_then(|pending| pending.reader.as_ref())
        .is_none()
}

#[cfg(unix)]
fn set_nonblocking_stdio<T: AsRawFd>(stdio: &T) -> std::io::Result<()> {
    let fd = stdio.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn set_nonblocking_stdin(stdin: &ChildStdin) -> std::io::Result<()> {
    set_nonblocking_stdio(stdin)
}

#[cfg(unix)]
fn set_nonblocking_stdout(stdout: &ChildStdout) -> std::io::Result<()> {
    set_nonblocking_stdio(stdout)
}

#[cfg(unix)]
fn set_nonblocking_stderr(stderr: &ChildStderr) -> std::io::Result<()> {
    set_nonblocking_stdio(stderr)
}

#[cfg(not(unix))]
fn set_nonblocking_stdin(_stdin: &ChildStdin) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Unsupported,
        "plugin hook process stdio requires nonblocking pipe support",
    ))
}

#[cfg(not(unix))]
fn set_nonblocking_stdout(_stdout: &ChildStdout) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Unsupported,
        "plugin hook process stdio requires nonblocking pipe support",
    ))
}

#[cfg(not(unix))]
fn set_nonblocking_stderr(_stderr: &ChildStderr) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Unsupported,
        "plugin hook process stdio requires nonblocking pipe support",
    ))
}

fn cleanup_plugin_hook_child(child: &mut Child) {
    let deadline = Instant::now() + HOOK_CLEANUP_WAIT_GRACE;
    loop {
        cleanup_plugin_hook_child_group(child.id());
        let _ = child.kill();
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[cfg(unix)]
fn cleanup_plugin_hook_child_group(child_id: u32) {
    let Ok(pid) = i32::try_from(child_id) else {
        return;
    };
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn cleanup_plugin_hook_child_group(_child_id: u32) {}

fn hook_invocation(
    plugin: &PluginRuntimePlugin,
    hook: &PluginRuntimeHook,
    context_payload: Value,
) -> PluginHookCommandInvocation {
    let working_dir = hook
        .command
        .command_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| plugin.root.clone());
    PluginHookCommandInvocation {
        plugin_id: plugin.id.clone(),
        event: hook.event,
        event_name: hook.event_name.clone(),
        command: hook.command.clone(),
        working_dir,
        stdin_payload: json!({
            "event": hook.event.as_str(),
            "plugin_id": plugin.id,
            "context": context_payload,
        }),
    }
}

fn llm_after_context_payload(context: &AgentHookContext, response: &LlmResponse) -> Value {
    json!({
        "iteration": context.iteration,
        "message_count": context.messages.len(),
        "last_message_role": last_message_role(context),
        "llm": {
            "finish_reason": response.finish_reason,
            "content_chars": response.content.as_ref().map(|content| content.chars().count()),
            "tool_call_count": response.tool_calls.len(),
            "usage_keys": response.usage.keys().cloned().collect::<Vec<_>>(),
        }
    })
}

fn tool_before_context_payload(context: &AgentHookContext, calls: &[RuntimeToolCall]) -> Value {
    let calls = calls
        .iter()
        .map(|call| {
            json!({
                "id": truncate_chars(&redact_string(&call.id), MAX_CONTEXT_PREVIEW_CHARS),
                "name": call.name,
                "argument_keys": argument_keys(&call.arguments),
                "arguments_preview": truncate_chars(
                    &redact_string(&call.arguments.to_string()),
                    MAX_CONTEXT_PREVIEW_CHARS,
                ),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "iteration": context.iteration,
        "message_count": context.messages.len(),
        "last_message_role": last_message_role(context),
        "tools": calls,
    })
}

fn last_message_role(context: &AgentHookContext) -> Option<String> {
    context
        .messages
        .last()
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn argument_keys(arguments: &Value) -> Vec<String> {
    arguments
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn redacted_bounded_bytes(bytes: &[u8]) -> String {
    let bounded = if bytes.len() > MAX_HOOK_STDIO_BYTES {
        &bytes[..MAX_HOOK_STDIO_BYTES]
    } else {
        bytes
    };
    let text = String::from_utf8_lossy(bounded);
    let mut redacted = redact_string(&text);
    if bytes.len() > MAX_HOOK_STDIO_BYTES {
        redacted.push_str("...[truncated]");
    }
    redacted
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("...[truncated]");
    }
    output
}

fn parse_hook_command(
    plugin: &DiscoveredPlugin,
    event_name: &str,
    entrypoint: &Value,
) -> Result<PluginExecutableCommand, PluginRuntimeDiagnostic> {
    let Some(object) = entrypoint.as_object() else {
        return Err(diagnostic(
            &plugin.id,
            Some(event_name),
            "unsupported_hook_entrypoint",
            "plugin hook entrypoint must be an object command declaration; shell strings are not supported",
        ));
    };

    if object.get("shell").is_some() || object.get("backend").is_some() {
        return Err(diagnostic(
            &plugin.id,
            Some(event_name),
            "unsupported_hook_entrypoint",
            "plugin hook entrypoint shell/backend forms are not supported",
        ));
    }

    let Some(command_value) = object.get("command") else {
        return Err(diagnostic(
            &plugin.id,
            Some(event_name),
            "missing_hook_command",
            "plugin hook entrypoint is missing command",
        ));
    };

    let (command, args) = parse_command_and_args(&plugin.id, event_name, object, command_value)?;
    let command_path = safe_command_path(&plugin.root, &command).map_err(|message| {
        diagnostic(
            &plugin.id,
            Some(event_name),
            "unsafe_hook_command",
            &message,
        )
    })?;
    let timeout_ms = parse_timeout_ms(&plugin.id, event_name, object)?;

    Ok(PluginExecutableCommand {
        command_path,
        args,
        timeout_ms,
    })
}

fn parse_command_and_args(
    plugin_id: &str,
    event_name: &str,
    object: &serde_json::Map<String, Value>,
    command_value: &Value,
) -> Result<(String, Vec<String>), PluginRuntimeDiagnostic> {
    match command_value {
        Value::String(command) => Ok((
            command.clone(),
            parse_args(plugin_id, event_name, object.get("args"))?,
        )),
        Value::Array(items) => {
            if object.get("args").is_some() {
                return Err(diagnostic(
                    plugin_id,
                    Some(event_name),
                    "invalid_hook_args",
                    "plugin hook command array must not be combined with args",
                ));
            }
            let Some((first, rest)) = items.split_first() else {
                return Err(diagnostic(
                    plugin_id,
                    Some(event_name),
                    "missing_hook_command",
                    "plugin hook command array must include an executable",
                ));
            };
            let Some(command) = first.as_str() else {
                return Err(diagnostic(
                    plugin_id,
                    Some(event_name),
                    "invalid_hook_command",
                    "plugin hook command array executable must be a string",
                ));
            };
            let mut args = Vec::with_capacity(rest.len());
            for arg in rest {
                let Some(arg) = arg.as_str() else {
                    return Err(diagnostic(
                        plugin_id,
                        Some(event_name),
                        "invalid_hook_args",
                        "plugin hook command array arguments must be strings",
                    ));
                };
                args.push(arg.to_owned());
            }
            Ok((command.to_owned(), args))
        }
        _ => Err(diagnostic(
            plugin_id,
            Some(event_name),
            "invalid_hook_command",
            "plugin hook command must be a string or argv array",
        )),
    }
}

fn parse_args(
    plugin_id: &str,
    event_name: &str,
    args: Option<&Value>,
) -> Result<Vec<String>, PluginRuntimeDiagnostic> {
    let Some(args) = args else {
        return Ok(Vec::new());
    };
    let Some(args) = args.as_array() else {
        return Err(diagnostic(
            plugin_id,
            Some(event_name),
            "invalid_hook_args",
            "plugin hook args must be an array of strings",
        ));
    };
    let mut parsed = Vec::with_capacity(args.len());
    for arg in args {
        let Some(arg) = arg.as_str() else {
            return Err(diagnostic(
                plugin_id,
                Some(event_name),
                "invalid_hook_args",
                "plugin hook args must contain only strings",
            ));
        };
        parsed.push(arg.to_owned());
    }
    Ok(parsed)
}

fn parse_timeout_ms(
    plugin_id: &str,
    event_name: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<u64, PluginRuntimeDiagnostic> {
    let Some(timeout) = object.get("timeoutMs") else {
        return Ok(default_hook_timeout_ms(event_name));
    };
    let Some(timeout) = timeout.as_u64() else {
        return Err(diagnostic(
            plugin_id,
            Some(event_name),
            "invalid_hook_timeout",
            "plugin hook timeoutMs must be a positive integer",
        ));
    };
    if timeout == 0 || timeout > MAX_HOOK_TIMEOUT_MS {
        return Err(diagnostic(
            plugin_id,
            Some(event_name),
            "invalid_hook_timeout",
            &format!("plugin hook timeoutMs must be between 1 and {MAX_HOOK_TIMEOUT_MS}"),
        ));
    }
    Ok(timeout)
}

fn default_hook_timeout_ms(event_name: &str) -> u64 {
    plugin_hook_catalog()
        .entries
        .into_iter()
        .find(|entry| entry.event.as_str() == event_name)
        .map(|entry| entry.timeout_ms)
        .unwrap_or(1_000)
}

fn safe_command_path(root: &Path, command: &str) -> Result<PathBuf, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("plugin hook command must not be empty".to_owned());
    }
    if trimmed.contains('\\') {
        return Err("plugin hook command must not contain backslash path separators".to_owned());
    }

    let command_path = Path::new(trimmed);
    if command_path.is_absolute() {
        return Err("plugin hook command must be relative to the plugin root".to_owned());
    }

    for component in command_path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => {}
            Component::CurDir | Component::ParentDir => {
                return Err(
                    "plugin hook command must not contain . or .. path components".to_owned(),
                );
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("plugin hook command must stay inside the plugin root".to_owned());
            }
            Component::Normal(_) => {
                return Err("plugin hook command must not contain empty path components".to_owned());
            }
        }
    }

    let path = root.join(command_path);
    if !path.starts_with(root) {
        return Err("plugin hook command must stay inside the plugin root".to_owned());
    }
    Ok(path)
}

fn hook_event_from_name(event_name: &str) -> Option<PluginHookEvent> {
    plugin_hook_catalog()
        .entries
        .into_iter()
        .find(|entry| entry.event.as_str() == event_name)
        .map(|entry| entry.event)
}

fn names_from_surface(surfaces: &Value, key: &str) -> Vec<String> {
    surfaces.get(key).map(names_from_value).unwrap_or_default()
}

fn names_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
        Value::Object(object) => object.keys().cloned().collect(),
        Value::String(name) if !name.trim().is_empty() => vec![name.trim().to_owned()],
        _ => Vec::new(),
    }
}

fn compare_hooks(left: &PluginRuntimeHook, right: &PluginRuntimeHook) -> Ordering {
    left.event_name
        .cmp(&right.event_name)
        .then_with(|| left.command.command_path.cmp(&right.command.command_path))
        .then_with(|| left.command.args.cmp(&right.command.args))
}

fn diagnostic(
    plugin_id: &str,
    event: Option<&str>,
    code: &str,
    detail: &str,
) -> PluginRuntimeDiagnostic {
    PluginRuntimeDiagnostic {
        plugin_id: plugin_id.to_owned(),
        event: event.map(str::to_owned),
        code: code.to_owned(),
        message: redact_string(detail),
    }
}
