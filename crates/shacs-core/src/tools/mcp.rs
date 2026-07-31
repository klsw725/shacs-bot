use crate::runtime::{
    ContainmentSnapshotRef, ProcessExecutionReceipt, ProcessGate, ProcessGateInput,
    ProcessRedactedSpawnSummary, ProcessRedactedStatus, ProcessSpawnAuthorization,
    ProcessSpawnReport, ProcessTerminalOutcome,
};
use crate::tools::{JsonMap, Tool, ToolRegistry, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const TRANSIENT_ERROR_NAMES: &[&str] = &[
    "ClosedResourceError",
    "BrokenResourceError",
    "EndOfStream",
    "BrokenPipeError",
    "ConnectionResetError",
    "ConnectionRefusedError",
    "ConnectionAbortedError",
    "ConnectionError",
];

const STDIO_CHILD_TERM_GRACE: Duration = Duration::from_millis(500);
const STDIO_CHILD_KILL_GRACE: Duration = Duration::from_millis(500);
const STDIO_CHILD_WAIT_STEP: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpCapabilityKind {
    Tool,
    Resource,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCapability {
    pub kind: McpCapabilityKind,
    pub server_name: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
    pub uri: Option<String>,
    pub arguments: Vec<McpPromptArgument>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpErrorKind {
    Timeout,
    Cancelled,
    Transient { type_name: String },
    Other { type_name: String },
    Protocol { code: i64, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum McpCallOutcome {
    Success(Vec<String>),
    Error(McpErrorKind),
}

pub trait McpClient: Send + Sync {
    fn call_tool(&self, name: &str, arguments: JsonMap, timeout_seconds: u64) -> McpCallOutcome;
    fn read_resource(&self, uri: &str, timeout_seconds: u64) -> McpCallOutcome;
    fn get_prompt(&self, name: &str, arguments: JsonMap, timeout_seconds: u64) -> McpCallOutcome;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpTransportKind {
    Stdio,
    Sse,
    StreamableHttp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerSpec {
    pub name: String,
    pub r#type: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub clear_env: bool,
    pub url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub timeout_seconds: u64,
    pub enabled_tools: Vec<String>,
    pub parent_containment_snapshot: Option<ContainmentSnapshotRef>,
    pub startup_gate: Option<McpStartupGate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpStartupGate {
    pub input: ProcessGateInput,
}

impl McpServerSpec {
    pub fn transport_kind(&self) -> Option<McpTransportKind> {
        match self
            .r#type
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some("stdio") => return Some(McpTransportKind::Stdio),
            Some("sse") => return Some(McpTransportKind::Sse),
            Some("streamableHttp") | Some("streamable_http") => {
                return Some(McpTransportKind::StreamableHttp)
            }
            Some(_) => return None,
            None => {}
        }
        if self
            .command
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            Some(McpTransportKind::Stdio)
        } else if let Some(url) = self.url.as_deref().filter(|value| !value.trim().is_empty()) {
            if url.trim_end_matches('/').ends_with("/sse") {
                Some(McpTransportKind::Sse)
            } else {
                Some(McpTransportKind::StreamableHttp)
            }
        } else {
            None
        }
    }

    pub fn normalized_stdio_command(&self) -> Option<(String, Vec<String>)> {
        let command = self.command.as_deref()?.trim();
        if command.is_empty() {
            return None;
        }
        let args = self.args.clone();
        if needs_windows_cmd_wrapper(command) {
            let mut wrapped = vec!["/d".to_owned(), "/c".to_owned(), command.to_owned()];
            wrapped.extend(args);
            Some(("cmd.exe".to_owned(), wrapped))
        } else {
            Some((command.to_owned(), args))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerConnectionReport {
    pub server_name: String,
    pub connected: bool,
    pub registered_count: usize,
    pub error: Option<String>,
    pub unmatched_enabled_tools: Vec<String>,
    pub parent_containment_snapshot: Option<ContainmentSnapshotRef>,
    pub startup_receipt: Option<ProcessExecutionReceipt>,
}

pub trait McpConnector: Send + Sync {
    fn connect(
        &self,
        spec: &McpServerSpec,
    ) -> Result<(Arc<dyn McpClient>, Vec<McpCapability>), String>;
    fn startup_receipt(&self, _server_name: &str) -> Option<ProcessExecutionReceipt> {
        None
    }
    fn close(&self, _server_name: &str) {}
}

#[derive(Default)]
pub struct StdioMcpConnector {
    connections: Mutex<HashMap<String, Arc<Mutex<StdioMcpConnection>>>>,
    startup_receipts: Mutex<HashMap<String, ProcessExecutionReceipt>>,
    child_observer: Option<Arc<dyn Fn(u32) + Send + Sync>>,
}

impl StdioMcpConnector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_child_observer(observer: Arc<dyn Fn(u32) + Send + Sync>) -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            startup_receipts: Mutex::new(HashMap::new()),
            child_observer: Some(observer),
        }
    }
}

impl McpConnector for StdioMcpConnector {
    fn connect(
        &self,
        spec: &McpServerSpec,
    ) -> Result<(Arc<dyn McpClient>, Vec<McpCapability>), String> {
        if spec.transport_kind() != Some(McpTransportKind::Stdio) {
            return Err(format!(
                "unsupported MCP transport for `{}`: {:?}",
                spec.name,
                spec.transport_kind()
            ));
        }
        let (command, args) = spec
            .normalized_stdio_command()
            .ok_or_else(|| format!("MCP server `{}` requires a stdio command", spec.name))?;
        let startup_gate = spec
            .startup_gate
            .clone()
            .ok_or_else(|| format!("MCP server `{}` requires a process startup gate", spec.name))?;
        let mut connected = None;
        let receipt = ProcessGate::new()
            .evaluate_and_maybe_spawn(startup_gate.input, |authorization| {
                let result = connect_stdio_child(
                    authorization,
                    spec,
                    &command,
                    &args,
                    self.child_observer.as_deref(),
                );
                let terminal = match &result {
                    Ok(_) => ProcessTerminalOutcome::Succeeded,
                    Err(error) if error.contains("timed out") => ProcessTerminalOutcome::TimedOut,
                    Err(_) => ProcessTerminalOutcome::Failed,
                };
                connected = Some(result);
                ProcessSpawnReport {
                    terminal_outcome: terminal,
                    redacted_summary: mcp_startup_summary(terminal),
                }
            })
            .map_err(|error| error.to_string())?;
        self.startup_receipts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(spec.name.clone(), receipt.clone());
        if receipt.dispatch_count == 0 {
            return Err(format!(
                "MCP server `{}` startup denied: {:?}",
                spec.name, receipt.terminal_outcome
            ));
        }
        let (client, capabilities) = connected.ok_or_else(|| {
            format!(
                "MCP server `{}` startup did not produce a connection",
                spec.name
            )
        })??;
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(spec.name.clone(), client.clone());
        Ok((client, capabilities))
    }

    fn startup_receipt(&self, server_name: &str) -> Option<ProcessExecutionReceipt> {
        self.startup_receipts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(server_name)
            .cloned()
    }

    fn close(&self, server_name: &str) {
        if let Some(connection) = self
            .connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(server_name)
        {
            connection
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .close();
        }
    }
}

fn mcp_startup_summary(terminal: ProcessTerminalOutcome) -> ProcessRedactedSpawnSummary {
    let (code, summary) = match terminal {
        ProcessTerminalOutcome::Succeeded => ("completed_success", "MCP stdio startup completed"),
        ProcessTerminalOutcome::Failed => ("startup_failed", "MCP stdio startup failed"),
        ProcessTerminalOutcome::TimedOut => ("timed_out", "MCP stdio startup timed out"),
        ProcessTerminalOutcome::Denied => ("denied", "MCP stdio startup denied"),
        ProcessTerminalOutcome::ReplaySkipped => {
            ("replay_skipped", "MCP stdio startup replay skipped")
        }
        ProcessTerminalOutcome::Cancelled => ("cancelled", "MCP stdio startup cancelled"),
        ProcessTerminalOutcome::Interrupted => ("interrupted", "MCP stdio startup interrupted"),
    };
    ProcessRedactedSpawnSummary {
        status: Some(ProcessRedactedStatus {
            code: code.to_owned(),
            summary: summary.to_owned(),
        }),
        ..ProcessRedactedSpawnSummary::empty()
    }
}

fn connect_stdio_child(
    authorization: ProcessSpawnAuthorization,
    spec: &McpServerSpec,
    command: &str,
    args: &[String],
    child_observer: Option<&(dyn Fn(u32) + Send + Sync)>,
) -> Result<(Arc<Mutex<StdioMcpConnection>>, Vec<McpCapability>), String> {
    let (child, stdin, stdout) =
        spawn_stdio_child(authorization, spec, command, args, child_observer)?;
    let mut connection = StdioMcpConnection::new(child, stdin, stdout);
    if let Err(error) = connection.initialize(spec.timeout_seconds) {
        connection.close();
        return Err(error);
    }
    let capabilities = match connection.list_capabilities(&spec.name, spec.timeout_seconds) {
        Ok(capabilities) => capabilities,
        Err(error) => {
            connection.close();
            return Err(error);
        }
    };
    Ok((Arc::new(Mutex::new(connection)), capabilities))
}

fn spawn_stdio_child(
    _authorization: ProcessSpawnAuthorization,
    spec: &McpServerSpec,
    command: &str,
    args: &[String],
    child_observer: Option<&(dyn Fn(u32) + Send + Sync)>,
) -> Result<(Child, ChildStdin, ChildStdout), String> {
    let mut command_builder = Command::new(command);
    command_builder.args(args);
    if spec.clear_env {
        command_builder.env_clear();
    }
    StdioChildProcessGroup::configure_command(&mut command_builder);
    let mut child = command_builder
        .envs(spec.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            format!(
                "MCP server `{}` failed to start `{command}`: {error}",
                spec.name
            )
        })?;
    if let Some(observer) = child_observer {
        observer(child.id());
    }
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("MCP server `{}` did not expose stdin", spec.name));
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("MCP server `{}` did not expose stdout", spec.name));
    };
    Ok((child, stdin, stdout))
}

struct StdioMcpConnection {
    child: Option<Child>,
    process_group: StdioChildProcessGroup,
    stdin: Option<ChildStdin>,
    stdout_rx: mpsc::Receiver<io::Result<Value>>,
    stdout_reader: Option<JoinHandle<()>>,
    next_id: u64,
}

impl StdioMcpConnection {
    fn new(child: Child, stdin: ChildStdin, stdout: ChildStdout) -> Self {
        let process_group = StdioChildProcessGroup::from_child(&child);
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let stdout_reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let done = match read_mcp_message(&mut reader) {
                    Ok(value) => stdout_tx.send(Ok(value)).is_err(),
                    Err(error) => {
                        let _ = stdout_tx.send(Err(error));
                        true
                    }
                };
                if done {
                    break;
                }
            }
        });
        Self {
            child: Some(child),
            process_group,
            stdin: Some(stdin),
            stdout_rx,
            stdout_reader: Some(stdout_reader),
            next_id: 1,
        }
    }

    fn initialize(&mut self, timeout_seconds: u64) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "shacs-bot", "version": env!("CARGO_PKG_VERSION")}
            }),
            timeout_seconds,
        )?;
        write_mcp_message(
            self.stdin_mut()?,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .map_err(|error| format!("MCP initialized notification failed: {error}"))?;
        Ok(())
    }

    fn list_capabilities(
        &mut self,
        server_name: &str,
        timeout_seconds: u64,
    ) -> Result<Vec<McpCapability>, String> {
        let mut capabilities = Vec::new();
        match self.request("tools/list", json!({}), timeout_seconds) {
            Ok(result) => capabilities.extend(parse_tool_capabilities(
                server_name,
                timeout_seconds,
                result,
            )),
            Err(error) if error.contains("timed out") => return Err(error),
            Err(_) => {}
        }
        match self.request("resources/list", json!({}), timeout_seconds) {
            Ok(result) => capabilities.extend(parse_resource_capabilities(
                server_name,
                timeout_seconds,
                result,
            )),
            Err(error) if error.contains("timed out") => return Err(error),
            Err(_) => {}
        }
        match self.request("prompts/list", json!({}), timeout_seconds) {
            Ok(result) => capabilities.extend(parse_prompt_capabilities(
                server_name,
                timeout_seconds,
                result,
            )),
            Err(error) if error.contains("timed out") => return Err(error),
            Err(_) => {}
        }
        Ok(capabilities)
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout_seconds: u64,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        write_mcp_message(
            self.stdin_mut()?,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .map_err(|error| format!("MCP request `{method}` write failed: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(timeout_seconds.max(1));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.close();
                return Err(format!("MCP request `{method}` timed out"));
            }
            let response = match self.stdout_rx.recv_timeout(remaining) {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    return Err(format!("MCP request `{method}` read failed: {error}"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.close();
                    return Err(format!("MCP request `{method}` timed out"));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(format!("MCP request `{method}` stream closed"));
                }
            };
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("protocol error");
                return Err(message.to_owned());
            }
            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn call_outcome(
        &mut self,
        method: &str,
        params: Value,
        timeout_seconds: u64,
    ) -> McpCallOutcome {
        match self.request(method, params, timeout_seconds) {
            Ok(result) => McpCallOutcome::Success(parts_from_mcp_result(method, &result)),
            Err(error) => McpCallOutcome::Error(McpErrorKind::Other { type_name: error }),
        }
    }

    fn stdin_mut(&mut self) -> Result<&mut ChildStdin, String> {
        self.stdin
            .as_mut()
            .ok_or_else(|| "MCP stdio connection is closed".to_owned())
    }

    fn close(&mut self) {
        let _stdin = self.stdin.take();
        if let Some(mut child) = self.child.take() {
            self.process_group.terminate(&mut child);
            let _ = wait_child_until(&mut child, STDIO_CHILD_TERM_GRACE);
            self.process_group.force_kill(&mut child);
            let _ = wait_child_until(&mut child, STDIO_CHILD_KILL_GRACE);
            let _ = child.wait();
        }
        if let Some(stdout_reader) = self.stdout_reader.take() {
            let _ = stdout_reader.join();
        }
    }
}

fn wait_child_until(child: &mut Child, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return true,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return false;
                }
                thread::sleep(STDIO_CHILD_WAIT_STEP);
            }
        }
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct StdioChildProcessGroup {
    pgid: nix::unistd::Pid,
}

