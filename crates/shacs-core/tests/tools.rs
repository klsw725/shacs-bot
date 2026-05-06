use serde_json::{json, Value};
use shacs_core::tools::{
    ask_user_options_from_messages, ask_user_outbound, ask_user_tool_result_messages,
    pending_ask_user_id, AskUserTool, CronTool,
};
use shacs_core::tools::{
    is_transient_mcp_error, normalize_schema_for_openai, register_mcp_capabilities,
    sanitize_mcp_name, tool_parameters, tool_parameters_schema, wrap_command, EditFileTool,
    ExecConfig, ExecTool, FileState, JsonMap, McpCallOutcome, McpCapability, McpCapabilityKind,
    McpClient, McpConnector, McpErrorKind, McpOperation, McpPromptArgument, McpRuntime,
    McpServerSpec, McpTransportKind, NotebookEditTool, ObjectSchema, OutboundMessage, PathContext,
    ReadFileTool, Schema, SearchHttpClient, SearchHttpResponse, SelfRuntimeState, SelfTool,
    SpawnRequest, StringSchema, Tool, ToolParameters, ToolRegistry, ToolResult,
    UreqWebSearchClient, WebClient, WebFetchConfig, WebFetchTool, WebSearchClient, WebSearchConfig,
    WebSearchResult, WebSearchTool, WriteFileTool,
};
use shacs_cron::{
    system_job, CronJobState, CronRunStatus, CronSchedule, CronService, InMemoryCronService,
};
use std::error::Error;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

#[cfg(unix)]
use filetime::{set_file_mtime, FileTime};

struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo a message a number of times."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "message",
                StringSchema::new("Message to echo").min_length(1),
            )
            .property(
                "times",
                shacs_core::tools::IntegerSchema::new("Repeat count").minimum(1),
            )
            .required(["message", "times"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let times = params.get("times").and_then(Value::as_u64).unwrap_or(1);
        ToolResult::Text(vec![message; times as usize].join(" "))
    }
}

#[test]
fn registry_casts_and_validates_tool_params() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(EchoTool);

    let result = registry
        .execute("echo", json!({ "message": 42, "times": "2" }))
        .into_text();

    if result != "42 42" {
        return Err(format!("unexpected echo result: {result}").into());
    }
    Ok(())
}

#[test]
fn tools_init_export_parity_helpers_render_schema() -> Result<(), Box<dyn Error>> {
    fn render_schema(schema: &dyn Schema) -> Value {
        schema.to_json_schema()
    }

    let params = ToolParameters::new()
        .property("message", StringSchema::new("Message to echo"))
        .required(["message"]);
    let rendered = tool_parameters_schema(params);

    if rendered["type"] != "object" || rendered["required"] != json!(["message"]) {
        return Err(format!("tool_parameters_schema drifted: {rendered}").into());
    }

    let object = ObjectSchema::new().property("text", StringSchema::new("Text"));
    let rendered = tool_parameters(object);
    if rendered["properties"]["text"]["type"] != "string" {
        return Err(format!("tool_parameters drifted: {rendered}").into());
    }

    let fragment = StringSchema::new("alias smoke");
    if render_schema(&fragment)["type"] != "string" {
        return Err("Schema alias no longer exposes SchemaFragment behavior".into());
    }

    Ok(())
}

#[test]
fn registry_sorts_builtin_tools_before_mcp_tools() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedTool("mcp_beta"));
    registry.register(NamedTool("alpha"));

    let names = registry
        .definitions()
        .into_iter()
        .map(|schema| {
            schema
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "missing schema name".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;

    if names != ["alpha", "mcp_beta"] {
        return Err(format!("unexpected schema order: {names:?}").into());
    }
    Ok(())
}

#[test]
fn ask_user_tool_is_exclusive_and_interrupts_with_options() -> Result<(), Box<dyn Error>> {
    let tool = AskUserTool::new();
    if tool.name() != "ask_user" || !tool.exclusive() || tool.concurrency_safe() {
        return Err("ask_user tool flags do not match blocking semantics".into());
    }
    let schema = tool.parameters();
    if !schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.contains(&json!("question")))
    {
        return Err(format!("ask_user question is not required: {schema}").into());
    }
    let missing_question = tool.validate_params(&json_map(json!({}))?);
    if !missing_question
        .iter()
        .any(|error| error.render().contains("missing required question"))
    {
        return Err(format!("missing question validation error: {missing_question:?}").into());
    }

    let result = tool.execute(json_map(json!({
        "question": "Deploy now?",
        "options": ["Yes", "No", ""]
    }))?);
    match result {
        ToolResult::AskUserInterrupt { question, options } => {
            if question != "Deploy now?" || options != ["Yes", "No"] {
                return Err(format!("unexpected ask interrupt: {question:?} {options:?}").into());
            }
        }
        other => return Err(format!("ask_user did not interrupt: {other:?}").into()),
    }

    let mut registry = ToolRegistry::new();
    registry.register(tool);
    let result = registry.execute(
        "ask_user",
        json!({ "question": "Continue?", "options": ["Continue"] }),
    );
    if !matches!(result, ToolResult::AskUserInterrupt { .. }) {
        return Err(format!("registry did not preserve ask interrupt: {result:?}").into());
    }
    Ok(())
}

#[test]
fn ask_user_helpers_track_pending_answers_and_options() -> Result<(), Box<dyn Error>> {
    let history = vec![
        json!({
            "role": "assistant",
            "tool_calls": [
                {"id": "call_other", "function": {"name": "message", "arguments": {}}},
                {"id": "call_ask", "function": {"name": "ask_user", "arguments": "{\"options\":[\"A\",\"B\",3]}"}}
            ]
        }),
        json!({ "role": "tool", "tool_call_id": "call_other", "content": "ok" }),
    ];
    if pending_ask_user_id(&history).as_deref() != Some("call_ask") {
        return Err(format!("pending ask id not found in {history:?}").into());
    }
    if ask_user_options_from_messages(&history) != ["A", "B"] {
        return Err(format!("ask options not extracted from {history:?}").into());
    }

    let answered = [
        history.clone(),
        vec![json!({ "role": "tool", "tool_call_id": "call_ask", "content": "A" })],
    ]
    .concat();
    if pending_ask_user_id(&answered).is_some() {
        return Err(format!("answered ask_user remained pending: {answered:?}").into());
    }

    let multiple_pending = vec![json!({
        "role": "assistant",
        "tool_calls": [
            {"id": "call_10", "function": {"name": "ask_user", "arguments": {"options": ["old"]}}},
            {"id": "call_2", "function": {"name": "ask_user", "arguments": {"options": ["new"]}}}
        ]
    })];
    if pending_ask_user_id(&multiple_pending).as_deref() != Some("call_2") {
        return Err(format!(
            "pending ask should use insertion order, not id sort: {multiple_pending:?}"
        )
        .into());
    }

    let object_arguments = vec![json!({
        "role": "assistant",
        "tool_calls": [{"id": "object_args", "function": {"name": "ask_user", "arguments": {"options": ["Object"]}}}]
    })];
    if ask_user_options_from_messages(&object_arguments) != ["Object"] {
        return Err(format!("object function arguments not parsed: {object_arguments:?}").into());
    }

    let top_level_arguments = vec![json!({
        "role": "assistant",
        "tool_calls": [{"id": "top_level", "name": "ask_user", "arguments": {"options": ["Top"]}}]
    })];
    if ask_user_options_from_messages(&top_level_arguments) != ["Top"] {
        return Err(format!("top-level arguments not parsed: {top_level_arguments:?}").into());
    }

    let messages = ask_user_tool_result_messages("system", &history, "call_ask", "B");
    if messages.first() != Some(&json!({ "role": "system", "content": "system" }))
        || messages.last()
            != Some(&json!({
                "role": "tool",
                "tool_call_id": "call_ask",
                "name": "ask_user",
                "content": "B"
            }))
    {
        return Err(format!("unexpected ask result messages: {messages:?}").into());
    }
    Ok(())
}

#[test]
fn ask_user_outbound_renders_buttons_by_channel() -> Result<(), Box<dyn Error>> {
    let options = vec!["Approve".to_owned(), "Reject".to_owned()];
    let (content, buttons) = ask_user_outbound(Some("Choose"), &options, "telegram");
    if content.as_deref() != Some("Choose") || buttons != [options.clone()] {
        return Err(
            format!("telegram should use structured buttons: {content:?} {buttons:?}").into(),
        );
    }

    let (content, buttons) = ask_user_outbound(Some("Choose"), &options, "slack");
    if content.as_deref() != Some("Choose\n\n1. Approve\n2. Reject") || !buttons.is_empty() {
        return Err(
            format!("slack should inline numbered options: {content:?} {buttons:?}").into(),
        );
    }

    let (content, buttons) = ask_user_outbound(None, &options, "discord");
    if content.as_deref() != Some("1. Approve\n2. Reject") || !buttons.is_empty() {
        return Err(format!(
            "missing content should render option text only: {content:?} {buttons:?}"
        )
        .into());
    }

    let (content, buttons) = ask_user_outbound(Some("Question"), &[], "telegram");
    if content.as_deref() != Some("Question") || !buttons.is_empty() {
        return Err(
            format!("empty options should not add buttons: {content:?} {buttons:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn spawn_tool_uses_context_and_delegates_to_spawner() -> Result<(), Box<dyn Error>> {
    let requests = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured = requests.clone();
    let tool = shacs_core::tools::SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured
            .lock()
            .map_err(|error| error.to_string())?
            .push(request.clone());
        let label = request
            .label
            .clone()
            .unwrap_or_else(|| "no-label".to_owned());
        Ok(format!(
            "Subagent [{label}] started (id: test1234). I'll notify you when it completes."
        ))
    }));

    if tool.name() != "spawn" || tool.exclusive() || tool.concurrency_safe() {
        return Err("spawn tool flags do not match side-effecting default semantics".into());
    }
    if !tool
        .description()
        .contains("The subagent will complete the task and report back when done")
    {
        return Err(format!("spawn description drifted: {}", tool.description()).into());
    }
    let schema = tool.parameters();
    if !schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.contains(&json!("task")))
        || !schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| {
                properties.get("task").and_then(|value| value.get("type")) == Some(&json!("string"))
                    && properties.get("label").and_then(|value| value.get("type"))
                        == Some(&json!("string"))
            })
    {
        return Err(format!("spawn schema drifted: {schema}").into());
    }
    let missing_task = tool.validate_params(&json_map(json!({}))?);
    if !missing_task
        .iter()
        .any(|error| error.render().contains("missing required task"))
    {
        return Err(format!("missing task validation error: {missing_task:?}").into());
    }

    tool.set_context("telegram", "chat-1", Some("unified-session".to_owned()));
    let result = tool
        .execute(json_map(json!({
            "task": "Summarize the repository",
            "label": "repo summary"
        }))?)
        .into_text();
    if !result.contains("Subagent [repo summary] started") {
        return Err(format!("unexpected spawn result: {result}").into());
    }

    let requests = requests.lock().map_err(|error| error.to_string())?;
    let request = requests.first().ok_or("missing spawn request")?;
    if request.task != "Summarize the repository"
        || request.label.as_deref() != Some("repo summary")
        || request.origin_channel != "telegram"
        || request.origin_chat_id != "chat-1"
        || request.session_key != "unified-session"
    {
        return Err(format!("spawn request mismatch: {request:?}").into());
    }
    drop(requests);

    let empty_label = tool
        .execute(json_map(json!({
            "task": "Empty label falls back downstream",
            "label": ""
        }))?)
        .into_text();
    if !empty_label.contains("Subagent [no-label] started") {
        return Err(format!("empty label was not normalized to None: {empty_label}").into());
    }
    Ok(())
}

