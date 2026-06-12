use chrono::Utc;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shacs_eval::evaluator::{
    build_evaluator_messages, evaluate_notification_tool_schema, parse_notification_decision,
};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderRequest,
};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const HEARTBEAT_FILE_NAME: &str = "HEARTBEAT.md";
pub const HEARTBEAT_TOOL_NAME: &str = "heartbeat";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatAction {
    Skip,
    Run,
}

impl HeartbeatAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Run => "run",
        }
    }

    pub fn from_tool_arg(value: Option<&str>) -> Self {
        match value {
            Some("run") => Self::Run,
            _ => Self::Skip,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatDecision {
    pub action: HeartbeatAction,
    pub tasks: String,
}

impl HeartbeatDecision {
    pub fn skip() -> Self {
        Self {
            action: HeartbeatAction::Skip,
            tasks: String::new(),
        }
    }

    pub fn run(tasks: impl Into<String>) -> Self {
        Self {
            action: HeartbeatAction::Run,
            tasks: tasks.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatStartResult {
    Started,
    Disabled,
    AlreadyRunning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatTickOutcome {
    MissingHeartbeatFile,
    Skipped,
    NoExecutor,
    NoExecutionResponse,
    SuppressedNonDeliverable,
    SilencedByEvaluator,
    Notified { response: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HeartbeatError {
    Provider(ProviderError),
    Execute(String),
    Evaluate(String),
    Notify(String),
}

impl fmt::Display for HeartbeatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider(error) => write!(formatter, "heartbeat provider error: {error}"),
            Self::Execute(error) => write!(formatter, "heartbeat execution error: {error}"),
            Self::Evaluate(error) => write!(formatter, "heartbeat evaluation error: {error}"),
            Self::Notify(error) => write!(formatter, "heartbeat notification error: {error}"),
        }
    }
}

impl std::error::Error for HeartbeatError {}

impl From<ProviderError> for HeartbeatError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

pub trait HeartbeatTaskExecutor: Send + Sync {
    fn execute(&self, tasks: &str) -> Result<String, HeartbeatError>;
}

impl<F> HeartbeatTaskExecutor for F
where
    F: Fn(&str) -> Result<String, HeartbeatError> + Send + Sync,
{
    fn execute(&self, tasks: &str) -> Result<String, HeartbeatError> {
        self(tasks)
    }
}

pub trait HeartbeatResponseEvaluator: Send + Sync {
    fn should_notify(&self, response: &str, tasks: &str) -> Result<bool, HeartbeatError>;
}

impl<F> HeartbeatResponseEvaluator for F
where
    F: Fn(&str, &str) -> Result<bool, HeartbeatError> + Send + Sync,
{
    fn should_notify(&self, response: &str, tasks: &str) -> Result<bool, HeartbeatError> {
        self(response, tasks)
    }
}

pub trait HeartbeatNotifier: Send + Sync {
    fn notify(&self, response: &str) -> Result<(), HeartbeatError>;
}

impl<F> HeartbeatNotifier for F
where
    F: Fn(&str) -> Result<(), HeartbeatError> + Send + Sync,
{
    fn notify(&self, response: &str) -> Result<(), HeartbeatError> {
        self(response)
    }
}

#[derive(Debug, Clone)]
pub struct HeartbeatService {
    workspace: PathBuf,
    model: String,
    interval_s: u64,
    enabled: bool,
    timezone: Option<String>,
    running: bool,
}

#[derive(Clone)]
pub struct ProviderNotificationEvaluator {
    provider: Arc<dyn ProviderClient>,
    model: String,
}

impl ProviderNotificationEvaluator {
    pub fn new(provider: Arc<dyn ProviderClient>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

impl HeartbeatResponseEvaluator for ProviderNotificationEvaluator {
    fn should_notify(&self, response: &str, tasks: &str) -> Result<bool, HeartbeatError> {
        let settings = GenerationSettings {
            temperature: 0.0,
            max_tokens: 256,
            ..GenerationSettings::default()
        };
        let request = ProviderRequest {
            messages: build_evaluator_messages(tasks, response),
            tools: vec![evaluate_notification_tool_schema()],
            model: self.model.clone(),
            settings,
            tool_choice: None,
        };
        let response = self
            .provider
            .chat(request)
            .map_err(HeartbeatError::Provider)?;
        let response = serde_json::to_value(response).map_err(|error| {
            HeartbeatError::Evaluate(format!(
                "notification decision could not serialize: {error}"
            ))
        })?;
        Ok(parse_notification_decision(&response))
    }
}

pub struct HeartbeatWorker {
    stop: Arc<AtomicBool>,
    wake: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl HeartbeatWorker {
    pub fn start(
        service: HeartbeatService,
        provider: Arc<dyn ProviderClient>,
        settings: GenerationSettings,
        executor: Arc<dyn HeartbeatTaskExecutor>,
        evaluator: Arc<dyn HeartbeatResponseEvaluator>,
        notifier: Arc<dyn HeartbeatNotifier>,
    ) -> Result<Option<Self>, HeartbeatError> {
        if !service.enabled() {
            return Ok(None);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_stop = Arc::clone(&stop);
        let thread_wake = Arc::clone(&wake);
        let interval = Duration::from_secs(service.interval_s().max(1));
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                let (lock, condvar) = &*thread_wake;
                let pending = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let (mut pending, _) = condvar
                    .wait_timeout_while(pending, interval, |pending| {
                        !*pending && !thread_stop.load(Ordering::SeqCst)
                    })
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *pending = false;
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let _ = service.tick(
                    provider.as_ref(),
                    settings.clone(),
                    Some(executor.as_ref()),
                    Some(evaluator.as_ref()),
                    Some(notifier.as_ref()),
                );
            }
        });
        Ok(Some(Self {
            stop,
            wake,
            handle: Some(handle),
        }))
    }

    pub fn wake(&self) {
        let (lock, condvar) = &*self.wake;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        condvar.notify_all();
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake();
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.stop();
        if let Some(handle) = self.handle.take() {
            handle.join()
        } else {
            Ok(())
        }
    }
}

impl Drop for HeartbeatWorker {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl HeartbeatService {
    pub fn new(
        workspace: impl Into<PathBuf>,
        model: impl Into<String>,
        interval_s: u64,
        enabled: bool,
        timezone: Option<String>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            model: model.into(),
            interval_s,
            enabled,
            timezone,
            running: false,
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn interval_s(&self) -> u64 {
        self.interval_s
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn heartbeat_file(&self) -> PathBuf {
        self.workspace.join(HEARTBEAT_FILE_NAME)
    }

    pub fn read_heartbeat_file(&self) -> Option<String> {
        read_heartbeat_file(&self.heartbeat_file())
    }

    pub fn start(&mut self) -> HeartbeatStartResult {
        if !self.enabled {
            return HeartbeatStartResult::Disabled;
        }
        if self.running {
            return HeartbeatStartResult::AlreadyRunning;
        }
        self.running = true;
        HeartbeatStartResult::Started
    }

    pub fn stop(&mut self) -> bool {
        let was_running = self.running;
        self.running = false;
        was_running
    }

    pub fn decide(
        &self,
        provider: &dyn ProviderClient,
        settings: GenerationSettings,
        content: &str,
    ) -> Result<HeartbeatDecision, HeartbeatError> {
        let current_time = current_time_str(self.timezone.as_deref());
        let request = build_decision_request(content, &current_time, &self.model, settings);
        let response = provider.chat(request)?;
        Ok(parse_decision_response(&response))
    }

    pub fn tick(
        &self,
        provider: &dyn ProviderClient,
        settings: GenerationSettings,
        executor: Option<&dyn HeartbeatTaskExecutor>,
        evaluator: Option<&dyn HeartbeatResponseEvaluator>,
        notifier: Option<&dyn HeartbeatNotifier>,
    ) -> Result<HeartbeatTickOutcome, HeartbeatError> {
        let Some(content) = self.read_heartbeat_file() else {
            return Ok(HeartbeatTickOutcome::MissingHeartbeatFile);
        };
        if content.trim().is_empty() {
            return Ok(HeartbeatTickOutcome::MissingHeartbeatFile);
        }

        let decision = self.decide(provider, settings, &content)?;
        if decision.action != HeartbeatAction::Run {
            return Ok(HeartbeatTickOutcome::Skipped);
        }

        let Some(executor) = executor else {
            return Ok(HeartbeatTickOutcome::NoExecutor);
        };
        let response = executor.execute(&decision.tasks)?;
        if response.is_empty() {
            return Ok(HeartbeatTickOutcome::NoExecutionResponse);
        }
        if !is_deliverable(&response) {
            return Ok(HeartbeatTickOutcome::SuppressedNonDeliverable);
        }

        let should_notify = match evaluator {
            Some(evaluator) => evaluator.should_notify(&response, &decision.tasks)?,
            None => false,
        };
        if should_notify {
            if let Some(notifier) = notifier {
                notifier.notify(&response)?;
                return Ok(HeartbeatTickOutcome::Notified { response });
            }
        }
        Ok(HeartbeatTickOutcome::SilencedByEvaluator)
    }

    pub fn trigger_now(
        &self,
        provider: &dyn ProviderClient,
        settings: GenerationSettings,
        executor: Option<&dyn HeartbeatTaskExecutor>,
    ) -> Result<Option<String>, HeartbeatError> {
        let Some(content) = self.read_heartbeat_file() else {
            return Ok(None);
        };
        if content.trim().is_empty() {
            return Ok(None);
        }
        let decision = self.decide(provider, settings, &content)?;
        if decision.action != HeartbeatAction::Run {
            return Ok(None);
        }
        let Some(executor) = executor else {
            return Ok(None);
        };
        executor.execute(&decision.tasks).map(Some)
    }
}

pub fn read_heartbeat_file(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .filter(|content| !content.is_empty())
}

pub fn heartbeat_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": HEARTBEAT_TOOL_NAME,
            "description": "Report heartbeat decision after reviewing tasks.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["skip", "run"],
                        "description": "skip = nothing to do, run = has active tasks"
                    },
                    "tasks": {
                        "type": "string",
                        "description": "Natural-language summary of active tasks (required for run)"
                    }
                },
                "required": ["action"]
            }
        }
    })
}

pub fn build_decision_request(
    content: &str,
    current_time: &str,
    model: &str,
    settings: GenerationSettings,
) -> ProviderRequest {
    ProviderRequest {
        messages: vec![
            json!({
                "role": "system",
                "content": "You are a heartbeat agent. Call the heartbeat tool to report your decision."
            }),
            json!({
                "role": "user",
                "content": format!(
                    "Current Time: {current_time}\n\nReview the following HEARTBEAT.md and decide whether there are active tasks.\n\n{content}"
                )
            }),
        ],
        tools: vec![heartbeat_tool_schema()],
        model: model.to_owned(),
        settings,
        tool_choice: None,
    }
}

pub fn parse_decision_response(response: &LlmResponse) -> HeartbeatDecision {
    if !response.should_execute_tools() {
        return HeartbeatDecision::skip();
    }

    let Some(call) = response.tool_calls.first() else {
        return HeartbeatDecision::skip();
    };
    if call.name != HEARTBEAT_TOOL_NAME {
        return HeartbeatDecision::skip();
    }

    let action = HeartbeatAction::from_tool_arg(
        call.arguments
            .get("action")
            .and_then(serde_json::Value::as_str),
    );
    let tasks = call
        .arguments
        .get("tasks")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();

    match action {
        HeartbeatAction::Run => HeartbeatDecision::run(tasks),
        HeartbeatAction::Skip => HeartbeatDecision::skip(),
    }
}

pub fn is_deliverable(response: &str) -> bool {
    let text = response.to_lowercase();
    if text.contains("couldn't produce a final answer") {
        return false;
    }

    let leaked_patterns = [
        "heartbeat.md",
        "awareness.md",
        "judgment call:",
        "decision logic",
        "valid options are",
        "my instructions",
        "i am supposed to",
        "strict heartbeat interpretation",
    ];
    !leaked_patterns.iter().any(|pattern| text.contains(pattern))
}

pub fn current_time_str(timezone: Option<&str>) -> String {
    if let Some(timezone) = timezone.and_then(|value| value.parse::<Tz>().ok()) {
        return Utc::now()
            .with_timezone(&timezone)
            .format("%Y-%m-%d %H:%M:%S %Z")
            .to_string();
    }
    Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};
    use shacs_providers::{ProviderEvent, ToolCallRequest};
    use std::error::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[derive(Clone)]
    struct MockProvider {
        response: LlmResponse,
        seen_request: Arc<Mutex<Option<ProviderRequest>>>,
    }

    impl MockProvider {
        fn new(response: LlmResponse) -> Self {
            Self {
                response,
                seen_request: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl ProviderClient for MockProvider {
        fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
            *self
                .seen_request
                .lock()
                .expect("mock request lock poisoned") = Some(request);
            Ok(self.response.clone())
        }

        fn chat_stream(
            &self,
            request: ProviderRequest,
            _on_event: &mut dyn FnMut(ProviderEvent),
        ) -> Result<LlmResponse, ProviderError> {
            self.chat(request)
        }
    }

    fn heartbeat_response(action: &str, tasks: &str, finish_reason: &str) -> LlmResponse {
        let mut arguments = Map::new();
        arguments.insert("action".to_owned(), Value::String(action.to_owned()));
        arguments.insert("tasks".to_owned(), Value::String(tasks.to_owned()));
        LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "call-1",
                HEARTBEAT_TOOL_NAME,
                arguments,
            )],
            finish_reason: finish_reason.to_owned(),
            ..LlmResponse::default()
        }
    }

    #[test]
    fn request_uses_virtual_heartbeat_tool() {
        let request = build_decision_request(
            "## Active Tasks\n- check backups",
            "2026-05-05 10:00:00 KST",
            "model-a",
            GenerationSettings::default(),
        );

        assert_eq!(request.model, "model-a");
        assert_eq!(request.tools[0]["function"]["name"], HEARTBEAT_TOOL_NAME);
        assert_eq!(request.messages[0]["role"], "system");
        assert!(request.messages[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("check backups"));
    }

    #[test]
    fn parse_decision_requires_executable_tool_finish_reason() {
        let blocked = heartbeat_response("run", "active task", "length");
        assert_eq!(parse_decision_response(&blocked), HeartbeatDecision::skip());

        let runnable = heartbeat_response("run", "active task", "tool_calls");
        assert_eq!(
            parse_decision_response(&runnable),
            HeartbeatDecision::run("active task")
        );
    }

    #[test]
    fn deliverable_filter_suppresses_fallback_and_leaked_reasoning() {
        assert!(!is_deliverable("I couldn't produce a final answer"));
        assert!(!is_deliverable(
            "I inspected HEARTBEAT.md and made a judgment call:"
        ));
        assert!(is_deliverable("Backups were checked successfully."));
    }

    #[test]
    fn service_start_stop_matches_enabled_state() {
        let mut disabled = HeartbeatService::new("/tmp", "model", 1800, false, None);
        assert_eq!(disabled.start(), HeartbeatStartResult::Disabled);
        assert!(!disabled.is_running());

        let mut enabled = HeartbeatService::new("/tmp", "model", 1800, true, None);
        assert_eq!(enabled.start(), HeartbeatStartResult::Started);
        assert_eq!(enabled.start(), HeartbeatStartResult::AlreadyRunning);
        assert!(enabled.stop());
        assert!(!enabled.is_running());
    }

    #[test]
    fn tick_runs_evaluates_and_notifies() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        fs::write(
            tempdir.path().join(HEARTBEAT_FILE_NAME),
            "## Active Tasks\n- check backups",
        )
        .expect("write heartbeat file");
        let provider = MockProvider::new(heartbeat_response("run", "check backups", "tool_calls"));
        let service = HeartbeatService::new(tempdir.path(), "model", 1800, true, None);
        let notified = Arc::new(Mutex::new(String::new()));
        let notified_for_callback = Arc::clone(&notified);
        let executor = |tasks: &str| Ok(format!("completed: {tasks}"));
        let evaluator = |response: &str, tasks: &str| Ok(response.contains(tasks));
        let notifier = move |response: &str| {
            *notified_for_callback
                .lock()
                .expect("notification lock poisoned") = response.to_owned();
            Ok(())
        };

        let outcome = service
            .tick(
                &provider,
                GenerationSettings::default(),
                Some(&executor),
                Some(&evaluator),
                Some(&notifier),
            )
            .expect("tick succeeds");

        assert_eq!(
            outcome,
            HeartbeatTickOutcome::Notified {
                response: "completed: check backups".to_owned()
            }
        );
        assert_eq!(
            notified
                .lock()
                .expect("notification lock poisoned")
                .as_str(),
            "completed: check backups"
        );
    }

    #[test]
    fn trigger_now_returns_none_without_file_or_run_decision() {
        let tempdir = tempfile::tempdir().expect("create tempdir");
        let skip_provider = MockProvider::new(heartbeat_response("skip", "", "tool_calls"));
        let service = HeartbeatService::new(tempdir.path(), "model", 1800, true, None);
        let executor = |_tasks: &str| Ok("done".to_owned());

        assert_eq!(
            service
                .trigger_now(
                    &skip_provider,
                    GenerationSettings::default(),
                    Some(&executor)
                )
                .expect("missing file is not fatal"),
            None
        );

        fs::write(tempdir.path().join(HEARTBEAT_FILE_NAME), "active")
            .expect("write heartbeat file");
        assert_eq!(
            service
                .trigger_now(
                    &skip_provider,
                    GenerationSettings::default(),
                    Some(&executor)
                )
                .expect("skip decision is not fatal"),
            None
        );
    }

    #[test]
    fn provider_notification_evaluator_uses_tool_decision() -> Result<(), Box<dyn Error>> {
        let mut arguments = Map::new();
        arguments.insert("should_notify".to_owned(), Value::Bool(false));
        let provider = Arc::new(MockProvider::new(LlmResponse {
            tool_calls: vec![ToolCallRequest::new(
                "call-1",
                "evaluate_notification",
                arguments,
            )],
            finish_reason: "tool_calls".to_owned(),
            ..LlmResponse::default()
        }));
        let evaluator = ProviderNotificationEvaluator::new(provider.clone(), "judge-model");

        assert!(!evaluator.should_notify("routine success", "check backups")?);
        let request = provider
            .seen_request
            .lock()
            .map_err(|_| "request lock poisoned")?
            .clone()
            .ok_or("missing evaluator request")?;
        assert_eq!(request.model, "judge-model");
        assert_eq!(request.settings.temperature, 0.0);
        assert_eq!(request.settings.max_tokens, 256);
        assert_eq!(
            request.tools[0]["function"]["name"],
            "evaluate_notification"
        );
        Ok(())
    }

    #[test]
    fn worker_wakes_runs_and_stops_cleanly() -> Result<(), Box<dyn Error>> {
        let tempdir = tempfile::tempdir()?;
        fs::write(
            tempdir.path().join(HEARTBEAT_FILE_NAME),
            "## Active Tasks\n- check backups",
        )?;
        let provider = Arc::new(MockProvider::new(heartbeat_response(
            "run",
            "check backups",
            "tool_calls",
        )));
        let service = HeartbeatService::new(tempdir.path(), "model", 3_600, true, None);
        let runs = Arc::new(AtomicUsize::new(0));
        let notifications = Arc::new(AtomicUsize::new(0));

        let runs_for_executor = Arc::clone(&runs);
        let executor: Arc<dyn HeartbeatTaskExecutor> = Arc::new(move |tasks: &str| {
            runs_for_executor.fetch_add(1, Ordering::SeqCst);
            Ok(format!("completed: {tasks}"))
        });
        let evaluator: Arc<dyn HeartbeatResponseEvaluator> =
            Arc::new(|_response: &str, _tasks: &str| Ok::<bool, HeartbeatError>(true));
        let notifications_for_notifier = Arc::clone(&notifications);
        let notifier: Arc<dyn HeartbeatNotifier> = Arc::new(move |_response: &str| {
            notifications_for_notifier.fetch_add(1, Ordering::SeqCst);
            Ok::<(), HeartbeatError>(())
        });

        let worker = HeartbeatWorker::start(
            service,
            provider,
            GenerationSettings::default(),
            executor,
            evaluator,
            notifier,
        )?
        .ok_or("enabled heartbeat should start worker")?;
        worker.wake();

        let deadline = Instant::now() + Duration::from_secs(2);
        while runs.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        worker.join().map_err(|_| "heartbeat worker panicked")?;
        Ok(())
    }
}