#[cfg(unix)]
impl StdioChildProcessGroup {
    fn configure_command(command: &mut Command) {
        command.process_group(0);
    }

    fn from_child(child: &Child) -> Self {
        let pgid = i32::try_from(child.id())
            .map(nix::unistd::Pid::from_raw)
            .unwrap_or_else(|_| nix::unistd::Pid::from_raw(0));
        Self { pgid }
    }

    fn terminate(self, child: &mut Child) {
        if !self.signal(nix::sys::signal::Signal::SIGTERM) {
            let _ = child.kill();
        }
    }

    fn force_kill(self, child: &mut Child) {
        if !self.signal(nix::sys::signal::Signal::SIGKILL) {
            let _ = child.kill();
        }
    }

    fn signal(self, signal: nix::sys::signal::Signal) -> bool {
        if self.pgid.as_raw() <= 0 || self.pgid == nix::unistd::getpgrp() {
            return false;
        }
        nix::sys::signal::killpg(self.pgid, signal).is_ok()
    }
}

#[cfg(not(unix))]
#[derive(Debug, Clone, Copy)]
struct StdioChildProcessGroup;

#[cfg(not(unix))]
impl StdioChildProcessGroup {
    fn configure_command(_command: &mut Command) {}

    fn from_child(_child: &Child) -> Self {
        Self
    }