#[test]
fn spawn_tool_defaults_errors_and_thread_local_context() -> Result<(), Box<dyn Error>> {
    let requests = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured = requests.clone();
    let tool = shacs_core::tools::SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("started".to_owned())
    }));

    let default_result = tool
        .execute(json_map(json!({ "task": "Use default context" }))?)
        .into_text();
    if default_result != "started" {
        return Err(format!("unexpected default spawn result: {default_result}").into());
    }
    {
        let requests = requests.lock().map_err(|error| error.to_string())?;
        let request = requests.first().ok_or("missing default spawn request")?;
        if request.origin_channel != "cli"
            || request.origin_chat_id != "direct"
            || request.session_key != "cli:direct"
            || request.label.is_some()
        {
            return Err(format!("default spawn context mismatch: {request:?}").into());
        }
    }

    let barrier = Arc::new(Barrier::new(2));
    let first_tool = tool.clone();
    let second_tool = tool.clone();
    let first_barrier = barrier.clone();
    let first = std::thread::spawn(move || -> Result<(), String> {
        first_tool.set_context("thread-a", "A", None);
        first_barrier.wait();
        let result = first_tool
            .execute(json_map(json!({ "task": "thread A" })).map_err(|error| error.to_string())?)
            .into_text();
        (result == "started")
            .then_some(())
            .ok_or_else(|| format!("unexpected thread A result: {result}"))
    });
    let second_barrier = barrier.clone();
    let second = std::thread::spawn(move || -> Result<(), String> {
        second_tool.set_context("thread-b", "B", None);
        second_barrier.wait();
        let result = second_tool
            .execute(json_map(json!({ "task": "thread B" })).map_err(|error| error.to_string())?)
            .into_text();
        (result == "started")
            .then_some(())
            .ok_or_else(|| format!("unexpected thread B result: {result}"))
    });
    first
        .join()
        .map_err(|_| std::io::Error::other("thread A panicked"))?
        .map_err(std::io::Error::other)?;
    second
        .join()
        .map_err(|_| std::io::Error::other("thread B panicked"))?
        .map_err(std::io::Error::other)?;

    let requests = requests.lock().map_err(|error| error.to_string())?;
    let thread_a = requests
        .iter()
        .find(|request| request.task == "thread A")
        .ok_or("missing thread A spawn")?;
    let thread_b = requests
        .iter()
        .find(|request| request.task == "thread B")
        .ok_or("missing thread B spawn")?;
    if thread_a.origin_channel != "thread-a"
        || thread_a.session_key != "thread-a:A"
        || thread_b.origin_channel != "thread-b"
        || thread_b.session_key != "thread-b:B"
    {
        return Err(format!("thread-local spawn context leaked: {thread_a:?} {thread_b:?}").into());
    }
    drop(requests);

    let failing = shacs_core::tools::SpawnTool::new(Arc::new(|_request: SpawnRequest| {
        Err("subagent manager unavailable".to_owned())
    }));
    let error = failing
        .execute(json_map(json!({ "task": "fail" }))?)
        .into_text();
    if error != "Error spawning subagent: subagent manager unavailable" {
        return Err(format!("spawn error was not surfaced: {error}").into());
    }
    Ok(())
}

struct NamedTool(&'static str);

impl Tool for NamedTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "named test tool"
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text(self.0.to_owned())
    }
}

#[derive(Clone)]
struct StaticWebClient {
    response: Result<shacs_core::tools::HttpResponse, String>,
}

#[derive(Clone)]
struct StaticSearchClient {
    results: Vec<WebSearchResult>,
}

#[derive(Clone)]
struct StaticSearchHttpClient {
    response: Result<SearchHttpResponse, String>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl WebSearchClient for StaticSearchClient {
    fn search(
        &self,
        _config: &WebSearchConfig,
        _query: &str,
        _count: usize,
    ) -> Result<Vec<WebSearchResult>, String> {
        Ok(self.results.clone())
    }
}

impl StaticSearchHttpClient {
    fn new(response: SearchHttpResponse) -> Self {
        Self {
            response: Ok(response),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl SearchHttpClient for StaticSearchHttpClient {
    fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        _headers: &[(&str, String)],
        _timeout: Duration,
    ) -> Result<SearchHttpResponse, String> {
        self.calls
            .lock()
            .map_err(|error| error.to_string())?
            .push(format!(
                "GET {url}?{}",
                query
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join("&")
            ));
        self.response.clone()
    }

    fn post_json(
        &self,
        url: &str,
        _headers: &[(&str, String)],
        body: Value,
        _timeout: Duration,
    ) -> Result<SearchHttpResponse, String> {
        self.calls
            .lock()
            .map_err(|error| error.to_string())?
            .push(format!("POST {url} {body}"));
        self.response.clone()
    }
}

impl WebClient for StaticWebClient {
    fn get(
        &self,
        _url: &str,
        _user_agent: &str,
        _timeout: Duration,
        _max_redirects: usize,
        _network_guard: &shacs_core::tools::NetworkGuard,
    ) -> Result<shacs_core::tools::HttpResponse, String> {
        self.response.clone()
    }
}

#[test]
fn read_file_returns_numbered_text_and_dedups_unchanged_reads() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let file = temp.path().join("note.txt");
    std::fs::write(&file, "alpha\nbeta\ngamma\n")?;

    let tool = ReadFileTool::new(PathContext::workspace(temp.path()));
    let first = tool
        .execute(json_map(
            json!({ "path": "note.txt", "offset": 2, "limit": 1 }),
        )?)
        .into_text();
    if !first.contains("2| beta") {
        return Err(format!("missing numbered line in: {first}").into());
    }

    let second = tool
        .execute(json_map(
            json!({ "path": "note.txt", "offset": 2, "limit": 1 }),
        )?)
        .into_text();
    if second != "[File unchanged since last read: note.txt]" {
        return Err(format!("unexpected dedup response: {second}").into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn read_file_does_not_dedup_same_mtime_content_change() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let file = temp.path().join("note.txt");
    std::fs::write(&file, "before\n")?;
    let original_mtime = FileTime::from_system_time(std::fs::metadata(&file)?.modified()?);

    let tool = ReadFileTool::new(PathContext::workspace(temp.path()));
    let first = tool
        .execute(json_map(json!({ "path": "note.txt" }))?)
        .into_text();
    if !first.contains("1| before") {
        return Err(format!("unexpected first read: {first}").into());
    }

    std::fs::write(&file, "after\n")?;
    set_file_mtime(&file, original_mtime)?;

    let second = tool
        .execute(json_map(json!({ "path": "note.txt" }))?)
        .into_text();
    if second.contains("[File unchanged") || !second.contains("1| after") {
        return Err(format!("dedup hid same-mtime content change: {second}").into());
    }
    Ok(())
}

#[test]
fn glob_and_grep_find_read_only_workspace_content() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::create_dir(temp.path().join("src"))?;
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn needle() {}\n")?;
    std::fs::write(temp.path().join("README.md"), "needle docs\n")?;

    let context = PathContext::workspace(temp.path());
    let mut registry = ToolRegistry::new();
    registry.register(shacs_core::tools::GlobTool::new(context.clone()));
    registry.register(shacs_core::tools::GrepTool::new(context));

    let glob = registry
        .execute(
            "glob",
            json!({ "pattern": "*.rs", "path": ".", "head_limit": 10 }),
        )
        .into_text();
    if !glob.contains("src/lib.rs") {
        return Err(format!("glob did not find src/lib.rs: {glob}").into());
    }

    let grep = registry
        .execute(
            "grep",
            json!({ "pattern": "needle", "path": ".", "type": "rs", "output_mode": "content" }),
        )
        .into_text();
    if !grep.contains("src/lib.rs:1") || !grep.contains("> 1| pub fn needle() {}") {
        return Err(format!("grep did not return expected content: {grep}").into());
    }
    Ok(())
}

#[test]
fn write_file_creates_parent_and_invalidates_shared_read_dedup() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let file_state = Arc::new(Mutex::new(FileState::new()));
    let context = PathContext::workspace(temp.path());
    let reader = ReadFileTool::with_file_state(context.clone(), file_state.clone());
    let writer = WriteFileTool::with_file_state(context, file_state);

    let write = writer
        .execute(json_map(json!({
            "path": "nested/note.txt",
            "content": "before\n"
        }))?)
        .into_text();
    if !write.contains("Successfully wrote") {
        return Err(format!("unexpected write response: {write}").into());
    }

    let first = reader
        .execute(json_map(json!({ "path": "nested/note.txt" }))?)
        .into_text();
    if !first.contains("1| before") {
        return Err(format!("unexpected first read: {first}").into());
    }

    writer.execute(json_map(json!({
        "path": "nested/note.txt",
        "content": "after\n"
    }))?);
    let second = reader
        .execute(json_map(json!({ "path": "nested/note.txt" }))?)
        .into_text();
    if second.contains("[File unchanged") || !second.contains("1| after") {
        return Err(format!("write did not invalidate dedup: {second}").into());
    }
    Ok(())
}

#[test]
fn read_hashlines_and_edit_replace_line_with_verified_tag() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("note.txt"), "one\ntwo\nthree\n")?;
    let file_state = Arc::new(Mutex::new(FileState::new()));
    let context = PathContext::workspace(temp.path());
    let reader = ReadFileTool::with_file_state(context.clone(), file_state.clone());
    let editor = EditFileTool::with_file_state(context, file_state);

    let hashlines = reader
        .execute(json_map(json!({
            "path": "note.txt",
            "hashlines": true,
            "hash_len": 8
        }))?)
        .into_text();
    let tag = tag_for_line(&hashlines, 2)?;

    let edit = editor
        .execute(json_map(json!({
            "path": "note.txt",
            "op": "replace_line",
            "line_tag": tag,
            "text": "TWO",
            "hash_len": 8
        }))?)
        .into_text();
    if !edit.contains("Successfully edited") {
        return Err(format!("unexpected edit response: {edit}").into());
    }

    let updated = std::fs::read_to_string(temp.path().join("note.txt"))?;
    if updated != "one\nTWO\nthree\n" {
        return Err(format!("unexpected edited content: {updated:?}").into());
    }
    Ok(())
}

#[test]
fn edit_rejects_stale_hashline_tag() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("note.txt"), "one\ntwo\n")?;
    let context = PathContext::workspace(temp.path());
    let reader = ReadFileTool::new(context.clone());
    let editor = EditFileTool::new(context);

    let hashlines = reader
        .execute(json_map(json!({ "path": "note.txt", "hashlines": true }))?)
        .into_text();
    let stale_tag = tag_for_line(&hashlines, 2)?;
    std::fs::write(temp.path().join("note.txt"), "one\nchanged\n")?;

    let result = editor
        .execute(json_map(json!({
            "path": "note.txt",
            "op": "replace_line",
            "line_tag": stale_tag,
            "text": "TWO"
        }))?)
        .into_text();
    if !result.contains("Hashline tag does not match") {
        return Err(format!("stale tag was not rejected: {result}").into());
    }
    Ok(())
}

#[test]
fn edit_rejects_non_hex_hashline_tag() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("note.txt"), "one\n")?;
    let editor = EditFileTool::new(PathContext::workspace(temp.path()));

    let result = editor
        .execute(json_map(json!({
            "path": "note.txt",
            "op": "replace_line",
            "line_tag": "L1#zzzzzzzz",
            "text": "ONE"
        }))?)
        .into_text();
    if !result.contains("Invalid non-hex hashline hash") {
        return Err(format!("invalid hashline tag was not rejected: {result}").into());
    }
    Ok(())
}

