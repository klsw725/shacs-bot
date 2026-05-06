use crate::tools::SchemaFragment;
use crate::tools::{JsonMap, StringSchema, Tool, ToolParameters, ToolResult, ValidationError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

static NEXT_SPAWN_TOOL_ID: AtomicUsize = AtomicUsize::new(1);

thread_local! {
    static SPAWN_CONTEXTS: RefCell<HashMap<usize, SpawnContext>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub task: String,
    pub label: Option<String>,
    pub origin_channel: String,
    pub origin_chat_id: String,
    pub session_key: String,
}

pub trait SubagentSpawner: Send + Sync {
    fn spawn(&self, request: SpawnRequest) -> Result<String, String>;
}

impl<F> SubagentSpawner for F
where
    F: Fn(SpawnRequest) -> Result<String, String> + Send + Sync,
{
    fn spawn(&self, request: SpawnRequest) -> Result<String, String> {
        self(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnContext {
    origin_channel: String,
    origin_chat_id: String,
    session_key: String,
}

impl SpawnContext {
    fn new(channel: String, chat_id: String, session_key: Option<String>) -> Self {
        let session_key = session_key.unwrap_or_else(|| format!("{channel}:{chat_id}"));
        Self {
            origin_channel: channel,
            origin_chat_id: chat_id,
            session_key,
        }
    }
}

#[derive(Clone)]
pub struct SpawnTool {
    id: usize,
    spawner: Arc<dyn SubagentSpawner>,
    initial_context: SpawnContext,
}

impl SpawnTool {
    pub fn new(spawner: Arc<dyn SubagentSpawner>) -> Self {
        Self::with_defaults(spawner, "cli", "direct", Some("cli:direct".to_owned()))
    }

    pub fn with_defaults(
        spawner: Arc<dyn SubagentSpawner>,
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        session_key: Option<String>,
    ) -> Self {
        Self {
            id: NEXT_SPAWN_TOOL_ID.fetch_add(1, Ordering::Relaxed),
            spawner,
            initial_context: SpawnContext::new(channel.into(), chat_id.into(), session_key),
        }
    }

    pub fn set_context(
        &self,
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        effective_key: Option<String>,
    ) {
        self.replace_context(SpawnContext::new(
            channel.into(),
            chat_id.into(),
            effective_key,
        ));
    }

    pub fn clear_context(&self) {
        SPAWN_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().remove(&self.id);
        });
    }

    fn context(&self) -> SpawnContext {
        SPAWN_CONTEXTS.with(|contexts| {
            contexts
                .borrow()
                .get(&self.id)
                .cloned()
                .unwrap_or_else(|| self.initial_context.clone())
        })
    }

    fn replace_context(&self, context: SpawnContext) {
        SPAWN_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().insert(self.id, context);
        });
    }
}

impl Tool for SpawnTool {
    fn name(&self) -> &str {
        "spawn"
    }

    fn description(&self) -> &str {
        "Spawn a subagent to handle a task in the background. Use this for complex or time-consuming tasks that can run independently. The subagent will complete the task and report back when done. For deliverables or existing projects, inspect the workspace first and use a dedicated subdirectory when helpful."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "task",
                StringSchema::new("The task for the subagent to complete"),
            )
            .property(
                "label",
                StringSchema::new("Optional short label for the task (for display)"),
            )
            .required(["task"])
            .to_json_schema()
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        crate::tools::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        )
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let task = params
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let label = params
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| !label.is_empty())
            .map(str::to_owned);
        let context = self.context();
        match self.spawner.spawn(SpawnRequest {
            task,
            label,
            origin_channel: context.origin_channel,
            origin_chat_id: context.origin_chat_id,
            session_key: context.session_key,
        }) {
            Ok(message) => message.into(),
            Err(error) => format!("Error spawning subagent: {error}").into(),
        }
    }
}