    fn terminate(self, child: &mut Child) {
        let _ = child.kill();
    }

    fn force_kill(self, child: &mut Child) {
        let _ = child.kill();
    }
}

impl Drop for StdioMcpConnection {
    fn drop(&mut self) {
        self.close();
    }
}

impl McpClient for Mutex<StdioMcpConnection> {
    fn call_tool(&self, name: &str, arguments: JsonMap, timeout_seconds: u64) -> McpCallOutcome {
        self.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call_outcome(
                "tools/call",
                json!({"name": name, "arguments": Value::Object(arguments)}),
                timeout_seconds,
            )
    }

    fn read_resource(&self, uri: &str, timeout_seconds: u64) -> McpCallOutcome {
        self.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call_outcome("resources/read", json!({"uri": uri}), timeout_seconds)
    }

    fn get_prompt(&self, name: &str, arguments: JsonMap, timeout_seconds: u64) -> McpCallOutcome {
        self.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call_outcome(
                "prompts/get",
                json!({"name": name, "arguments": Value::Object(arguments)}),
                timeout_seconds,
            )
    }
}

#[derive(Default)]
pub struct McpRuntime {
    connector: Option<Arc<dyn McpConnector>>,
    connected_servers: Mutex<Vec<String>>,
    closed: AtomicBool,
}

impl McpRuntime {
    pub fn new(connector: Option<Arc<dyn McpConnector>>) -> Self {
        Self {
            connector,
            connected_servers: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        }
    }

    pub fn connect_and_register(
        &self,
        registry: &mut ToolRegistry,
        specs: &[McpServerSpec],
    ) -> Vec<McpServerConnectionReport> {
        let mut reports = Vec::new();
        let Some(connector) = self.connector.as_ref() else {
            return specs
                .iter()
                .map(|spec| McpServerConnectionReport {
                    server_name: spec.name.clone(),
                    connected: false,
                    registered_count: 0,
                    error: Some("no MCP connector configured".to_owned()),
                    unmatched_enabled_tools: Vec::new(),
                    parent_containment_snapshot: spec.parent_containment_snapshot.clone(),
                    startup_receipt: None,
                })
                .collect();
        };
        for spec in specs {
            if spec.enabled_tools.is_empty() {
                reports.push(McpServerConnectionReport {
                    server_name: spec.name.clone(),
                    connected: false,
                    registered_count: 0,
                    error: None,
                    unmatched_enabled_tools: Vec::new(),
                    parent_containment_snapshot: spec.parent_containment_snapshot.clone(),
                    startup_receipt: None,
                });
                continue;
            }
            if spec.transport_kind().is_none() {
                reports.push(McpServerConnectionReport {
                    server_name: spec.name.clone(),
                    connected: false,
                    registered_count: 0,
                    error: Some("missing or unsupported MCP transport".to_owned()),
                    unmatched_enabled_tools: Vec::new(),
                    parent_containment_snapshot: spec.parent_containment_snapshot.clone(),
                    startup_receipt: None,
                });
                continue;
            }
            match connector.connect(spec) {
                Ok((client, mut capabilities)) => {
                    for capability in &mut capabilities {
                        capability.server_name = spec.name.clone();
                        capability.timeout_seconds = spec.timeout_seconds;
                    }
                    let report = register_mcp_capabilities(
                        registry,
                        client,
                        capabilities,
                        &spec.enabled_tools,
                    );
                    self.connected_servers
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(spec.name.clone());
                    reports.push(McpServerConnectionReport {
                        server_name: spec.name.clone(),
                        connected: true,
                        registered_count: report.registered_count,
                        error: None,
                        unmatched_enabled_tools: report.unmatched_enabled_tools,
                        parent_containment_snapshot: spec.parent_containment_snapshot.clone(),
                        startup_receipt: connector.startup_receipt(&spec.name),
                    });
                }
                Err(error) => reports.push(McpServerConnectionReport {
                    server_name: spec.name.clone(),
                    connected: false,
                    registered_count: 0,
                    error: Some(error),
                    unmatched_enabled_tools: Vec::new(),
                    parent_containment_snapshot: spec.parent_containment_snapshot.clone(),
                    startup_receipt: connector.startup_receipt(&spec.name),
                }),
            }
        }
        reports
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(connector) = self.connector.as_ref() {
            for server in self
                .connected_servers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
            {
                connector.close(server);
            }
        }
    }
}

impl Drop for McpRuntime {
    fn drop(&mut self) {
        self.close();
    }
}