#[test]
fn edit_supports_insert_delete_and_range_operations() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("note.txt"), "a\nb\nc\nd\n")?;
    let context = PathContext::workspace(temp.path());
    let reader = ReadFileTool::new(context.clone());
    let editor = EditFileTool::new(context);

    let hashlines = reader
        .execute(json_map(json!({ "path": "note.txt", "hashlines": true }))?)
        .into_text();
    let b_tag = tag_for_line(&hashlines, 2)?;
    editor.execute(json_map(json!({
        "path": "note.txt",
        "op": "insert_after",
        "line_tag": b_tag,
        "text": "bb"
    }))?);

    let hashlines = reader
        .execute(json_map(json!({ "path": "note.txt", "hashlines": true }))?)
        .into_text();
    let c_tag = tag_for_line(&hashlines, 4)?;
    let d_tag = tag_for_line(&hashlines, 5)?;
    editor.execute(json_map(json!({
        "path": "note.txt",
        "op": "replace_range",
        "start_tag": c_tag,
        "end_tag": d_tag,
        "text": "tail"
    }))?);

    let updated = std::fs::read_to_string(temp.path().join("note.txt"))?;
    if updated != "a\nb\nbb\ntail\n" {
        return Err(format!("unexpected range edit result: {updated:?}").into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn recursive_tools_skip_symlinks_that_escape_workspace() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("secret.txt"), "needle secret\n")?;
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("outside_link"))?;

    let context = PathContext::workspace(workspace.path());
    let mut registry = ToolRegistry::new();
    registry.register(shacs_core::tools::ListDirTool::new(context.clone()));
    registry.register(shacs_core::tools::GlobTool::new(context.clone()));
    registry.register(shacs_core::tools::GrepTool::new(context));

    let list = registry
        .execute(
            "list_dir",
            json!({ "path": ".", "recursive": true, "max_entries": 20 }),
        )
        .into_text();
    if list.contains("secret.txt") || list.contains("outside_link") {
        return Err(format!("list_dir exposed symlink target: {list}").into());
    }

    let glob = registry
        .execute(
            "glob",
            json!({ "pattern": "**/*.txt", "path": ".", "head_limit": 20 }),
        )
        .into_text();
    if glob.contains("secret.txt") || glob.contains("outside_link") {
        return Err(format!("glob exposed symlink target: {glob}").into());
    }

    let grep = registry
        .execute(
            "grep",
            json!({ "pattern": "needle", "path": ".", "output_mode": "content" }),
        )
        .into_text();
    if grep.contains("secret") || grep.contains("outside_link") {
        return Err(format!("grep exposed symlink target: {grep}").into());
    }
    Ok(())
}

#[test]
fn write_file_rejects_allowed_dir_escape() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let writer = WriteFileTool::new(PathContext::workspace(temp.path()));

    let result = writer
        .execute(json_map(json!({
            "path": "../outside.txt",
            "content": "nope"
        }))?)
        .into_text();
    if !result.contains("outside allowed directory") {
        return Err(format!("allowed_dir escape was not rejected: {result}").into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn write_and_edit_reject_symlink_input_paths() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target.txt");
    std::fs::write(&target, "one\n")?;
    std::os::unix::fs::symlink(&target, temp.path().join("link.txt"))?;

    let context = PathContext::workspace(temp.path());
    let writer = WriteFileTool::new(context.clone());
    let write_result = writer
        .execute(json_map(json!({
            "path": "link.txt",
            "content": "two\n"
        }))?)
        .into_text();
    if !write_result.contains("Refusing to write through symlink") {
        return Err(format!("write_file accepted symlink input: {write_result}").into());
    }

    let reader = ReadFileTool::new(context.clone());
    let hashlines = reader
        .execute(json_map(
            json!({ "path": "target.txt", "hashlines": true }),
        )?)
        .into_text();
    let tag = tag_for_line(&hashlines, 1)?;
    let editor = EditFileTool::new(context);
    let edit_result = editor
        .execute(json_map(json!({
            "path": "link.txt",
            "op": "replace_line",
            "line_tag": tag,
            "text": "two"
        }))?)
        .into_text();
    if !edit_result.contains("Refusing to write through symlink") {
        return Err(format!("edit_file accepted symlink input: {edit_result}").into());
    }
    Ok(())
}

#[test]
fn notebook_edit_creates_notebook_on_insert() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let result = tool
        .execute(json_map(json!({
            "path": "new.ipynb",
            "cell_index": 0,
            "new_source": "print('hi')",
            "cell_type": "code",
            "edit_mode": "insert"
        }))?)
        .into_text();
    if !result.contains("Successfully created") {
        return Err(format!("unexpected notebook create result: {result}").into());
    }
    let notebook = read_json(temp.path().join("new.ipynb"))?;
    let cells = notebook["cells"].as_array().ok_or("cells must be array")?;
    if cells.len() != 1 || cells[0]["source"] != "print('hi')" {
        return Err(format!("unexpected created notebook: {notebook}").into());
    }
    if cells[0].get("id").and_then(Value::as_str).is_none() {
        return Err("new nbformat 4.5 cell should include id".into());
    }
    Ok(())
}

#[test]
fn notebook_edit_replaces_cell_and_converts_type() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    write_notebook_fixture(
        temp.path().join("test.ipynb"),
        json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [{
                "cell_type": "code",
                "source": "print('old')",
                "metadata": {},
                "outputs": [{"name": "stdout"}],
                "execution_count": 1,
                "id": "abcd1234"
            }]
        }),
    )?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let result = tool
        .execute(json_map(json!({
            "path": "test.ipynb",
            "cell_index": 0,
            "new_source": "# markdown",
            "cell_type": "markdown",
            "edit_mode": "replace"
        }))?)
        .into_text();
    if !result.contains("Successfully edited cell 0") {
        return Err(format!("unexpected notebook replace result: {result}").into());
    }
    let notebook = read_json(temp.path().join("test.ipynb"))?;
    let cell = &notebook["cells"][0];
    if cell["cell_type"] != "markdown" || cell["source"] != "# markdown" {
        return Err(format!("cell was not converted: {cell}").into());
    }
    if cell.get("outputs").is_some() || cell.get("execution_count").is_some() {
        return Err(format!("markdown cell kept code-only fields: {cell}").into());
    }
    Ok(())
}

#[test]
fn notebook_edit_preserves_same_type_metadata_outputs_and_id() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    write_notebook_fixture(
        temp.path().join("test.ipynb"),
        json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [{
                "cell_type": "code",
                "source": "old",
                "metadata": {"keep": true},
                "outputs": [{"output_type": "stream", "name": "stdout", "text": "x"}],
                "execution_count": 7,
                "id": "sameid01"
            }]
        }),
    )?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    tool.execute(json_map(json!({
        "path": "test.ipynb",
        "cell_index": 0,
        "new_source": "new",
        "cell_type": "code",
        "edit_mode": "replace"
    }))?);

    let notebook = read_json(temp.path().join("test.ipynb"))?;
    let cell = &notebook["cells"][0];
    if cell["source"] != "new"
        || cell["metadata"]["keep"] != true
        || cell["outputs"].as_array().map(Vec::len) != Some(1)
        || cell["execution_count"] != 7
        || cell["id"] != "sameid01"
    {
        return Err(format!("same-type replace did not preserve fields: {cell}").into());
    }
    Ok(())
}

#[test]
fn notebook_edit_inserts_and_deletes_cells() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    write_notebook_fixture(
        temp.path().join("test.ipynb"),
        json!({
            "nbformat": 4,
            "nbformat_minor": 4,
            "metadata": {},
            "cells": [
                {"cell_type": "markdown", "source": "one", "metadata": {}},
                {"cell_type": "markdown", "source": "two", "metadata": {}}
            ]
        }),
    )?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    tool.execute(json_map(json!({
        "path": "test.ipynb",
        "cell_index": 0,
        "new_source": "inserted",
        "cell_type": "markdown",
        "edit_mode": "insert"
    }))?);
    let after_insert = read_json(temp.path().join("test.ipynb"))?;
    if after_insert["cells"][1]["source"] != "inserted" {
        return Err(format!("insert landed in wrong position: {after_insert}").into());
    }

    tool.execute(json_map(json!({
        "path": "test.ipynb",
        "cell_index": 1,
        "edit_mode": "delete"
    }))?);
    let after_delete = read_json(temp.path().join("test.ipynb"))?;
    let cells = after_delete["cells"]
        .as_array()
        .ok_or("cells must be array")?;
    if cells.len() != 2 || cells[0]["source"] != "one" || cells[1]["source"] != "two" {
        return Err(format!("delete did not remove inserted cell: {after_delete}").into());
    }
    Ok(())
}

#[test]
fn notebook_edit_rejects_invalid_paths_modes_and_json() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("bad.ipynb"), "not json")?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let wrong_extension = tool
        .execute(json_map(json!({ "path": "note.txt", "cell_index": 0 }))?)
        .into_text();
    if !wrong_extension.contains("only works on .ipynb") {
        return Err(format!("wrong extension was not rejected: {wrong_extension}").into());
    }

    let invalid_mode = tool
        .execute(json_map(json!({
            "path": "bad.ipynb",
            "cell_index": 0,
            "edit_mode": "move"
        }))?)
        .into_text();
    if !invalid_mode.contains("Invalid edit_mode") {
        return Err(format!("invalid mode was not rejected: {invalid_mode}").into());
    }

    let invalid_cell_type = tool
        .execute(json_map(json!({
            "path": "bad.ipynb",
            "cell_index": 0,
            "cell_type": "raw"
        }))?)
        .into_text();
    if !invalid_cell_type.contains("Invalid cell_type") {
        return Err(format!("invalid cell_type was not rejected: {invalid_cell_type}").into());
    }

    let bad_json = tool
        .execute(json_map(json!({
            "path": "bad.ipynb",
            "cell_index": 0,
            "edit_mode": "replace"
        }))?)
        .into_text();
    if !bad_json.contains("Failed to parse notebook") {
        return Err(format!("invalid json was not rejected: {bad_json}").into());
    }
    Ok(())
}

