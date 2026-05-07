use crate::tools::{JsonMap, Tool, ToolRegistry, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    pub url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub timeout_seconds: u64,
    pub enabled_tools: Vec<String>,
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
}

pub trait McpConnector: Send + Sync {
    fn connect(
        &self,
        spec: &McpServerSpec,
    ) -> Result<(Arc<dyn McpClient>, Vec<McpCapability>), String>;
    fn close(&self, _server_name: &str) {}
}

#[derive(Default)]
pub struct StdioMcpConnector {
    connections: Mutex<HashMap<String, Arc<Mutex<StdioMcpConnection>>>>,
}

impl StdioMcpConnector {
    pub fn new() -> Self {
        Self::default()
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
        let mut child = Command::new(&command)
            .args(&args)
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("MCP server `{}` did not expose stdin", spec.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("MCP server `{}` did not expose stdout", spec.name))?;
        let mut connection = StdioMcpConnection::new(child, stdin, stdout);
        connection.initialize()?;
        let capabilities = connection.list_capabilities(&spec.name, spec.timeout_seconds)?;
        let client = Arc::new(Mutex::new(connection));
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(spec.name.clone(), client.clone());
        Ok((client, capabilities))
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

struct StdioMcpConnection {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioMcpConnection {
    fn new(child: Child, stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        }
    }

    fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "shacs-bot", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        write_mcp_message(
            &mut self.stdin,
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
        if let Ok(result) = self.request("tools/list", json!({})) {
            capabilities.extend(parse_tool_capabilities(
                server_name,
                timeout_seconds,
                result,
            ));
        }
        if let Ok(result) = self.request("resources/list", json!({})) {
            capabilities.extend(parse_resource_capabilities(
                server_name,
                timeout_seconds,
                result,
            ));
        }
        if let Ok(result) = self.request("prompts/list", json!({})) {
            capabilities.extend(parse_prompt_capabilities(
                server_name,
                timeout_seconds,
                result,
            ));
        }
        Ok(capabilities)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        write_mcp_message(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .map_err(|error| format!("MCP request `{method}` write failed: {error}"))?;
        loop {
            let response = read_mcp_message(&mut self.stdout)
                .map_err(|error| format!("MCP request `{method}` read failed: {error}"))?;
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

    fn call_outcome(&mut self, method: &str, params: Value) -> McpCallOutcome {
        match self.request(method, params) {
            Ok(result) => McpCallOutcome::Success(parts_from_mcp_result(method, &result)),
            Err(error) => McpCallOutcome::Error(McpErrorKind::Other { type_name: error }),
        }
    }

    fn close(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StdioMcpConnection {
    fn drop(&mut self) {
        self.close();
    }
}

impl McpClient for Mutex<StdioMcpConnection> {
    fn call_tool(&self, name: &str, arguments: JsonMap, _timeout_seconds: u64) -> McpCallOutcome {
        self.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call_outcome(
                "tools/call",
                json!({"name": name, "arguments": Value::Object(arguments)}),
            )
    }

    fn read_resource(&self, uri: &str, _timeout_seconds: u64) -> McpCallOutcome {
        self.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call_outcome("resources/read", json!({"uri": uri}))
    }

    fn get_prompt(&self, name: &str, arguments: JsonMap, _timeout_seconds: u64) -> McpCallOutcome {
        self.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call_outcome(
                "prompts/get",
                json!({"name": name, "arguments": Value::Object(arguments)}),
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
                })
                .collect();
        };
        for spec in specs {
            if spec.transport_kind().is_none() {
                reports.push(McpServerConnectionReport {
                    server_name: spec.name.clone(),
                    connected: false,
                    registered_count: 0,
                    error: Some("missing or unsupported MCP transport".to_owned()),
                    unmatched_enabled_tools: Vec::new(),
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
                    });
                }
                Err(error) => reports.push(McpServerConnectionReport {
                    server_name: spec.name.clone(),
                    connected: false,
                    registered_count: 0,
                    error: Some(error),
                    unmatched_enabled_tools: Vec::new(),
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
        match capability.kind {
            McpCapabilityKind::Tool => {
                let wrapped = wrapped_name(&capability.server_name, "", &capability.name);
                if !allow_all_tools
                    && !enabled.contains(&capability.name)
                    && !enabled.contains(&wrapped)
                {
                    continue;
                }
                if enabled.contains(&capability.name) {
                    matched_enabled_tools.insert(capability.name.clone());
                }
                if enabled.contains(&wrapped) {
                    matched_enabled_tools.insert(wrapped.clone());
                }
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
}