impl<F> McpClient for F
where
    F: Fn(McpOperation) -> McpCallOutcome + Send + Sync,
{
    fn call_tool(&self, name: &str, arguments: JsonMap, timeout_seconds: u64) -> McpCallOutcome {
        self(McpOperation::CallTool {
            name: name.to_owned(),
            arguments,
            timeout_seconds,
        })
    }

    fn read_resource(&self, uri: &str, timeout_seconds: u64) -> McpCallOutcome {
        self(McpOperation::ReadResource {
            uri: uri.to_owned(),
            timeout_seconds,
        })
    }

    fn get_prompt(&self, name: &str, arguments: JsonMap, timeout_seconds: u64) -> McpCallOutcome {
        self(McpOperation::GetPrompt {
            name: name.to_owned(),
            arguments,
            timeout_seconds,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpOperation {
    CallTool {
        name: String,
        arguments: JsonMap,
        timeout_seconds: u64,
    },
    ReadResource {
        uri: String,
        timeout_seconds: u64,
    },
    GetPrompt {
        name: String,
        arguments: JsonMap,
        timeout_seconds: u64,
    },
}

#[derive(Clone)]
pub struct McpToolWrapper {
    client: Arc<dyn McpClient>,
    capability: McpCapability,
    wrapped_name: String,
    description: String,
    parameters: Value,
}

impl McpToolWrapper {
    pub fn new(client: Arc<dyn McpClient>, capability: McpCapability) -> Self {
        let wrapped_name = wrapped_name(&capability.server_name, "", &capability.name);
        let description = capability
            .description
            .clone()
            .unwrap_or_else(|| capability.name.clone());
        let parameters = normalize_schema_for_openai(
            capability
                .input_schema
                .clone()
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
        );
        Self {
            client,
            capability,
            wrapped_name,
            description,
            parameters,
        }
    }
}

impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.wrapped_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let outcome = retry_transient(|| {
            self.client.call_tool(
                &self.capability.name,
                params.clone(),
                self.capability.timeout_seconds,
            )
        });
        format_tool_outcome(outcome, self.capability.timeout_seconds).into()
    }
}

#[derive(Clone)]
pub struct McpResourceWrapper {
    client: Arc<dyn McpClient>,
    capability: McpCapability,
    wrapped_name: String,
    description: String,
}

impl McpResourceWrapper {
    pub fn new(client: Arc<dyn McpClient>, capability: McpCapability) -> Self {
        let wrapped_name = wrapped_name(&capability.server_name, "resource", &capability.name);
        let desc = capability
            .description
            .clone()
            .unwrap_or_else(|| capability.name.clone());
        let uri = capability.uri.clone().unwrap_or_default();
        Self {
            client,
            capability,
            wrapped_name,
            description: format!("[MCP Resource] {desc}\nURI: {uri}"),
        }
    }
}

impl Tool for McpResourceWrapper {
    fn name(&self) -> &str {
        &self.wrapped_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        let Some(uri) = self.capability.uri.as_deref() else {
            return "(MCP resource read failed: MissingUri)".into();
        };
        let outcome = retry_transient(|| {
            self.client
                .read_resource(uri, self.capability.timeout_seconds)
        });
        format_resource_outcome(outcome, self.capability.timeout_seconds).into()
    }
}

#[derive(Clone)]
pub struct McpPromptWrapper {
    client: Arc<dyn McpClient>,
    capability: McpCapability,
    wrapped_name: String,
    description: String,
    parameters: Value,
}

impl McpPromptWrapper {
    pub fn new(client: Arc<dyn McpClient>, capability: McpCapability) -> Self {
        let wrapped_name = wrapped_name(&capability.server_name, "prompt", &capability.name);
        let desc = capability
            .description
            .clone()
            .unwrap_or_else(|| capability.name.clone());
        let mut properties = Map::new();
        let mut required = Vec::new();
        for argument in &capability.arguments {
            let mut property = Map::new();
            property.insert("type".to_owned(), Value::String("string".to_owned()));
            if let Some(description) = &argument.description {
                property.insert("description".to_owned(), Value::String(description.clone()));
            }
            properties.insert(argument.name.clone(), Value::Object(property));
            if argument.required {
                required.push(Value::String(argument.name.clone()));
            }
        }
        Self {
            client,
            capability,
            wrapped_name,
            description: format!(
                "[MCP Prompt] {desc}\nReturns a filled prompt template that can be used as a workflow guide."
            ),
            parameters: json!({
                "type": "object",
                "properties": properties,
                "required": required,
            }),
        }
    }
}

impl Tool for McpPromptWrapper {
    fn name(&self) -> &str {
        &self.wrapped_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let outcome = retry_transient(|| {
            self.client.get_prompt(
                &self.capability.name,
                params.clone(),
                self.capability.timeout_seconds,
            )
        });
        format_prompt_outcome(outcome, self.capability.timeout_seconds).into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRegistrationReport {
    pub registered_count: usize,
    pub unmatched_enabled_tools: Vec<String>,
}

pub fn register_mcp_capabilities(
    registry: &mut ToolRegistry,
    client: Arc<dyn McpClient>,
    capabilities: Vec<McpCapability>,
    enabled_tools: &[String],
) -> McpRegistrationReport {
    let enabled = enabled_tools.iter().cloned().collect::<HashSet<_>>();
    let allow_all_tools = enabled.contains("*");
    let mut matched_enabled_tools = HashSet::new();
    let mut registered_count = 0;

    for capability in capabilities {
        let wrapped = wrapped_capability_name(&capability);
        if !allow_all_tools && !enabled.contains(&capability.name) && !enabled.contains(&wrapped) {
            continue;
        }
        if enabled.contains(&capability.name) {
            matched_enabled_tools.insert(capability.name.clone());
        }
        if enabled.contains(&wrapped) {
            matched_enabled_tools.insert(wrapped.clone());
        }

        match capability.kind {
            McpCapabilityKind::Tool => {
                registry.register(McpToolWrapper::new(client.clone(), capability));
            }
            McpCapabilityKind::Resource => {
                registry.register(McpResourceWrapper::new(client.clone(), capability));
            }
            McpCapabilityKind::Prompt => {
                registry.register(McpPromptWrapper::new(client.clone(), capability));
            }
        }
        registered_count += 1;
    }

    let mut unmatched_enabled_tools = if allow_all_tools {
        Vec::new()
    } else {
        enabled
            .difference(&matched_enabled_tools)
            .cloned()
            .collect::<Vec<_>>()
    };
    unmatched_enabled_tools.sort();
    McpRegistrationReport {
        registered_count,
        unmatched_enabled_tools,
    }
}

fn wrapped_capability_name(capability: &McpCapability) -> String {
    let kind = match capability.kind {
        McpCapabilityKind::Tool => "",
        McpCapabilityKind::Resource => "resource",
        McpCapabilityKind::Prompt => "prompt",
    };
    wrapped_name(&capability.server_name, kind, &capability.name)
}

pub fn sanitize_mcp_name(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    let mut previous_underscore = false;
    for character in name.chars() {
        let next = if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            character
        } else {
            '_'
        };
        if next == '_' {
            if !previous_underscore {
                sanitized.push(next);
            }
            previous_underscore = true;
        } else {
            sanitized.push(next);
            previous_underscore = false;
        }
    }
    sanitized
}

pub fn is_transient_mcp_error(type_name: &str) -> bool {
    TRANSIENT_ERROR_NAMES.contains(&type_name)
}

fn needs_windows_cmd_wrapper(command: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let command = command.to_ascii_lowercase();
    matches!(command.as_str(), "npx" | "npm" | "pnpm" | "yarn" | "bunx")
        || command.ends_with(".cmd")
        || command.ends_with(".bat")
}

fn write_mcp_message(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn read_mcp_message(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "MCP stream ended before headers",
            ));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid MCP Content-Length: {error}"),
                    )
                })?);
            }
        }
    }
    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing MCP Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn parse_tool_capabilities(
    server_name: &str,
    timeout_seconds: u64,
    result: Value,
) -> Vec<McpCapability> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?.to_owned();
            Some(McpCapability {
                kind: McpCapabilityKind::Tool,
                server_name: server_name.to_owned(),
                name,
                description: optional_string(tool.get("description")),
                input_schema: tool
                    .get("inputSchema")
                    .or_else(|| tool.get("input_schema"))
                    .cloned(),
                uri: None,
                arguments: Vec::new(),
                timeout_seconds,
            })
        })
        .collect()
}