#[test]
fn notebook_edit_large_insert_index_appends_without_overflow() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    write_notebook_fixture(
        temp.path().join("test.ipynb"),
        json!({
            "nbformat": 4,
            "nbformat_minor": 4,
            "metadata": {},
            "cells": [{"cell_type": "markdown", "source": "one", "metadata": {}}]
        }),
    )?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let result = tool
        .execute(json_map(json!({
            "path": "test.ipynb",
            "cell_index": u64::MAX,
            "new_source": "last",
            "cell_type": "markdown",
            "edit_mode": "insert"
        }))?)
        .into_text();
    if !result.contains("Successfully inserted") {
        return Err(format!("large index insert failed: {result}").into());
    }
    let notebook = read_json(temp.path().join("test.ipynb"))?;
    let cells = notebook["cells"].as_array().ok_or("cells must be array")?;
    if cells.len() != 2 || cells[1]["source"] != "last" {
        return Err(format!("large index did not append: {notebook}").into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn notebook_edit_rejects_symlink_input_path() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    write_notebook_fixture(
        temp.path().join("real.ipynb"),
        json!({"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": []}),
    )?;
    std::os::unix::fs::symlink(
        temp.path().join("real.ipynb"),
        temp.path().join("link.ipynb"),
    )?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let result = tool
        .execute(json_map(json!({
            "path": "link.ipynb",
            "cell_index": 0,
            "new_source": "x",
            "edit_mode": "insert"
        }))?)
        .into_text();
    if !result.contains("Refusing to write through symlink") {
        return Err(format!("notebook_edit accepted symlink path: {result}").into());
    }
    Ok(())
}

#[test]
fn notebook_edit_rejects_allowed_dir_escape_on_create() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let result = tool
        .execute(json_map(json!({
            "path": "../outside.ipynb",
            "cell_index": 0,
            "new_source": "x",
            "edit_mode": "insert"
        }))?)
        .into_text();
    if !result.contains("outside allowed directory") {
        return Err(format!("notebook create allowed path escape: {result}").into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn notebook_edit_rejects_symlink_parent_on_create() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::create_dir(temp.path().join("real"))?;
    std::os::unix::fs::symlink(temp.path().join("real"), temp.path().join("linkdir"))?;
    let tool = NotebookEditTool::new(PathContext::workspace(temp.path()));

    let result = tool
        .execute(json_map(json!({
            "path": "linkdir/new.ipynb",
            "cell_index": 0,
            "new_source": "x",
            "edit_mode": "insert"
        }))?)
        .into_text();
    if !result.contains("Refusing to write through symlink") {
        return Err(format!("notebook create accepted symlink parent: {result}").into());
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn write_file_rejects_symlink_parent_component() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::create_dir(temp.path().join("real"))?;
    std::os::unix::fs::symlink(temp.path().join("real"), temp.path().join("linkdir"))?;

    let writer = WriteFileTool::new(PathContext::workspace(temp.path()));
    let result = writer
        .execute(json_map(json!({
            "path": "linkdir/new.txt",
            "content": "nope"
        }))?)
        .into_text();
    if !result.contains("Refusing to write through symlink") {
        return Err(format!("write_file accepted symlink parent: {result}").into());
    }
    Ok(())
}

#[test]
fn exec_tool_runs_command_and_reports_exit_code() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = ExecTool::with_workspace(temp.path());

    let result = tool
        .execute(json_map(json!({
            "command": "printf hello",
            "timeout": 5
        }))?)
        .into_text();
    if !result.contains("hello") || !result.contains("Exit code: 0") {
        return Err(format!("exec did not return expected output: {result}").into());
    }
    Ok(())
}

#[test]
fn exec_tool_blocks_dangerous_and_non_allowlisted_commands() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = ExecTool::with_workspace(temp.path());
    let blocked = tool
        .execute(json_map(json!({ "command": "rm -rf ./target" }))?)
        .into_text();
    if !blocked.contains("dangerous pattern") {
        return Err(format!("dangerous command was not blocked: {blocked}").into());
    }

    let mut config = ExecConfig::new(PathContext::workspace(temp.path()));
    config.allow_patterns = vec![r"^printf\b".to_owned()];
    let allowlisted = ExecTool::new(config);
    let denied = allowlisted
        .execute(json_map(json!({ "command": "echo nope" }))?)
        .into_text();
    if !denied.contains("not in allowlist") {
        return Err(format!("non-allowlisted command was not blocked: {denied}").into());
    }
    Ok(())
}

#[test]
fn exec_tool_restricts_working_dir_and_paths_to_workspace() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let mut config = ExecConfig::new(PathContext::workspace(temp.path()));
    config.restrict_to_workspace = true;
    let tool = ExecTool::new(config);

    let bad_cwd = tool
        .execute(json_map(json!({
            "command": "printf nope",
            "working_dir": outside.path().to_string_lossy()
        }))?)
        .into_text();
    if !bad_cwd.contains("working_dir is outside") {
        return Err(format!("outside working_dir was not blocked: {bad_cwd}").into());
    }

    let traversal = tool
        .execute(json_map(json!({ "command": "printf ../secret" }))?)
        .into_text();
    if !traversal.contains("path traversal") {
        return Err(format!("path traversal was not blocked: {traversal}").into());
    }

    let nonexistent_external = tool
        .execute(json_map(json!({
            "command": "printf /tmp/shacs-nonexistent-output-file"
        }))?)
        .into_text();
    if !nonexistent_external.contains("path outside working dir") {
        return Err(format!(
            "nonexistent external absolute path was not blocked: {nonexistent_external}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn exec_tool_blocks_internal_urls_and_invalid_deny_regex() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = ExecTool::with_workspace(temp.path());
    let metadata_url = tool
        .execute(json_map(json!({
            "command": "curl http://169.254.169.254/latest/meta-data"
        }))?)
        .into_text();
    if !metadata_url.contains("internal/private URL") {
        return Err(format!("metadata URL was not blocked: {metadata_url}").into());
    }

    let mut config = ExecConfig::new(PathContext::workspace(temp.path()));
    config.deny_patterns = vec!["[".to_owned()];
    let invalid_regex = ExecTool::new(config)
        .execute(json_map(json!({ "command": "printf safe" }))?)
        .into_text();
    if !invalid_regex.contains("invalid deny pattern") {
        return Err(format!("invalid deny regex did not fail closed: {invalid_regex}").into());
    }
    Ok(())
}

#[test]
fn exec_tool_truncates_multibyte_output_without_panicking() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = ExecTool::with_workspace(temp.path());
    let result = tool
        .execute(json_map(json!({
            "command": "printf '한%.0s' {1..12000}",
            "timeout": 5
        }))?)
        .into_text();
    if !result.contains("chars truncated") || !result.contains("Exit code: 0") {
        return Err(format!("multibyte output was not truncated safely: {result}").into());
    }
    Ok(())
}

#[test]
fn exec_tool_times_out_long_running_command() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tool = ExecTool::with_workspace(temp.path());

    let result = tool
        .execute(json_map(json!({ "command": "sleep 2", "timeout": 1 }))?)
        .into_text();
    if !result.contains("Command timed out after 1 seconds") {
        return Err(format!("timeout did not fire: {result}").into());
    }
    Ok(())
}

#[test]
fn sandbox_wraps_bwrap_command_with_workspace_bind() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let wrapped = wrap_command(
        "bwrap",
        "printf 'hello world'",
        temp.path(),
        temp.path(),
        None,
    )?;
    if !wrapped.contains("bwrap --new-session")
        || !wrapped.contains("--bind")
        || !wrapped.contains("printf")
    {
        return Err(format!("unexpected bwrap command: {wrapped}").into());
    }
    Ok(())
}

#[test]
fn web_fetch_blocks_internal_url_before_client_call() -> Result<(), Box<dyn Error>> {
    let tool = WebFetchTool::new(Arc::new(StaticWebClient {
        response: Err("client should not be called".to_owned()),
    }));
    let result = tool
        .execute(json_map(json!({ "url": "http://169.254.169.254/latest" }))?)
        .into_text();
    let value: Value = serde_json::from_str(&result)?;
    if !value
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("URL validation failed"))
    {
        return Err(format!("internal URL was not blocked: {result}").into());
    }

    let mapped = tool
        .execute(json_map(
            json!({ "url": "http://[::ffff:127.0.0.1]/latest" }),
        )?)
        .into_text();
    if !mapped.contains("URL validation failed") {
        return Err(format!("IPv4-mapped IPv6 URL was not blocked: {mapped}").into());
    }
    Ok(())
}

#[test]
fn web_fetch_ssrf_whitelist_allows_only_configured_cidrs() -> Result<(), Box<dyn Error>> {
    let tool = WebFetchTool::with_config(
        WebFetchConfig {
            network_guard: shacs_core::tools::NetworkGuard::with_ssrf_whitelist([
                "bad-cidr",
                "100.64.0.0/10",
            ]),
            ..WebFetchConfig::default()
        },
        Arc::new(StaticWebClient {
            response: Ok(shacs_core::tools::HttpResponse {
                final_url: "http://100.64.0.42/ok".to_owned(),
                status: 200,
                content_type: "text/plain".to_owned(),
                body: b"allowed".to_vec(),
            }),
        }),
    );

    let allowed = tool
        .execute(json_map(json!({ "url": "http://100.64.0.42/ok" }))?)
        .into_text();
    if !allowed.contains("allowed") {
        return Err(format!("whitelisted CGNAT URL was not fetched: {allowed}").into());
    }

    let blocked = tool
        .execute(json_map(json!({ "url": "http://169.254.169.254/latest" }))?)
        .into_text();
    if !blocked.contains("URL validation failed") {
        return Err(format!("metadata URL was not still blocked: {blocked}").into());
    }
    Ok(())
}

#[test]
fn web_fetch_extracts_html_as_markdown_with_untrusted_banner() -> Result<(), Box<dyn Error>> {
    let tool = WebFetchTool::new(Arc::new(StaticWebClient {
        response: Ok(shacs_core::tools::HttpResponse {
            final_url: "http://93.184.216.34/docs".to_owned(),
            status: 200,
            content_type: "text/html; charset=utf-8".to_owned(),
            body: br#"<html><head><title>Guide</title><script>bad()</script></head><body><h1>Hello</h1><p>See <a href="/x">link</a></p></body></html>"#.to_vec(),
        }),
    }));
    let result = tool
        .execute(json_map(json!({ "url": "http://93.184.216.34/docs" }))?)
        .into_text();
    let value: Value = serde_json::from_str(&result)?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or("missing text")?;
    if value.get("extractor") != Some(&Value::String("readability".to_owned()))
        || !text.contains("[External content")
        || !text.contains("# Guide")
        || !text.contains("# Hello")
        || text.contains("bad()")
    {
        return Err(format!("unexpected html extraction: {result}").into());
    }
    Ok(())
}

#[test]
fn web_fetch_blocks_private_redirect_and_truncates_text() -> Result<(), Box<dyn Error>> {
    let config = WebFetchConfig {
        max_chars: 100,
        ..WebFetchConfig::default()
    };
    let redirect_tool = WebFetchTool::with_config(
        config.clone(),
        Arc::new(StaticWebClient {
            response: Ok(shacs_core::tools::HttpResponse {
                final_url: "http://127.0.0.1/private".to_owned(),
                status: 200,
                content_type: "text/plain".to_owned(),
                body: b"secret".to_vec(),
            }),
        }),
    );
    let blocked = redirect_tool
        .execute(json_map(json!({ "url": "http://93.184.216.34/start" }))?)
        .into_text();
    if !blocked.contains("Redirect blocked") {
        return Err(format!("private redirect was not blocked: {blocked}").into());
    }

    let truncating_tool = WebFetchTool::with_config(
        config,
        Arc::new(StaticWebClient {
            response: Ok(shacs_core::tools::HttpResponse {
                final_url: "http://93.184.216.34/long".to_owned(),
                status: 200,
                content_type: "text/plain".to_owned(),
                body: "한".repeat(120).into_bytes(),
            }),
        }),
    );
    let truncated = truncating_tool
        .execute(json_map(json!({ "url": "http://93.184.216.34/long" }))?)
        .into_text();
    let value: Value = serde_json::from_str(&truncated)?;
    if value.get("truncated") != Some(&Value::Bool(true))
        || !value
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("[External content"))
    {
        return Err(format!("text was not truncated safely: {truncated}").into());
    }
    Ok(())
}

#[test]
fn web_fetch_returns_image_content_blocks_and_http_errors() -> Result<(), Box<dyn Error>> {
    let image_tool = WebFetchTool::new(Arc::new(StaticWebClient {
        response: Ok(shacs_core::tools::HttpResponse {
            final_url: "http://93.184.216.34/image.png".to_owned(),
            status: 200,
            content_type: "image/png".to_owned(),
            body: vec![1, 2, 3],
        }),
    }));
    let image = image_tool
        .execute(json_map(
            json!({ "url": "http://93.184.216.34/image.png" }),
        )?)
        .into_text();
    let image_value: Value = serde_json::from_str(&image)?;
    if !image_value.is_array()
        || image_value.pointer("/0/type") != Some(&Value::String("image_url".to_owned()))
        || !image_value
            .pointer("/0/image_url/url")
            .and_then(Value::as_str)
            .is_some_and(|url| url.starts_with("data:image/png;base64,"))
    {
        return Err(format!("image response did not match content block shape: {image}").into());
    }

    let error_tool = WebFetchTool::new(Arc::new(StaticWebClient {
        response: Ok(shacs_core::tools::HttpResponse {
            final_url: "http://93.184.216.34/missing".to_owned(),
            status: 404,
            content_type: "text/plain".to_owned(),
            body: b"not found".to_vec(),
        }),
    }));
    let error = error_tool
        .execute(json_map(json!({ "url": "http://93.184.216.34/missing" }))?)
        .into_text();
    let error_value: Value = serde_json::from_str(&error)?;
    if !error_value
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("HTTP status 404"))
    {
        return Err(format!("HTTP error was not reported as JSON error: {error}").into());
    }
    Ok(())
}

#[test]
fn web_search_schema_and_duckduckgo_exclusivity_match_reference() -> Result<(), Box<dyn Error>> {
    let duck = WebSearchTool::default();
    if !duck.read_only() || !duck.exclusive() {
        return Err("duckduckgo web_search should be read-only and exclusive".into());
    }
    let brave = WebSearchTool::new(WebSearchConfig {
        provider: "brave".to_owned(),
        api_key: "key".to_owned(),
        ..WebSearchConfig::default()
    });
    if brave.exclusive() {
        return Err("non-duckduckgo web_search should not be exclusive".into());
    }
    let schema = brave.parameters();
    if schema
        .pointer("/properties/count/maximum")
        .and_then(Value::as_i64)
        != Some(10)
    {
        return Err(format!("web_search count maximum missing: {schema}").into());
    }
    Ok(())
}

#[test]
fn web_search_formats_results_and_strips_untrusted_html() -> Result<(), Box<dyn Error>> {
    let tool = WebSearchTool::with_client(
        WebSearchConfig {
            provider: "duckduckgo".to_owned(),
            ..WebSearchConfig::default()
        },
        Arc::new(StaticSearchClient {
            results: vec![
                WebSearchResult {
                    title: "<b>One &amp; Two</b>".to_owned(),
                    url: "https://example.com/one".to_owned(),
                    content: "<script>bad()</script><p>Useful&nbsp;snippet</p>".to_owned(),
                },
                WebSearchResult {
                    title: "Ignored".to_owned(),
                    url: "https://example.com/two".to_owned(),
                    content: "second".to_owned(),
                },
            ],
        }),
    );
    let result = tool
        .execute(json_map(json!({ "query": "rust", "count": 1 }))?)
        .into_text();
    if !result.contains("Results for: rust")
        || !result.contains("1. One & Two")
        || !result.contains("Useful snippet")
        || !result.contains("https://example.com/one")
        || result.contains("bad()")
        || result.contains("Ignored")
    {
        return Err(format!("unexpected search formatting: {result}").into());
    }
    Ok(())
}

#[test]
fn web_search_wires_json_providers() -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            "brave",
            json!({ "web": { "results": [{ "title": "Brave", "url": "https://brave.example", "description": "brave body" }] } }),
            "Brave",
            "GET https://api.search.brave.com/res/v1/web/search",
        ),
        (
            "tavily",
            json!({ "results": [{ "title": "Tavily", "url": "https://tavily.example", "content": "tavily body" }] }),
            "Tavily",
            "POST https://api.tavily.com/search",
        ),
        (
            "searxng",
            json!({ "results": [{ "title": "SearX", "url": "https://searx.example", "content": "searx body" }] }),
            "SearX",
            "GET http://93.184.216.34/search",
        ),
        (
            "jina",
            json!({ "data": [{ "title": "Jina", "url": "https://jina.example", "content": "jina body" }] }),
            "Jina",
            "GET https://s.jina.ai/rust",
        ),
        (
            "kagi",
            json!({ "data": [{ "t": 0, "title": "Kagi", "url": "https://kagi.example", "snippet": "kagi body" }, { "t": 1, "title": "Ignored" }] }),
            "Kagi",
            "GET https://kagi.com/api/v0/search",
        ),
        (
            "olostep",
            json!({ "result": { "links": [{ "title": "Olostep", "url": "https://olostep.example", "description": "olostep body" }] } }),
            "Olostep",
            "POST https://api.olostep.com/v1/searches",
        ),
    ];

    for (provider, body, expected_title, expected_call) in cases {
        let http = StaticSearchHttpClient::new(SearchHttpResponse {
            status: 200,
            body: serde_json::to_string(&body)?,
        });
        let calls = http.calls.clone();
        let tool = WebSearchTool::with_client(
            WebSearchConfig {
                provider: provider.to_owned(),
                api_key: "key".to_owned(),
                base_url: "http://93.184.216.34".to_owned(),
                ..WebSearchConfig::default()
            },
            Arc::new(UreqWebSearchClient::with_http(Arc::new(http))),
        );
        let result = tool
            .execute(json_map(json!({ "query": "rust", "count": 2 }))?)
            .into_text();
        if !result.contains(expected_title) || result.contains("Ignored") {
            return Err(format!("{provider} result was not parsed correctly: {result}").into());
        }
        let calls = calls.lock().map_err(|error| error.to_string())?;
        if !calls
            .first()
            .is_some_and(|call| call.starts_with(expected_call))
        {
            return Err(format!("{provider} endpoint mismatch: {calls:?}").into());
        }
    }
    Ok(())
}

#[test]
fn web_search_blocks_private_searxng_base_url() -> Result<(), Box<dyn Error>> {
    let tool = WebSearchTool::with_client(
        WebSearchConfig {
            provider: "searxng".to_owned(),
            base_url: "http://127.0.0.1:8888".to_owned(),
            ..WebSearchConfig::default()
        },
        Arc::new(UreqWebSearchClient::with_http(Arc::new(
            StaticSearchHttpClient::new(SearchHttpResponse {
                status: 200,
                body: "{}".to_owned(),
            }),
        ))),
    );

    let result = tool
        .execute(json_map(json!({ "query": "rust" }))?)
        .into_text();
    if !result.contains("private/internal") {
        return Err(format!("private searxng base URL was not blocked: {result}").into());
    }
    Ok(())
}

#[test]
fn web_search_duckduckgo_fallback_and_json_parsing() -> Result<(), Box<dyn Error>> {
    let http = StaticSearchHttpClient::new(SearchHttpResponse {
        status: 200,
        body: serde_json::to_string(&json!({
            "Heading": "Duck & Result",
            "AbstractURL": "https://example.com/doc",
            "AbstractText": "Snippet text",
            "RelatedTopics": [{ "Text": "Related", "FirstURL": "https://example.com/related" }]
        }))?,
    });
    let calls = http.calls.clone();
    let tool = WebSearchTool::with_client(
        WebSearchConfig {
            provider: "brave".to_owned(),
            api_key: String::new(),
            ..WebSearchConfig::default()
        },
        Arc::new(UreqWebSearchClient::with_http(Arc::new(http))),
    );
    let result = tool
        .execute(json_map(json!({ "query": "rust", "count": 1 }))?)
        .into_text();
    if !result.contains("Duck & Result")
        || !result.contains("https://example.com/doc")
        || !result.contains("Snippet text")
    {
        return Err(format!("duckduckgo fallback was not parsed correctly: {result}").into());
    }
    let calls = calls.lock().map_err(|error| error.to_string())?;
    if !calls
        .first()
        .is_some_and(|call| call.starts_with("GET https://api.duckduckgo.com/"))
    {
        return Err(format!("missing DuckDuckGo fallback call: {calls:?}").into());
    }
    Ok(())
}

#[test]
fn cron_tool_adds_every_job_with_context_payload() -> Result<(), Box<dyn Error>> {
    let service = Arc::new(InMemoryCronService::new());
    let tool = CronTool::with_timezone(service.clone(), "UTC");

    let no_context = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "ping",
            "every_seconds": 60
        }))?)
        .into_text();
    if !no_context.contains("no session context") {
        return Err(format!("cron add without context did not fail: {no_context}").into());
    }

    tool.set_context(
        "telegram",
        "chat-1",
        Some(json!({ "thread": "abc" })),
        Some("session-1".to_owned()),
    );
    let result = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "Send reminder",
            "every_seconds": 90,
            "deliver": true
        }))?)
        .into_text();
    if !result.contains("Created job 'Send reminder'") {
        return Err(format!("unexpected add result: {result}").into());
    }
    let jobs = service.list_jobs();
    let job = jobs.first().ok_or("missing added job")?;
    if job.schedule.every_ms != Some(90_000)
        || job.payload.channel.as_deref() != Some("telegram")
        || job.payload.to.as_deref() != Some("chat-1")
        || job.payload.session_key.as_deref() != Some("session-1")
        || !job.payload.deliver
    {
        return Err(format!("added job did not preserve payload: {job:?}").into());
    }
    Ok(())
}

#[test]
fn cron_tool_supports_cron_at_and_thread_local_context() -> Result<(), Box<dyn Error>> {
    let service = Arc::new(InMemoryCronService::new());
    let tool = CronTool::with_timezone(service.clone(), "Asia/Seoul");

    tool.set_context("slack", "C1", None, None);
    let cron_result = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "cron reminder",
            "cron_expr": "0 9 * * *"
        }))?)
        .into_text();
    if !cron_result.contains("Created job 'cron reminder'") {
        return Err(format!("unexpected cron add result: {cron_result}").into());
    }

    let at_result = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "one shot",
            "at": "2026-05-02T10:30:00.123"
        }))?)
        .into_text();
    if !at_result.contains("Created job 'one shot'") {
        return Err(format!("fractional at add failed: {at_result}").into());
    }

    let jobs = service.list_jobs();
    let cron_job = jobs
        .iter()
        .find(|job| job.payload.message == "cron reminder")
        .ok_or("missing cron job")?;
    if cron_job.schedule.expr.as_deref() != Some("0 9 * * *")
        || cron_job.schedule.tz.as_deref() != Some("Asia/Seoul")
        || cron_job.payload.session_key.as_deref() != Some("slack:C1")
        || !cron_job.payload.deliver
    {
        return Err(format!("cron job defaults were not preserved: {cron_job:?}").into());
    }
    let at_job = jobs
        .iter()
        .find(|job| job.payload.message == "one shot")
        .ok_or("missing at job")?;
    if at_job.schedule.at_ms.is_none() || !at_job.delete_after_run {
        return Err(format!("at job was not one-shot: {at_job:?}").into());
    }

    let barrier = Arc::new(Barrier::new(2));
    let first_tool = tool.clone();
    let second_tool = tool.clone();
    let first_barrier = barrier.clone();
    let first = std::thread::spawn(move || -> Result<String, String> {
        first_tool.set_context("thread-a", "A", None, None);
        first_barrier.wait();
        Ok(first_tool
            .execute(
                json_map(json!({
                    "action": "add",
                    "message": "thread A",
                    "every_seconds": 1
                }))
                .map_err(|error| error.to_string())?,
            )
            .into_text())
    });
    let second_barrier = barrier.clone();
    let second = std::thread::spawn(move || -> Result<String, String> {
        second_tool.set_context("thread-b", "B", None, None);
        second_barrier.wait();
        Ok(second_tool
            .execute(
                json_map(json!({
                    "action": "add",
                    "message": "thread B",
                    "every_seconds": 1
                }))
                .map_err(|error| error.to_string())?,
            )
            .into_text())
    });
    first
        .join()
        .map_err(|_| std::io::Error::other("thread A panicked"))?
        .map_err(std::io::Error::other)?;
    second
        .join()
        .map_err(|_| std::io::Error::other("thread B panicked"))?
        .map_err(std::io::Error::other)?;

    let jobs = service.list_jobs();
    let thread_a = jobs
        .iter()
        .find(|job| job.payload.message == "thread A")
        .ok_or("missing thread A job")?;
    let thread_b = jobs
        .iter()
        .find(|job| job.payload.message == "thread B")
        .ok_or("missing thread B job")?;
    if thread_a.payload.channel.as_deref() != Some("thread-a")
        || thread_b.payload.channel.as_deref() != Some("thread-b")
    {
        return Err(format!("thread-local cron context leaked: {thread_a:?} {thread_b:?}").into());
    }
    Ok(())
}