fn parse_resource_capabilities(
    server_name: &str,
    timeout_seconds: u64,
    result: Value,
) -> Vec<McpCapability> {
    result
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|resource| {
            let uri = resource.get("uri").and_then(Value::as_str)?.to_owned();
            let name = resource
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&uri)
                .to_owned();
            Some(McpCapability {
                kind: McpCapabilityKind::Resource,
                server_name: server_name.to_owned(),
                name,
                description: optional_string(resource.get("description")),
                input_schema: None,
                uri: Some(uri),
                arguments: Vec::new(),
                timeout_seconds,
            })
        })
        .collect()
}

fn parse_prompt_capabilities(
    server_name: &str,
    timeout_seconds: u64,
    result: Value,
) -> Vec<McpCapability> {
    result
        .get("prompts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|prompt| {
            let name = prompt.get("name").and_then(Value::as_str)?.to_owned();
            let arguments = prompt
                .get("arguments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|argument| {
                    Some(McpPromptArgument {
                        name: argument.get("name").and_then(Value::as_str)?.to_owned(),
                        description: optional_string(argument.get("description")),
                        required: argument
                            .get("required")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect();
            Some(McpCapability {
                kind: McpCapabilityKind::Prompt,
                server_name: server_name.to_owned(),
                name,
                description: optional_string(prompt.get("description")),
                input_schema: None,
                uri: None,
                arguments,
                timeout_seconds,
            })
        })
        .collect()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn parts_from_mcp_result(method: &str, result: &Value) -> Vec<String> {
    match method {
        "resources/read" => result
            .get("contents")
            .and_then(Value::as_array)
            .map(|contents| contents.iter().map(resource_part_text).collect())
            .unwrap_or_default(),
        "prompts/get" => result
            .get("messages")
            .and_then(Value::as_array)
            .map(|messages| messages.iter().flat_map(prompt_message_parts).collect())
            .unwrap_or_default(),
        _ => result
            .get("content")
            .and_then(Value::as_array)
            .map(|content| content.iter().map(content_part_text).collect())
            .unwrap_or_default(),
    }
}

fn content_part_text(value: &Value) -> String {
    value
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| value.to_string())
}

fn resource_part_text(value: &Value) -> String {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_owned();
    }
    if let Some(blob) = value.get("blob").and_then(Value::as_str) {
        return format!("[Binary resource: {} bytes]", blob.len());
    }
    content_part_text(value)
}

fn prompt_message_parts(value: &Value) -> Vec<String> {
    let Some(content) = value.get("content") else {
        return vec![value.to_string()];
    };
    if let Some(items) = content.as_array() {
        return items.iter().map(content_part_text).collect();
    }
    vec![content_part_text(content)]
}

pub fn normalize_schema_for_openai(schema: Value) -> Value {
    let Value::Object(mut object) = schema else {
        return json!({ "type": "object", "properties": {} });
    };

    if let Some(Value::Array(types)) = object.get("type") {
        let non_null = types
            .iter()
            .filter(|value| value.as_str() != Some("null"))
            .cloned()
            .collect::<Vec<_>>();
        if types.iter().any(|value| value.as_str() == Some("null")) && non_null.len() == 1 {
            object.insert("type".to_owned(), non_null[0].clone());
            object.insert("nullable".to_owned(), Value::Bool(true));
        }
    }

    for key in ["oneOf", "anyOf"] {
        if let Some((branch, nullable)) = extract_nullable_branch(object.get(key)) {
            object.remove(key);
            if let Value::Object(branch) = branch {
                for (key, value) in branch {
                    object.insert(key, value);
                }
            }
            if nullable {
                object.insert("nullable".to_owned(), Value::Bool(true));
            }
            break;
        }
    }

    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        let normalized = properties
            .iter()
            .map(|(name, value)| {
                let value = if value.is_object() {
                    normalize_schema_for_openai(value.clone())
                } else {
                    value.clone()
                };
                (name.clone(), value)
            })
            .collect::<Map<_, _>>();
        *properties = normalized;
    }

    if let Some(items) = object.get_mut("items") {
        if items.is_object() {
            *items = normalize_schema_for_openai(items.clone());
        }
    }

    if object.get("type").and_then(Value::as_str) == Some("object") {
        object
            .entry("properties".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        object
            .entry("required".to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
    }
    Value::Object(object)
}

fn extract_nullable_branch(value: Option<&Value>) -> Option<(Value, bool)> {
    let options = value?.as_array()?;
    let mut non_null = Vec::new();
    let mut saw_null = false;
    for option in options {
        let object = option.as_object()?;
        if object.get("type").and_then(Value::as_str) == Some("null") {
            saw_null = true;
        } else {
            non_null.push(option.clone());
        }
    }
    (saw_null && non_null.len() == 1).then(|| (non_null.remove(0), true))
}

fn wrapped_name(server_name: &str, kind: &str, name: &str) -> String {
    if kind.is_empty() {
        sanitize_mcp_name(&format!("mcp_{server_name}_{name}"))
    } else {
        sanitize_mcp_name(&format!("mcp_{server_name}_{kind}_{name}"))
    }
}

fn retry_transient(mut operation: impl FnMut() -> McpCallOutcome) -> McpCallOutcome {
    let first = operation();
    if matches!(first, McpCallOutcome::Error(McpErrorKind::Transient { .. })) {
        operation()
    } else {
        first
    }
}

fn format_tool_outcome(outcome: McpCallOutcome, timeout_seconds: u64) -> String {
    match outcome {
        McpCallOutcome::Success(parts) => join_parts(parts),
        McpCallOutcome::Error(McpErrorKind::Timeout) => {
            format!("(MCP tool call timed out after {timeout_seconds}s)")
        }
        McpCallOutcome::Error(McpErrorKind::Cancelled) => {
            "(MCP tool call was cancelled)".to_owned()
        }
        McpCallOutcome::Error(McpErrorKind::Transient { type_name }) => {
            format!("(MCP tool call failed after retry: {type_name})")
        }
        McpCallOutcome::Error(McpErrorKind::Other { type_name }) => {
            format!("(MCP tool call failed: {type_name})")
        }
        McpCallOutcome::Error(McpErrorKind::Protocol { code, message }) => {
            format!("(MCP tool call failed: {message} [code {code}])")
        }
    }
}

fn format_resource_outcome(outcome: McpCallOutcome, timeout_seconds: u64) -> String {
    match outcome {
        McpCallOutcome::Success(parts) => join_parts(parts),
        McpCallOutcome::Error(McpErrorKind::Timeout) => {
            format!("(MCP resource read timed out after {timeout_seconds}s)")
        }
        McpCallOutcome::Error(McpErrorKind::Cancelled) => {
            "(MCP resource read was cancelled)".to_owned()
        }
        McpCallOutcome::Error(McpErrorKind::Transient { type_name }) => {
            format!("(MCP resource read failed after retry: {type_name})")
        }
        McpCallOutcome::Error(McpErrorKind::Other { type_name }) => {
            format!("(MCP resource read failed: {type_name})")
        }
        McpCallOutcome::Error(McpErrorKind::Protocol { code, message }) => {
            format!("(MCP resource read failed: {message} [code {code}])")
        }
    }
}

fn format_prompt_outcome(outcome: McpCallOutcome, timeout_seconds: u64) -> String {
    match outcome {
        McpCallOutcome::Success(parts) => join_parts(parts),
        McpCallOutcome::Error(McpErrorKind::Timeout) => {
            format!("(MCP prompt call timed out after {timeout_seconds}s)")
        }
        McpCallOutcome::Error(McpErrorKind::Cancelled) => {
            "(MCP prompt call was cancelled)".to_owned()
        }
        McpCallOutcome::Error(McpErrorKind::Transient { type_name }) => {
            format!("(MCP prompt call failed after retry: {type_name})")
        }
        McpCallOutcome::Error(McpErrorKind::Other { type_name }) => {
            format!("(MCP prompt call failed: {type_name})")
        }
        McpCallOutcome::Error(McpErrorKind::Protocol { code, message }) => {
            format!("(MCP prompt call failed: {message} [code {code}])")
        }
    }
}

fn join_parts(parts: Vec<String>) -> String {
    if parts.is_empty() {
        "(no output)".to_owned()
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    static STDIO_PROCESS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn stdio_mcp_framing_round_trips_json_rpc_messages() {
        let message = json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}});
        let mut bytes = Vec::new();
        write_mcp_message(&mut bytes, &message).expect("write MCP frame");

        let mut reader = BufReader::new(Cursor::new(bytes));
        let decoded = read_mcp_message(&mut reader).expect("read MCP frame");
        assert_eq!(decoded, message);
    }

    #[test]
    fn stdio_mcp_parses_capabilities_and_result_parts() {
        let capabilities = parse_tool_capabilities(
            "srv",
            5,
            json!({"tools": [{"name": "search", "description": "Search", "inputSchema": {"type": "object"}}]}),
        );
        assert_eq!(capabilities.len(), 1);
        assert_eq!(capabilities[0].name, "search");
        assert_eq!(capabilities[0].timeout_seconds, 5);

        assert_eq!(
            parts_from_mcp_result(
                "tools/call",
                &json!({"content": [{"type": "text", "text": "done"}]})
            ),
            vec!["done".to_owned()]
        );
        assert_eq!(
            parts_from_mcp_result("resources/read", &json!({"contents": [{"blob": "abcd"}]})),
            vec!["[Binary resource: 4 bytes]".to_owned()]
        );
    }

    #[test]
    fn stdio_mcp_parses_resource_and_prompt_capabilities_without_tools() {
        let resources = parse_resource_capabilities(
            "srv",
            7,
            json!({"resources": [{"uri": "file://README.md", "description": "Readme"}]}),
        );
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, McpCapabilityKind::Resource);
        assert_eq!(resources[0].name, "file://README.md");
        assert_eq!(resources[0].uri.as_deref(), Some("file://README.md"));
        assert_eq!(resources[0].timeout_seconds, 7);

        let prompts = parse_prompt_capabilities(
            "srv",
            9,
            json!({"prompts": [{"name": "plan", "arguments": [{"name": "topic", "required": true}]}]}),
        );
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].kind, McpCapabilityKind::Prompt);
        assert_eq!(prompts[0].name, "plan");
        assert_eq!(prompts[0].arguments[0].name, "topic");
        assert!(prompts[0].arguments[0].required);
        assert_eq!(prompts[0].timeout_seconds, 9);
    }

    #[cfg(unix)]
    #[test]
    fn stdio_mcp_connector_clear_env_removes_parent_env_but_keeps_configured_env() {
        let _guard = stdio_process_test_guard();
        let tempdir = tempfile::tempdir().expect("temporary MCP server root");
        let server_path = tempdir.path().join("server");
        let leak_marker = tempdir.path().join("leaked-home");
        let allowed_marker = tempdir.path().join("allowed-env");
        std::fs::write(
            &server_path,
            format!(
                "#!/bin/sh\n\
if [ -n \"$HOME\" ]; then printf leaked > {}; fi\n\
if [ \"$SHACS_ALLOWED\" = \"1\" ]; then printf allowed > {}; fi\n\
frame() {{ body=$1; printf 'Content-Length: %s\\r\\n\\r\\n%s' \"${{#body}}\" \"$body\"; }}\n\
frame '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"serverInfo\":{{\"name\":\"test\",\"version\":\"1\"}}}}}}'\n\
frame '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"tools\":[]}}}}'\n\
frame '{{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{{\"resources\":[]}}}}'\n\
frame '{{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{{\"prompts\":[]}}}}'\n\
while read line; do :; done\n",
                shell_quote_path(&leak_marker),
                shell_quote_path(&allowed_marker)
            ),
        )
        .expect("write MCP test server");
        let mut permissions = std::fs::metadata(&server_path)
            .expect("MCP test server metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&server_path, permissions)
            .expect("make MCP test server executable");

        let spec = McpServerSpec {
            name: "clear-env".to_owned(),
            r#type: Some("stdio".to_owned()),
            command: Some(server_path.to_string_lossy().into_owned()),
            args: Vec::new(),
            env: vec![("SHACS_ALLOWED".to_owned(), "1".to_owned())],
            clear_env: true,
            url: None,
            headers: Vec::new(),
            timeout_seconds: 5,
            enabled_tools: Vec::new(),
            parent_containment_snapshot: None,
            startup_gate: Some(allow_startup_gate("clear-env", "server")),
        };

        let connector = StdioMcpConnector::new();
        let (_client, capabilities) = connector.connect(&spec).expect("connect MCP test server");

        assert!(capabilities.is_empty());
        assert!(!leak_marker.exists());
        assert!(allowed_marker.exists());
        connector.close("clear-env");
    }

    #[cfg(unix)]
    #[test]
    fn stdio_mcp_connection_close_consumes_cleanup_handles_once() {
        let _guard = stdio_process_test_guard();
        let tempdir = tempfile::tempdir().expect("temporary MCP server root");
        let server_path = tempdir.path().join("server");
        std::fs::write(&server_path, "#!/bin/sh\nwhile read line; do :; done\n")
            .expect("write MCP close test server");
        let mut permissions = std::fs::metadata(&server_path)
            .expect("MCP close test server metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&server_path, permissions)
            .expect("make MCP close test server executable");
        let mut child = Command::new(&server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn close test child");
        let pid = child.id();
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let mut connection = StdioMcpConnection::new(child, stdin, stdout);

        connection.close();
        assert!(connection.child.is_none());
        assert!(connection.stdin.is_none());
        assert!(connection.stdout_reader.is_none());
        assert!(!process_exists(pid));

        connection.close();
        assert!(connection.child.is_none());
        assert!(connection.stdin.is_none());
        assert!(connection.stdout_reader.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn stdio_mcp_connector_close_terminates_background_descendant_process_group() {
        let _guard = stdio_process_test_guard();
        let fixture = StdioDescendantFixture::new("close", StdioFixtureMode::Persistent);
        let spec = fixture.spec(5);
        let connector = StdioMcpConnector::new();

        let (_client, _capabilities) = connector.connect(&spec).expect("connect MCP fixture");
        let identity = fixture.identity();
        connector.close("close");
        connector.close("close");

        assert!(!process_exists(identity.pid));
    }

    #[cfg(unix)]
    #[test]
    fn stdio_mcp_request_timeout_terminates_background_descendant_process_group() {
        let _guard = stdio_process_test_guard();
        let fixture = StdioDescendantFixture::new("timeout-tree", StdioFixtureMode::Silent);
        let spec = fixture.spec(10);
        let connector = StdioMcpConnector::new();

        let result = connector.connect(&spec);
        let identity = fixture.identity();
        connector.close("timeout-tree");

        let Err(error) = result else {
            panic!("silent MCP fixture should time out");
        };
        assert!(
            error.contains("timed out"),
            "unexpected timeout error: {error}"
        );
        assert!(!process_exists(identity.pid));
    }

    #[cfg(unix)]
    #[test]
    fn stdio_mcp_runtime_close_cancels_descendant_process_group_idempotently() {
        let _guard = stdio_process_test_guard();
        let fixture = StdioDescendantFixture::new("runtime-cancel", StdioFixtureMode::Persistent);
        let spec = fixture.spec(5);
        let connector = Arc::new(StdioMcpConnector::new());
        let runtime = McpRuntime::new(Some(connector));
        let mut registry = ToolRegistry::new();

        let reports = runtime.connect_and_register(&mut registry, &[spec]);
        let identity = fixture.identity();
        runtime.close();
        runtime.close();

        assert_eq!(reports.len(), 1);
        assert!(reports[0].connected);
        assert!(!process_exists(identity.pid));
    }

    #[cfg(unix)]
    #[test]
    fn stdio_mcp_close_terminates_descendant_after_direct_parent_exits_first() {
        let _guard = stdio_process_test_guard();
        let fixture = StdioDescendantFixture::new("parent-exits", StdioFixtureMode::ParentExits);
        let spec = fixture.spec(5);
        let connector = StdioMcpConnector::new();

        let (_client, _capabilities) = connector.connect(&spec).expect("connect MCP fixture");
        let identity = fixture.identity();
        connector.close("parent-exits");

        assert!(!process_exists(identity.pid));
    }

    #[cfg(not(unix))]
    #[test]
    fn stdio_mcp_process_group_cleanup_is_not_claimed_on_non_unix() {
        let _cleanup = StdioChildProcessGroup;
    }

    #[cfg(unix)]
    fn shell_quote_path(path: &std::path::Path) -> String {
        format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
    }

    #[cfg(unix)]
    fn stdio_process_test_guard() -> std::sync::MutexGuard<'static, ()> {
        STDIO_PROCESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum StdioFixtureMode {
        Persistent,
        Silent,
        ParentExits,
    }

    #[cfg(unix)]
    struct StdioDescendantFixture {
        _tempdir: tempfile::TempDir,
        server_path: PathBuf,
        identity_file: PathBuf,
        name: String,
    }

    #[cfg(unix)]
    struct DescendantIdentity {
        pid: u32,
    }

    #[cfg(unix)]
    impl StdioDescendantFixture {
        fn new(name: &str, mode: StdioFixtureMode) -> Self {
            let tempdir = tempfile::tempdir().expect("temporary MCP process-tree fixture root");
            let server_path = tempdir.path().join("server");
            let descendant_path = tempdir.path().join("descendant");
            let identity_file = tempdir.path().join("identity.txt");
            std::fs::write(
                &descendant_path,
                "#!/bin/sh\n\
while :; do /bin/sleep 30; done\n",
            )
            .expect("write descendant fixture");
            std::fs::write(
                &server_path,
                server_script(&descendant_path, &identity_file, mode),
            )
            .expect("write MCP process-tree fixture");
            make_executable(&server_path);
            make_executable(&descendant_path);
            Self {
                _tempdir: tempdir,
                server_path,
                identity_file,
                name: name.to_owned(),
            }
        }

        fn spec(&self, timeout_seconds: u64) -> McpServerSpec {
            McpServerSpec {
                name: self.name.clone(),
                r#type: Some("stdio".to_owned()),
                command: Some(self.server_path.to_string_lossy().into_owned()),
                args: Vec::new(),
                env: Vec::new(),
                clear_env: true,
                url: None,
                headers: Vec::new(),
                timeout_seconds,
                enabled_tools: vec!["*".to_owned()],
                parent_containment_snapshot: None,
                startup_gate: Some(allow_startup_gate(&self.name, "mcp-process-tree-fixture")),
            }
        }

        fn identity(&self) -> DescendantIdentity {
            let line = std::fs::read_to_string(&self.identity_file)
                .expect("read descendant identity file");
            let pid = line
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .expect("parse descendant pid");
            DescendantIdentity { pid }
        }
    }

    fn make_executable(path: &Path) {
        let mut permissions = std::fs::metadata(path)
            .expect("MCP fixture metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("make MCP fixture executable");
    }

    #[cfg(unix)]
    fn server_script(
        descendant_path: &Path,
        identity_file: &Path,
        mode: StdioFixtureMode,
    ) -> String {
        let tool_frame = match mode {
            StdioFixtureMode::Silent => {
                return silent_server_script(descendant_path, identity_file)
            }
            StdioFixtureMode::Persistent | StdioFixtureMode::ParentExits => {
                "frame '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}'\n"
            }
        };
        let tail = match mode {
            StdioFixtureMode::ParentExits => "exit 0\n",
            StdioFixtureMode::Persistent => "while :; do /bin/sleep 30; done\n",
            StdioFixtureMode::Silent => unreachable!("silent mode returns before tail selection"),
        };
        format!(
            "#!/bin/sh\n\
{} >&1 &\n\
printf '%s\\n' \"$!\" > {}\n\
frame() {{ body=$1; printf 'Content-Length: %s\\r\\n\\r\\n%s' \"${{#body}}\" \"$body\"; }}\n\
frame '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"serverInfo\":{{\"name\":\"tree\",\"version\":\"1\"}}}}}}'\n\
{}\
frame '{{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{{\"resources\":[]}}}}'\n\
frame '{{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{{\"prompts\":[]}}}}'\n\
{}",
            shell_quote_path(descendant_path),
            shell_quote_path(identity_file),
            tool_frame,
            tail
        )
    }

    #[cfg(unix)]
    fn silent_server_script(descendant_path: &Path, identity_file: &Path) -> String {
        format!(
            "#!/bin/sh\n\
{} >&1 &\n\
printf '%s\\n' \"$!\" > {}\n\
while :; do /bin/sleep 30; done\n",
            shell_quote_path(descendant_path),
            shell_quote_path(identity_file)
        )
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
    }

    fn allow_startup_gate(server_name: &str, command_family: &str) -> McpStartupGate {
        use crate::runtime::{
            containment_permission_proof_for_process_gate, ActionNormalizationState,
            ContainerNetworkMode, ContainerRuntimeKind, DockerContainmentSnapshot,
            InheritedPermissionContext, PermissionCeilingSnapshot, PermissionMode,
            PermissionModeSnapshot, PermissionRuleInput, PermissionedAction,
            PermissionedActionOrigin, ProcExecSummary, ProcessAdapterKind,
            ProcessContainmentProofCandidate, ProcessExecutionEnvelope,
            ProcessExecutionEnvelopeInput, ProcessGateTerminalPrecondition, ProcessIdentity,
            ProcessRedactedCommand, RuntimeBoundaryOrigin, SafetyCapability,
        };

        let action = PermissionedAction {
            action_id: format!("mcp-startup-{server_name}"),
            provider_tool_call_id: Some(format!("mcp-startup-{server_name}")),
            session_id: "session-mcp".to_owned(),
            turn_id: "turn-mcp".to_owned(),
            tool_name: format!("mcp_{server_name}_startup"),
            capabilities: vec![SafetyCapability::ProcExec],
            target_refs: Vec::new(),
            action_digest: format!("mcp-startup-digest-{server_name}"),
            argument_digest: "argument-digest".to_owned(),
            snapshot_digest: "snapshot-digest".to_owned(),
            policy_safety_snapshot_ref: Some(policy_ref()),
            origin: PermissionedActionOrigin::UserTurn,
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::BypassPermissions,
                source: Some("test".to_owned()),
                scope_ref: Some("workspace".to_owned()),
            },
            containment_snapshot: None,
            intent_snapshot: None,
            redacted_arguments: json!({"command_family": command_family}),
            secret_ref_evidence: Vec::new(),
            normalization_state: ActionNormalizationState::Ready,
            normalization_errors: Vec::new(),
        };
        let envelope = ProcessExecutionEnvelope::try_from_input(ProcessExecutionEnvelopeInput {
            identity: ProcessIdentity::new(format!("mcp:{server_name}"), "session-mcp", "turn-mcp"),
            adapter: ProcessAdapterKind::McpStdio,
            action,
            required_secret_ref_count: 0,
            redacted_command: ProcessRedactedCommand {
                command_family: command_family.to_owned(),
                redacted_summary: format!("mcp stdio server {server_name}"),
                redacted_targets: Vec::new(),
            },
        })
        .expect("MCP startup envelope fixture should be valid");
        let permission_rules = PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: vec!["workspace".to_owned()],
                network_mode: ContainerNetworkMode::Bridge,
                digest: Some("container-digest".to_owned()),
                summary: Some("docker non-root".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: command_family.to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
        };
        let inherited_context = InheritedPermissionContext {
            ceiling: PermissionCeilingSnapshot {
                parent_mode: PermissionMode::BypassPermissions,
                capability_ceiling: vec![SafetyCapability::ProcExec],
                approved_scope_refs: vec!["workspace".to_owned()],
                origin: RuntimeBoundaryOrigin::UserTurn,
            },
            requested_mode: PermissionMode::BypassPermissions,
            requested_capabilities: vec![SafetyCapability::ProcExec],
            per_action_evaluation_required: true,
        };
        let containment_proof = containment_permission_proof_for_process_gate(
            &envelope,
            &permission_rules,
            Some(&inherited_context),
            200,
        )
        .expect("MCP startup containment proof fixture should be valid");
        McpStartupGate {
            input: ProcessGateInput {
                envelope,
                permission_rules,
                inherited_context: Some(inherited_context),
                evaluator: None,
                approval: None,
                containment_proof: ProcessContainmentProofCandidate::Proof(Box::new(
                    containment_proof,
                )),
                interactive: false,
                terminal_precondition: ProcessGateTerminalPrecondition::Ready,
                now_unix_ms: 200,
            },
        }
    }

    fn policy_ref() -> crate::runtime::PolicySafetySnapshotRef {
        crate::runtime::PolicySafetySnapshotRef {
            schema_id: crate::runtime::PolicySafetySnapshotSchemaId::V1,
            snapshot_id: crate::runtime::PolicySafetySnapshotId("snapshot-mcp".to_owned()),
            policy_safety_digest: crate::runtime::PolicySafetyDigest(
                "1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            ),
            created_at_unix_ms: 100,
            expires_at_unix_ms: None,
            redacted_summary: crate::runtime::RedactedPolicySafetySummary {
                permission_mode: "bypass_permissions".to_owned(),
                capability_count: 1,
                containment_digest: None,
                source_ref_count: 1,
                provenance_ref_count: 1,
            },
        }
    }
}