#[test]
fn cron_tool_validates_schedule_and_context_rules() -> Result<(), Box<dyn Error>> {
    let service = Arc::new(InMemoryCronService::new());
    let tool = CronTool::with_timezone(service, "Asia/Seoul");
    tool.set_context("slack", "C1", None, None);

    let missing_message_errors = tool.validate_params(&json_map(json!({ "action": "add" }))?);
    if !missing_message_errors
        .iter()
        .any(|error| error.render().contains("message is required"))
    {
        return Err(format!("missing add validation error: {missing_message_errors:?}").into());
    }

    let tz_without_cron = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "bad tz",
            "every_seconds": 10,
            "tz": "UTC"
        }))?)
        .into_text();
    if !tz_without_cron.contains("tz can only be used with cron_expr") {
        return Err(format!("tz without cron_expr was not rejected: {tz_without_cron}").into());
    }

    let bad_time = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "bad time",
            "at": "not-a-date"
        }))?)
        .into_text();
    if !bad_time.contains("invalid ISO datetime") {
        return Err(format!("invalid at datetime was not rejected: {bad_time}").into());
    }

    let previous = tool.set_cron_context(true);
    let nested = tool
        .execute(json_map(json!({
            "action": "add",
            "message": "nested",
            "every_seconds": 10
        }))?)
        .into_text();
    tool.reset_cron_context(previous);
    if !nested.contains("cannot schedule new jobs") {
        return Err(format!("nested cron add was not rejected: {nested}").into());
    }
    Ok(())
}

#[test]
fn cron_tool_lists_state_and_protected_system_jobs() -> Result<(), Box<dyn Error>> {
    let mut dream = system_job("dream", "dream", CronSchedule::cron("0 */2 * * *", "UTC"));
    dream.state = CronJobState {
        next_run_at_ms: Some(1_700_000_000_000),
        last_run_at_ms: Some(1_699_999_000_000),
        last_status: Some(CronRunStatus::Error),
        last_error: Some("boom".to_owned()),
        run_history: Vec::new(),
    };
    let service = Arc::new(InMemoryCronService::with_jobs(vec![dream]));
    let tool = CronTool::new(service);

    let list = tool
        .execute(json_map(json!({ "action": "list" }))?)
        .into_text();
    if !list.contains("Scheduled jobs:")
        || !list.contains("dream")
        || !list.contains("Protected: visible")
        || !list.contains("Dream memory consolidation")
        || !list.contains("Last run:")
        || !list.contains("boom")
        || !list.contains("Next run:")
    {
        return Err(format!("unexpected cron list output: {list}").into());
    }
    Ok(())
}

#[test]
fn cron_tool_removes_jobs_and_protects_dream() -> Result<(), Box<dyn Error>> {
    let mut normal = system_job("normal", "normal", CronSchedule::every(60_000));
    normal.payload.kind = shacs_cron::CronPayloadKind::AgentTurn;
    let dream = system_job("dream", "dream", CronSchedule::every(60_000));
    let service = Arc::new(InMemoryCronService::with_jobs(vec![normal, dream]));
    let tool = CronTool::new(service);

    let removed = tool
        .execute(json_map(json!({ "action": "remove", "job_id": "normal" }))?)
        .into_text();
    if removed != "Removed job normal" {
        return Err(format!("unexpected remove result: {removed}").into());
    }

    let protected = tool
        .execute(json_map(json!({ "action": "remove", "job_id": "dream" }))?)
        .into_text();
    if !protected.contains("Cannot remove job `dream`") {
        return Err(format!("dream job was not protected: {protected}").into());
    }

    let missing = tool
        .execute(json_map(
            json!({ "action": "remove", "job_id": "missing" }),
        )?)
        .into_text();
    if missing != "Job missing not found" {
        return Err(format!("unexpected missing remove result: {missing}").into());
    }
    Ok(())
}

#[test]
fn message_tool_sends_with_context_media_buttons_and_metadata() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let sent = Arc::new(Mutex::new(Vec::<OutboundMessage>::new()));
    let capture = sent.clone();
    let tool = shacs_core::tools::MessageTool::with_sender(
        temp.path(),
        Arc::new(move |message: OutboundMessage| {
            capture
                .lock()
                .map_err(|error| error.to_string())?
                .push(message);
            Ok(())
        }),
        "telegram",
        "chat-1",
        Some("msg-1".to_owned()),
    );
    tool.set_context(
        "telegram",
        "chat-1",
        Some("ctx-msg".to_owned()),
        Some(json!({ "thread": "abc" })),
    );
    let previous = tool.set_record_channel_delivery(true);

    let result = tool
        .execute(json_map(json!({
            "content": "<think>hidden</think> hello",
            "media": ["image.png", "https://example.com/a.jpg", temp.path().join("doc.pdf")],
            "buttons": [["Yes", "No"], ["Later"]]
        }))?)
        .into_text();
    tool.reset_record_channel_delivery(previous);

    if result != "Message sent to telegram:chat-1 with 3 attachments with 3 button(s)" {
        return Err(format!("unexpected message result: {result}").into());
    }
    if !tool.sent_in_turn() {
        return Err("same-target message did not mark sent_in_turn".into());
    }
    let messages = sent.lock().map_err(|error| error.to_string())?;
    let message = messages.first().ok_or("missing sent message")?;
    let expected_relative_media = temp.path().join("image.png").to_string_lossy().into_owned();
    if message.content != "hello"
        || message.channel != "telegram"
        || message.chat_id != "chat-1"
        || message.media.first().map(String::as_str) != Some(expected_relative_media.as_str())
        || message.media.get(1).map(String::as_str) != Some("https://example.com/a.jpg")
        || message.buttons
            != vec![
                vec!["Yes".to_owned(), "No".to_owned()],
                vec!["Later".to_owned()],
            ]
        || message.metadata["thread"] != "abc"
        || message.metadata["message_id"] != "ctx-msg"
        || message.metadata["_record_channel_delivery"] != true
    {
        return Err(format!("message payload mismatch: {message:?}").into());
    }
    Ok(())
}

#[test]
fn message_tool_validates_target_sender_buttons_and_cross_chat_reply() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let unconfigured = shacs_core::tools::MessageTool::new(temp.path());
    let no_target = unconfigured
        .execute(json_map(json!({ "content": "hi" }))?)
        .into_text();
    if no_target != "Error: No target channel/chat specified" {
        return Err(format!("unexpected missing target result: {no_target}").into());
    }

    let configured_without_sender =
        shacs_core::tools::MessageTool::with_defaults(temp.path(), "slack", "C1", None);
    let no_sender = configured_without_sender
        .execute(json_map(json!({ "content": "hi" }))?)
        .into_text();
    if no_sender != "Error: Message sending not configured" {
        return Err(format!("unexpected missing sender result: {no_sender}").into());
    }

    let bad_buttons = configured_without_sender
        .execute(json_map(json!({ "content": "hi", "buttons": ["bad"] }))?)
        .into_text();
    if bad_buttons != "Error: buttons must be a list of list of strings" {
        return Err(format!("unexpected bad buttons result: {bad_buttons}").into());
    }

    let sent = Arc::new(Mutex::new(Vec::<OutboundMessage>::new()));
    let capture = sent.clone();
    let tool = shacs_core::tools::MessageTool::with_sender(
        temp.path(),
        Arc::new(move |message: OutboundMessage| {
            capture
                .lock()
                .map_err(|error| error.to_string())?
                .push(message);
            Ok(())
        }),
        "slack",
        "C1",
        Some("default-reply".to_owned()),
    );
    tool.set_context(
        "slack",
        "C1",
        Some("default-reply".to_owned()),
        Some(json!({ "thread": "same-target-only" })),
    );
    let cross_chat = tool
        .execute(json_map(json!({
            "content": "cross chat",
            "channel": "slack",
            "chat_id": "C2",
            "message_id": "must-not-leak"
        }))?)
        .into_text();
    if cross_chat != "Message sent to slack:C2" {
        return Err(format!("unexpected cross-chat result: {cross_chat}").into());
    }
    if tool.sent_in_turn() {
        return Err("cross-chat message should not mark sent_in_turn".into());
    }
    let messages = sent.lock().map_err(|error| error.to_string())?;
    let message = messages.first().ok_or("missing cross-chat message")?;
    if message.metadata != json!({}) {
        return Err(format!("cross-chat metadata leaked: {message:?}").into());
    }
    Ok(())
}

#[test]
fn message_tool_preserves_schema_and_edge_behaviors() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let sent = Arc::new(Mutex::new(Vec::<OutboundMessage>::new()));
    let capture = sent.clone();
    let tool = shacs_core::tools::MessageTool::with_sender(
        temp.path(),
        Arc::new(move |message: OutboundMessage| {
            capture
                .lock()
                .map_err(|error| error.to_string())?
                .push(message);
            Ok(())
        }),
        "discord",
        "D1",
        Some("default-message".to_owned()),
    );

    let schema = tool.parameters();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("missing message schema properties")?;
    if properties.contains_key("message_id") {
        return Err("message_id should not be exposed in the LLM-facing schema".into());
    }
    let missing_content = tool.validate_params(&json_map(json!({}))?);
    if !missing_content
        .iter()
        .any(|error| error.render().contains("missing required content"))
    {
        return Err(format!("missing content validation error: {missing_content:?}").into());
    }

    let result = tool
        .execute(json_map(json!({
            "content": "<|channel|> visible <think",
            "message_id": "explicit-message"
        }))?)
        .into_text();
    if result != "Message sent to discord:D1" {
        return Err(format!("unexpected same-target send result: {result}").into());
    }
    if !tool.sent_in_turn() {
        return Err("same-target send did not mark sent_in_turn".into());
    }
    tool.start_turn();
    if tool.sent_in_turn() {
        return Err("start_turn did not reset sent_in_turn".into());
    }
    let messages = sent.lock().map_err(|error| error.to_string())?;
    let message = messages.first().ok_or("missing same-target message")?;
    if message.content != "visible" || message.metadata["message_id"] != "explicit-message" {
        return Err(format!("strip_think or message_id override failed: {message:?}").into());
    }
    drop(messages);

    let previous = tool.set_record_channel_delivery(true);
    let cross_delivery = tool
        .execute(json_map(json!({
            "content": "proactive",
            "channel": "discord",
            "chat_id": "D2"
        }))?)
        .into_text();
    tool.reset_record_channel_delivery(previous);
    if cross_delivery != "Message sent to discord:D2" {
        return Err(format!("unexpected cross delivery result: {cross_delivery}").into());
    }
    let messages = sent.lock().map_err(|error| error.to_string())?;
    let proactive = messages.get(1).ok_or("missing proactive message")?;
    if proactive.metadata != json!({ "_record_channel_delivery": true }) {
        return Err(format!("cross-chat delivery metadata mismatch: {proactive:?}").into());
    }
    drop(messages);

    let failing = shacs_core::tools::MessageTool::with_sender(
        temp.path(),
        Arc::new(|_message: OutboundMessage| Err("offline".to_owned())),
        "discord",
        "D1",
        None,
    );
    let error = failing
        .execute(json_map(json!({ "content": "hi" }))?)
        .into_text();
    if error != "Error sending message: offline" {
        return Err(format!("sender error was not surfaced: {error}").into());
    }
    Ok(())
}

#[test]
fn self_tool_checks_summary_paths_and_redacts_sensitive_fields() -> Result<(), Box<dyn Error>> {
    let state = Arc::new(Mutex::new(SelfRuntimeState::new()));
    {
        let mut state = state.lock().map_err(|error| error.to_string())?;
        state.insert_value(
            "web_config",
            json!({
                "enable": true,
                "search": {
                    "provider": "tavily",
                    "api_key": "sk-secret-key"
                }
            }),
        );
        state.set_scratchpad("task", json!("review"));
    }
    let tool = SelfTool::new(state);
    if tool.name() != "my" {
        return Err("self tool name should be my".into());
    }
    let schema = tool.parameters();
    if !schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.contains(&json!("action")))
    {
        return Err(format!("my action is not required: {schema}").into());
    }

    let summary = tool
        .execute(json_map(json!({ "action": "check" }))?)
        .into_text();
    if !summary.contains("max_iterations: 40")
        || !summary.contains("context_window_tokens: 65536")
        || !summary.contains("model:")
        || !summary.contains("scratchpad")
        || summary.contains("sk-secret-key")
        || summary.contains("api_key")
    {
        return Err(format!("unexpected self summary: {summary}").into());
    }

    let parent = tool
        .execute(json_map(json!({ "action": "check", "key": "web_config" }))?)
        .into_text();
    if !parent.contains("search") || parent.contains("sk-secret-key") || parent.contains("api_key")
    {
        return Err(format!("parent object leaked sensitive field: {parent}").into());
    }

    let nested = tool
        .execute(json_map(
            json!({ "action": "check", "key": "web_config.search" }),
        )?)
        .into_text();
    if !nested.contains("provider")
        || nested.contains("sk-secret-key")
        || nested.contains("api_key")
    {
        return Err(format!("sensitive nested field was not redacted: {nested}").into());
    }

    let usage = tool
        .execute(json_map(
            json!({ "action": "check", "key": "_last_usage.prompt_tokens" }),
        )?)
        .into_text();
    if !usage.contains("100") {
        return Err(format!("dot-path dict lookup failed: {usage}").into());
    }

    let scratch = tool
        .execute(json_map(json!({ "action": "check", "key": "task" }))?)
        .into_text();
    if !scratch.contains("review") {
        return Err(format!("scratchpad fallback failed: {scratch}").into());
    }
    Ok(())
}

#[test]
fn self_tool_blocks_sensitive_and_read_only_paths() -> Result<(), Box<dyn Error>> {
    let state = Arc::new(Mutex::new(SelfRuntimeState::new()));
    let tool = SelfTool::new(state);

    for key in [
        "bus",
        "tools",
        "__class__",
        "_mcp_servers",
        "web_config.search.api_key",
    ] {
        let checked = tool
            .execute(json_map(json!({ "action": "check", "key": key }))?)
            .into_text();
        if !checked.contains("not accessible") {
            return Err(format!("{key} should not be inspectable: {checked}").into());
        }
    }

    let blocked = tool
        .execute(json_map(
            json!({ "action": "set", "key": "runner", "value": "bad" }),
        )?)
        .into_text();
    if !blocked.contains("protected") {
        return Err(format!("runner should be protected: {blocked}").into());
    }

    let read_only = tool
        .execute(json_map(
            json!({ "action": "set", "key": "subagents", "value": {} }),
        )?)
        .into_text();
    if !read_only.contains("read-only") {
        return Err(format!("subagents should be read-only: {read_only}").into());
    }
    Ok(())
}

#[test]
fn self_tool_sets_restricted_values_and_scratchpad() -> Result<(), Box<dyn Error>> {
    let state = Arc::new(Mutex::new(SelfRuntimeState::new()));
    let tool = SelfTool::new(state.clone());

    let max_iterations = tool
        .execute(json_map(
            json!({ "action": "set", "key": "max_iterations", "value": "80" }),
        )?)
        .into_text();
    if !max_iterations.contains("Set max_iterations = 80") {
        return Err(format!("max_iterations set failed: {max_iterations}").into());
    }
    {
        let state = state.lock().map_err(|error| error.to_string())?;
        if state.get_value("max_iterations") != Some(&json!(80))
            || state.max_iterations_syncs() != 1
        {
            return Err(format!("max_iterations state mismatch: {state:?}").into());
        }
    }

    for (value, expected) in [
        (json!(0), ">= 1"),
        (json!(999), "<= 100"),
        (json!(true), "got bool"),
        (json!(null), "got NoneType"),
    ] {
        let result = tool
            .execute(json_map(
                json!({ "action": "set", "key": "max_iterations", "value": value }),
            )?)
            .into_text();
        if !result.contains(expected) {
            return Err(format!("restricted validation mismatch: {result}").into());
        }
    }

    let scratch = tool
        .execute(json_map(
            json!({ "action": "set", "key": "preference", "value": {"tone": "concise"} }),
        )?)
        .into_text();
    if !scratch.contains("Set scratchpad.preference") {
        return Err(format!("scratchpad set failed: {scratch}").into());
    }
    {
        let state = state.lock().map_err(|error| error.to_string())?;
        if state.get_scratchpad("preference") != Some(&json!({"tone": "concise"})) {
            return Err(format!("scratchpad value mismatch: {state:?}").into());
        }
    }

    let type_mismatch = tool
        .execute(json_map(
            json!({ "action": "set", "key": "provider_retry_mode", "value": 42 }),
        )?)
        .into_text();
    if !type_mismatch.contains("expects str") {
        return Err(
            format!("existing attr type mismatch was not rejected: {type_mismatch}").into(),
        );
    }
    Ok(())
}

#[test]
fn self_tool_enforces_scratchpad_limits_json_safety_and_read_only_mode(
) -> Result<(), Box<dyn Error>> {
    let state = Arc::new(Mutex::new(SelfRuntimeState::new()));
    {
        let mut state = state.lock().map_err(|error| error.to_string())?;
        for index in 0..64 {
            state.set_scratchpad(format!("key_{index}"), json!(index));
        }
    }
    let tool = SelfTool::new(state.clone());
    let overflow = tool
        .execute(json_map(
            json!({ "action": "set", "key": "overflow", "value": "data" }),
        )?)
        .into_text();
    if !overflow.contains("scratchpad is full") {
        return Err(format!("scratchpad overflow was not rejected: {overflow}").into());
    }
    let update_existing = tool
        .execute(json_map(
            json!({ "action": "set", "key": "key_0", "value": "updated" }),
        )?)
        .into_text();
    if update_existing.contains("Error") {
        return Err(
            format!("existing scratchpad update should be allowed: {update_existing}").into(),
        );
    }

    let mut deep = json!({"level": 0});
    for _ in 0..12 {
        deep = json!({ "child": deep });
    }
    let too_deep = tool
        .execute(json_map(
            json!({ "action": "set", "key": "deep", "value": deep }),
        )?)
        .into_text();
    if !too_deep.contains("nesting too deep") {
        return Err(format!("deep nesting was not rejected: {too_deep}").into());
    }

    let read_only = SelfTool::with_modify_allowed(state, false);
    let disabled = read_only
        .execute(json_map(
            json!({ "action": "set", "key": "max_iterations", "value": 50 }),
        )?)
        .into_text();
    if !disabled.contains("set is disabled") || !read_only.description().contains("READ-ONLY MODE")
    {
        return Err(format!("read-only mode did not reject set: {disabled}").into());
    }
    Ok(())
}

#[test]
fn mcp_helpers_sanitize_names_and_normalize_nullable_schema() -> Result<(), Box<dyn Error>> {
    if sanitize_mcp_name("mcp:server/tool name!!") != "mcp_server_tool_name_" {
        return Err("MCP name sanitizer did not replace invalid characters".into());
    }
    if !is_transient_mcp_error("ConnectionResetError") || is_transient_mcp_error("ValueError") {
        return Err("MCP transient error detection drifted".into());
    }

    let schema = normalize_schema_for_openai(json!({
        "type": "object",
        "properties": {
            "maybe": {"type": ["string", "null"]},
            "nested": {
                "oneOf": [
                    {"type": "null"},
                    {"type": "object", "properties": {"count": {"type": ["integer", "null"]}}}
                ]
            },
            "items": {"type": "array", "items": {"anyOf": [{"type": "null"}, {"type": "number"}]}}
        }
    }));
    if schema["properties"]["maybe"]["type"] != "string"
        || schema["properties"]["maybe"]["nullable"] != true
        || schema["properties"]["nested"]["nullable"] != true
        || schema["properties"]["nested"]["properties"]["count"]["nullable"] != true
        || schema["properties"]["items"]["items"]["nullable"] != true
        || !schema["required"].as_array().is_some_and(Vec::is_empty)
    {
        return Err(format!("nullable schema was not normalized: {schema}").into());
    }
    Ok(())
}

#[test]
fn mcp_server_spec_detects_transport_and_normalizes_stdio_command() -> Result<(), Box<dyn Error>> {
    let stdio = McpServerSpec {
        name: "fs".to_owned(),
        r#type: None,
        command: Some("node".to_owned()),
        args: vec!["server.js".to_owned()],
        env: Vec::new(),
        url: None,
        headers: Vec::new(),
        timeout_seconds: 30,
        enabled_tools: vec!["*".to_owned()],
    };
    if stdio.transport_kind() != Some(McpTransportKind::Stdio)
        || stdio.normalized_stdio_command()
            != Some(("node".to_owned(), vec!["server.js".to_owned()]))
    {
        return Err(format!("stdio MCP spec was not normalized: {stdio:?}").into());
    }

    let sse = McpServerSpec {
        name: "remote".to_owned(),
        r#type: None,
        command: None,
        args: Vec::new(),
        env: Vec::new(),
        url: Some("https://example.test/sse".to_owned()),
        headers: Vec::new(),
        timeout_seconds: 30,
        enabled_tools: Vec::new(),
    };
    let http = McpServerSpec {
        url: Some("https://example.test/mcp".to_owned()),
        ..sse.clone()
    };
    if sse.transport_kind() != Some(McpTransportKind::Sse)
        || http.transport_kind() != Some(McpTransportKind::StreamableHttp)
    {
        return Err("MCP URL transport inference drifted".into());
    }

    let npx = McpServerSpec {
        name: "npm".to_owned(),
        r#type: Some("stdio".to_owned()),
        command: Some("npx".to_owned()),
        args: vec!["server".to_owned()],
        env: Vec::new(),
        url: None,
        headers: Vec::new(),
        timeout_seconds: 30,
        enabled_tools: Vec::new(),
    };
    let normalized = npx
        .normalized_stdio_command()
        .ok_or("npx stdio command did not normalize")?;
    if cfg!(windows) {
        if normalized
            != (
                "cmd.exe".to_owned(),
                vec![
                    "/d".to_owned(),
                    "/c".to_owned(),
                    "npx".to_owned(),
                    "server".to_owned(),
                ],
            )
        {
            return Err(format!("Windows npx wrapper drifted: {normalized:?}").into());
        }
    } else if normalized != ("npx".to_owned(), vec!["server".to_owned()]) {
        return Err(format!("non-Windows npx wrapper drifted: {normalized:?}").into());
    }
    Ok(())
}

#[test]
fn mcp_runtime_connects_registers_and_closes_servers() -> Result<(), Box<dyn Error>> {
    struct RecordingConnector {
        closed: Arc<Mutex<Vec<String>>>,
    }

    impl McpConnector for RecordingConnector {
        fn connect(
            &self,
            spec: &McpServerSpec,
        ) -> Result<(Arc<dyn McpClient>, Vec<McpCapability>), String> {
            let client: Arc<dyn McpClient> = Arc::new(|_operation: McpOperation| {
                McpCallOutcome::Success(vec!["connected".to_owned()])
            });
            Ok((
                client,
                vec![mcp_tool_capability(&spec.name, "ping", Some("Ping"))],
            ))
        }

        fn close(&self, server_name: &str) {
            self.closed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(server_name.to_owned());
        }
    }

    let closed = Arc::new(Mutex::new(Vec::new()));
    let connector = Arc::new(RecordingConnector {
        closed: Arc::clone(&closed),
    });
    let runtime = McpRuntime::new(Some(connector));
    let mut registry = ToolRegistry::new();
    let specs = vec![McpServerSpec {
        name: "srv".to_owned(),
        r#type: Some("stdio".to_owned()),
        command: Some("server".to_owned()),
        args: Vec::new(),
        env: Vec::new(),
        url: None,
        headers: Vec::new(),
        timeout_seconds: 11,
        enabled_tools: vec!["*".to_owned()],
    }];

    let reports = runtime.connect_and_register(&mut registry, &specs);
    if reports.len() != 1
        || !reports[0].connected
        || reports[0].registered_count != 1
        || reports[0].error.is_some()
        || !registry.has("mcp_srv_ping")
    {
        return Err(format!("MCP runtime did not register capabilities: {reports:?}").into());
    }
    let output = registry.execute("mcp_srv_ping", json!({})).into_text();
    if output != "connected" {
        return Err(format!("registered MCP tool did not route to client: {output}").into());
    }
    runtime.close();
    runtime.close();
    if *closed.lock().map_err(|error| error.to_string())? != vec!["srv".to_owned()] {
        return Err(format!("MCP runtime close was not idempotent: {closed:?}").into());
    }
    Ok(())
}

#[test]
fn mcp_registers_tools_resources_prompts_and_filters_enabled_tools() -> Result<(), Box<dyn Error>> {
    let operations = Arc::new(Mutex::new(Vec::<McpOperation>::new()));
    let capture = operations.clone();
    let client = Arc::new(move |operation: McpOperation| {
        capture
            .lock()
            .map_err(|error| error.to_string())
            .expect("record MCP operation")
            .push(operation.clone());
        match operation {
            McpOperation::CallTool { .. } => {
                McpCallOutcome::Success(vec!["tool output".to_owned()])
            }
            McpOperation::ReadResource { .. } => McpCallOutcome::Success(vec![
                "resource text".to_owned(),
                "[Binary resource: 4 bytes]".to_owned(),
            ]),
            McpOperation::GetPrompt { .. } => {
                McpCallOutcome::Success(vec!["prompt text".to_owned()])
            }
        }
    });
    let capabilities = vec![
        mcp_tool_capability("srv.one", "search-docs", Some("Search docs")),
        mcp_tool_capability("srv.one", "skip-me", None),
        McpCapability {
            kind: McpCapabilityKind::Resource,
            server_name: "srv.one".to_owned(),
            name: "README.md".to_owned(),
            description: Some("Readme".to_owned()),
            input_schema: None,
            uri: Some("file://README.md".to_owned()),
            arguments: Vec::new(),
            timeout_seconds: 7,
        },
        McpCapability {
            kind: McpCapabilityKind::Prompt,
            server_name: "srv.one".to_owned(),
            name: "plan".to_owned(),
            description: Some("Plan".to_owned()),
            input_schema: None,
            uri: None,
            arguments: vec![McpPromptArgument {
                name: "topic".to_owned(),
                description: Some("Topic".to_owned()),
                required: true,
            }],
            timeout_seconds: 9,
        },
    ];

    let mut registry = ToolRegistry::new();
    let report = register_mcp_capabilities(
        &mut registry,
        client,
        capabilities,
        &["search-docs".to_owned(), "missing".to_owned()],
    );
    if report.registered_count != 3 || report.unmatched_enabled_tools != ["missing"] {
        return Err(format!("unexpected MCP registration report: {report:?}").into());
    }
    if registry.has("mcp_srv_one_skip-me") {
        return Err("disabled MCP tool was registered".into());
    }

    let tool_result = registry
        .execute("mcp_srv_one_search-docs", json!({ "query": "rust" }))
        .into_text();
    let resource_result = registry
        .execute("mcp_srv_one_resource_README_md", json!({}))
        .into_text();
    let prompt_result = registry
        .execute("mcp_srv_one_prompt_plan", json!({ "topic": "migration" }))
        .into_text();
    if tool_result != "tool output"
        || resource_result != "resource text\n[Binary resource: 4 bytes]"
        || prompt_result != "prompt text"
    {
        return Err(format!(
            "unexpected MCP execution results: {tool_result} | {resource_result} | {prompt_result}"
        )
        .into());
    }

    let operations = operations.lock().map_err(|error| error.to_string())?;
    if !operations.iter().any(|operation| matches!(
        operation,
        McpOperation::CallTool { name, timeout_seconds, .. } if name == "search-docs" && *timeout_seconds == 30
    )) || !operations.iter().any(|operation| matches!(
        operation,
        McpOperation::ReadResource { uri, timeout_seconds } if uri == "file://README.md" && *timeout_seconds == 7
    )) || !operations.iter().any(|operation| matches!(
        operation,
        McpOperation::GetPrompt { name, timeout_seconds, .. } if name == "plan" && *timeout_seconds == 9
    )) {
        return Err(format!("MCP operations were not routed correctly: {operations:?}").into());
    }
    Ok(())
}

#[test]
fn mcp_wrappers_format_error_outcomes() -> Result<(), Box<dyn Error>> {
    let timeout_client = Arc::new(|operation: McpOperation| match operation {
        McpOperation::CallTool { .. } => McpCallOutcome::Error(McpErrorKind::Timeout),
        McpOperation::ReadResource { .. } => McpCallOutcome::Error(McpErrorKind::Cancelled),
        McpOperation::GetPrompt { .. } => McpCallOutcome::Error(McpErrorKind::Protocol {
            code: -32602,
            message: "bad args".to_owned(),
        }),
    });
    let mut registry = ToolRegistry::new();
    register_mcp_capabilities(
        &mut registry,
        timeout_client,
        vec![
            mcp_tool_capability("srv", "tool", None),
            McpCapability {
                kind: McpCapabilityKind::Resource,
                server_name: "srv".to_owned(),
                name: "res".to_owned(),
                description: None,
                input_schema: None,
                uri: Some("mem://res".to_owned()),
                arguments: Vec::new(),
                timeout_seconds: 30,
            },
            McpCapability {
                kind: McpCapabilityKind::Prompt,
                server_name: "srv".to_owned(),
                name: "prompt".to_owned(),
                description: None,
                input_schema: None,
                uri: None,
                arguments: Vec::new(),
                timeout_seconds: 30,
            },
        ],
        &["*".to_owned()],
    );
    if registry.execute("mcp_srv_tool", json!({})).into_text()
        != "(MCP tool call timed out after 30s)"
    {
        return Err("MCP tool timeout formatting drifted".into());
    }
    if registry
        .execute("mcp_srv_resource_res", json!({}))
        .into_text()
        != "(MCP resource read was cancelled)"
    {
        return Err("MCP resource cancellation formatting drifted".into());
    }
    if registry
        .execute("mcp_srv_prompt_prompt", json!({}))
        .into_text()
        != "(MCP prompt call failed: bad args [code -32602])"
    {
        return Err("MCP prompt protocol error formatting drifted".into());
    }
    Ok(())
}

#[test]
fn mcp_empty_enabled_tools_registers_no_tools() -> Result<(), Box<dyn Error>> {
    let client = Arc::new(|_operation: McpOperation| McpCallOutcome::Success(Vec::new()));
    let mut registry = ToolRegistry::new();
    let report = register_mcp_capabilities(
        &mut registry,
        client,
        vec![
            mcp_tool_capability("srv", "tool", None),
            McpCapability {
                kind: McpCapabilityKind::Resource,
                server_name: "srv".to_owned(),
                name: "res".to_owned(),
                description: None,
                input_schema: None,
                uri: Some("mem://res".to_owned()),
                arguments: Vec::new(),
                timeout_seconds: 30,
            },
            McpCapability {
                kind: McpCapabilityKind::Prompt,
                server_name: "srv".to_owned(),
                name: "prompt".to_owned(),
                description: None,
                input_schema: None,
                uri: None,
                arguments: Vec::new(),
                timeout_seconds: 30,
            },
        ],
        &[],
    );

    if report.registered_count != 2 || !report.unmatched_enabled_tools.is_empty() {
        return Err(format!("unexpected empty enabledTools report: {report:?}").into());
    }
    if registry.has("mcp_srv_tool") {
        return Err("empty enabledTools should not register MCP tools".into());
    }
    let resource = registry
        .get("mcp_srv_resource_res")
        .ok_or("resource was not registered")?;
    let prompt = registry
        .get("mcp_srv_prompt_prompt")
        .ok_or("prompt was not registered")?;
    if !resource.read_only() || !prompt.read_only() {
        return Err("MCP resource and prompt wrappers must be read-only".into());
    }
    Ok(())
}

#[test]
fn mcp_wrappers_retry_transient_errors_once() -> Result<(), Box<dyn Error>> {
    let operations = Arc::new(Mutex::new(Vec::<McpOperation>::new()));
    let capture = operations.clone();
    let client = Arc::new(move |operation: McpOperation| {
        let mut operations = capture.lock().map_err(|error| error.to_string())?;
        let previous_attempts = operations
            .iter()
            .filter(|previous| same_mcp_operation_kind(previous, &operation))
            .count();
        operations.push(operation);
        if previous_attempts == 0 {
            Ok(McpCallOutcome::Error(McpErrorKind::Transient {
                type_name: "ConnectionResetError".to_owned(),
            }))
        } else {
            Ok(McpCallOutcome::Success(vec!["recovered".to_owned()]))
        }
    });
    let client = Arc::new(move |operation: McpOperation| match client(operation) {
        Ok(outcome) => outcome,
        Err(error) => McpCallOutcome::Error(McpErrorKind::Other { type_name: error }),
    });
    let mut registry = ToolRegistry::new();
    register_mcp_capabilities(
        &mut registry,
        client,
        vec![
            mcp_tool_capability("srv", "tool", None),
            McpCapability {
                kind: McpCapabilityKind::Resource,
                server_name: "srv".to_owned(),
                name: "res".to_owned(),
                description: None,
                input_schema: None,
                uri: Some("mem://res".to_owned()),
                arguments: Vec::new(),
                timeout_seconds: 30,
            },
            McpCapability {
                kind: McpCapabilityKind::Prompt,
                server_name: "srv".to_owned(),
                name: "prompt".to_owned(),
                description: None,
                input_schema: None,
                uri: None,
                arguments: Vec::new(),
                timeout_seconds: 30,
            },
        ],
        &["*".to_owned()],
    );

    for name in [
        "mcp_srv_tool",
        "mcp_srv_resource_res",
        "mcp_srv_prompt_prompt",
    ] {
        let result = registry.execute(name, json!({})).into_text();
        if result != "recovered" {
            return Err(format!("{name} did not recover after retry: {result}").into());
        }
    }

    let operations = operations.lock().map_err(|error| error.to_string())?;
    if operations.len() != 6 {
        return Err(format!("MCP wrappers should call once plus one retry: {operations:?}").into());
    }
    Ok(())
}

fn json_map(value: Value) -> Result<JsonMap, Box<dyn Error>> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(format!("expected object, got {other}").into()),
    }
}

fn same_mcp_operation_kind(left: &McpOperation, right: &McpOperation) -> bool {
    matches!(
        (left, right),
        (McpOperation::CallTool { .. }, McpOperation::CallTool { .. })
            | (
                McpOperation::ReadResource { .. },
                McpOperation::ReadResource { .. }
            )
            | (
                McpOperation::GetPrompt { .. },
                McpOperation::GetPrompt { .. }
            )
    )
}

fn mcp_tool_capability(server_name: &str, name: &str, description: Option<&str>) -> McpCapability {
    McpCapability {
        kind: McpCapabilityKind::Tool,
        server_name: server_name.to_owned(),
        name: name.to_owned(),
        description: description.map(str::to_owned),
        input_schema: Some(json!({
            "type": "object",
            "properties": {
                "query": {"type": ["string", "null"]}
            }
        })),
        uri: None,
        arguments: Vec::new(),
        timeout_seconds: 30,
    }
}

fn tag_for_line(hashlines: &str, line_number: usize) -> Result<String, Box<dyn Error>> {
    let prefix = format!("L{line_number}#");
    let line = hashlines
        .lines()
        .find(|line| line.starts_with(&prefix))
        .ok_or_else(|| format!("missing hashline for line {line_number}: {hashlines}"))?;
    let tag = line
        .split_once('|')
        .map(|(tag, _)| tag.trim().to_owned())
        .ok_or_else(|| format!("invalid hashline: {line}"))?;
    Ok(tag)
}

fn read_json(path: impl AsRef<std::path::Path>) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn write_notebook_fixture(
    path: impl AsRef<std::path::Path>,
    notebook: Value,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_string_pretty(&notebook)?)?;
    Ok(())
}
