use crate::tools::{BooleanSchema, IntegerSchema, JsonMap, StringSchema, Tool, ToolParameters};
use crate::tools::{SchemaFragment, ToolResult, ValidationError};
use chrono::{DateTime, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use serde_json::Value;
use shacs_cron::{
    AddCronJob, CronJob, CronJobState, CronPayloadKind, CronRunStatus, CronSchedule,
    CronScheduleKind, CronService, RemoveJobResult,
};
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static CRON_CONTEXT: RefCell<CronContext> = RefCell::new(CronContext::default());
}

#[derive(Debug, Clone)]
struct CronContext {
    channel: String,
    chat_id: String,
    metadata: Value,
    session_key: String,
    in_cron_context: bool,
}

impl Default for CronContext {
    fn default() -> Self {
        Self {
            channel: String::new(),
            chat_id: String::new(),
            metadata: Value::Object(Default::default()),
            session_key: String::new(),
            in_cron_context: false,
        }
    }
}

#[derive(Clone)]
pub struct CronTool {
    cron: Arc<dyn CronService>,
    default_timezone: String,
}

impl CronTool {
    pub fn new(cron: Arc<dyn CronService>) -> Self {
        Self::with_timezone(cron, "UTC")
    }

    pub fn with_timezone(cron: Arc<dyn CronService>, default_timezone: impl Into<String>) -> Self {
        Self {
            cron,
            default_timezone: default_timezone.into(),
        }
    }

    pub fn set_context(
        &self,
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        metadata: Option<Value>,
        session_key: Option<String>,
    ) {
        let channel = channel.into();
        let chat_id = chat_id.into();
        CRON_CONTEXT.with(|context| {
            let mut context = context.borrow_mut();
            context.channel = channel.clone();
            context.chat_id = chat_id.clone();
            context.metadata = metadata.unwrap_or_else(|| Value::Object(Default::default()));
            context.session_key = session_key.unwrap_or_else(|| format!("{channel}:{chat_id}"));
        });
    }

    pub fn set_cron_context(&self, active: bool) -> bool {
        CRON_CONTEXT.with(|context| {
            let mut context = context.borrow_mut();
            let previous = context.in_cron_context;
            context.in_cron_context = active;
            previous
        })
    }

    pub fn reset_cron_context(&self, previous: bool) {
        CRON_CONTEXT.with(|context| context.borrow_mut().in_cron_context = previous);
    }
}

impl Tool for CronTool {
    fn name(&self) -> &str {
        "cron"
    }

    fn description(&self) -> &str {
        "Schedule reminders and recurring tasks. Actions: add, list, remove. If tz is omitted, cron expressions and naive ISO times use the tool's default timezone."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .raw_property(
                "action",
                serde_json::json!({
                    "type": "string",
                    "enum": ["add", "list", "remove"],
                    "description": "Action to perform"
                }),
            )
            .property("name", StringSchema::new("Optional short human-readable label for the job"))
            .property("message", StringSchema::new("Required when action='add'. Instruction for the agent to execute when the job triggers"))
            .property("every_seconds", IntegerSchema::new("Interval in seconds for recurring tasks").minimum(0))
            .property("cron_expr", StringSchema::new("Cron expression like '0 9 * * *'"))
            .property("tz", StringSchema::new("Optional IANA timezone for cron expressions"))
            .property("at", StringSchema::new("ISO datetime for one-time execution"))
            .property("deliver", BooleanSchema::new("Whether to deliver the execution result to the user channel").default(true))
            .property("job_id", StringSchema::new("Required when action='remove'. Job ID to remove"))
            .required(["action"])
            .to_json_schema()
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        let mut errors = crate::tools::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        );
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if action == "add"
            && params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            errors.push(ValidationError::new(
                "message",
                "is required when action='add'",
            ));
        }
        if action == "remove"
            && params
                .get("job_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            errors.push(ValidationError::new(
                "job_id",
                "is required when action='remove'",
            ));
        }
        errors
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match action {
            "add" => self.add_job(params).into(),
            "list" => self.list_jobs().into(),
            "remove" => self
                .remove_job(params.get("job_id").and_then(Value::as_str))
                .into(),
            other => format!("Unknown action: {other}").into(),
        }
    }
}

impl CronTool {
    fn add_job(&self, params: JsonMap) -> String {
        if CRON_CONTEXT.with(|context| context.borrow().in_cron_context) {
            return "Error: cannot schedule new jobs from within a cron job execution".to_owned();
        }
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if message.is_empty() {
            return "Error: cron action='add' requires a non-empty 'message' parameter describing what to do when the job triggers (e.g. the reminder text). Retry including message=\"...\".".to_owned();
        }
        let context = CRON_CONTEXT.with(|context| context.borrow().clone());
        if context.channel.is_empty() || context.chat_id.is_empty() {
            return "Error: no session context (channel/chat_id)".to_owned();
        }
        let cron_expr = params
            .get("cron_expr")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let tz = params
            .get("tz")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let at = params
            .get("at")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        if tz.is_some() && cron_expr.is_none() {
            return "Error: tz can only be used with cron_expr".to_owned();
        }

        let (schedule, delete_after_run) = if let Some(every_seconds) = params
            .get("every_seconds")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
        {
            (CronSchedule::every(every_seconds * 1000), false)
        } else if let Some(expr) = cron_expr {
            let effective_tz = tz.unwrap_or(&self.default_timezone);
            if let Err(error) = validate_timezone(effective_tz) {
                return error;
            }
            (CronSchedule::cron(expr, effective_tz), false)
        } else if let Some(at) = at {
            match parse_at_ms(at, &self.default_timezone) {
                Ok(at_ms) => (CronSchedule::at(at_ms), true),
                Err(error) => return error,
            }
        } else {
            return "Error: either every_seconds, cron_expr, or at is required".to_owned();
        };

        let name = params
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| message.chars().take(30).collect());
        let deliver = params
            .get("deliver")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let job = self.cron.add_job(AddCronJob {
            name,
            schedule,
            message: message.to_owned(),
            deliver,
            channel: Some(context.channel),
            to: Some(context.chat_id),
            delete_after_run,
            channel_meta: context.metadata,
            session_key: if context.session_key.is_empty() {
                None
            } else {
                Some(context.session_key)
            },
        });
        format!("Created job '{}' (id: {})", job.name, job.id)
    }

    fn list_jobs(&self) -> String {
        let jobs = self.cron.list_jobs();
        if jobs.is_empty() {
            return "No scheduled jobs.".to_owned();
        }
        let mut lines = Vec::new();
        for job in jobs {
            let timing = self.format_timing(&job.schedule);
            let mut parts = vec![format!("- {} (id: {}, {timing})", job.name, job.id)];
            if job.payload.kind == CronPayloadKind::SystemEvent {
                parts.push(format!("  Purpose: {}", system_job_purpose(&job)));
                parts
                    .push("  Protected: visible for inspection, but cannot be removed.".to_owned());
            }
            parts.extend(self.format_state(&job.state, &job.schedule));
            lines.push(parts.join("\n"));
        }
        format!("Scheduled jobs:\n{}", lines.join("\n"))
    }

    fn remove_job(&self, job_id: Option<&str>) -> String {
        let Some(job_id) = job_id.filter(|value| !value.is_empty()) else {
            return "Error: job_id is required for remove".to_owned();
        };
        match self.cron.remove_job(job_id) {
            RemoveJobResult::Removed => format!("Removed job {job_id}"),
            RemoveJobResult::Protected => {
                if self
                    .cron
                    .get_job(job_id)
                    .is_some_and(|job| job.name == "dream")
                {
                    "Cannot remove job `dream`.\nThis is a system-managed Dream memory consolidation job for long-term memory.\nIt remains visible so you can inspect it, but it cannot be removed.".to_owned()
                } else {
                    format!("Cannot remove job `{job_id}`.\nThis is a protected system-managed cron job.")
                }
            }
            RemoveJobResult::NotFound => format!("Job {job_id} not found"),
        }
    }

    fn format_timing(&self, schedule: &CronSchedule) -> String {
        match schedule.kind {
            CronScheduleKind::Cron => {
                let tz = schedule
                    .tz
                    .as_deref()
                    .map_or(String::new(), |tz| format!(" ({tz})"));
                format!("cron: {}{tz}", schedule.expr.as_deref().unwrap_or_default())
            }
            CronScheduleKind::Every => format_every(schedule.every_ms.unwrap_or_default()),
            CronScheduleKind::At => format!(
                "at {}",
                format_timestamp(
                    schedule.at_ms.unwrap_or_default(),
                    schedule.tz.as_deref().unwrap_or(&self.default_timezone),
                )
            ),
        }
    }

    fn format_state(&self, state: &CronJobState, schedule: &CronSchedule) -> Vec<String> {
        let display_tz = schedule.tz.as_deref().unwrap_or(&self.default_timezone);
        let mut lines = Vec::new();
        if let Some(last_run_at_ms) = state.last_run_at_ms {
            let status = state
                .last_status
                .as_ref()
                .map(CronRunStatus::as_str)
                .unwrap_or("unknown");
            let mut info = format!(
                "  Last run: {} — {status}",
                format_timestamp(last_run_at_ms, display_tz)
            );
            if let Some(error) = &state.last_error {
                info.push_str(&format!(" ({error})"));
            }
            lines.push(info);
        }
        if let Some(next_run_at_ms) = state.next_run_at_ms {
            lines.push(format!(
                "  Next run: {}",
                format_timestamp(next_run_at_ms, display_tz)
            ));
        }
        lines
    }
}

fn validate_timezone(tz: &str) -> Result<(), String> {
    tz.parse::<Tz>()
        .map(|_| ())
        .map_err(|_| format!("Error: unknown timezone '{tz}'"))
}

fn parse_at_ms(value: &str, default_timezone: &str) -> Result<i64, String> {
    if let Ok(datetime) = DateTime::parse_from_rfc3339(value) {
        return Ok(datetime.timestamp_millis());
    }
    let naive = parse_naive_iso_datetime(value)?;
    let tz = default_timezone
        .parse::<Tz>()
        .map_err(|_| format!("Error: unknown timezone '{default_timezone}'"))?;
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(datetime) => Ok(datetime.timestamp_millis()),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest.timestamp_millis()),
        LocalResult::None => Err(format!(
            "Error: invalid local datetime '{value}' in timezone '{default_timezone}'"
        )),
    }
}

fn format_timestamp(ms: i64, tz_name: &str) -> String {
    let tz = tz_name.parse::<Tz>().unwrap_or(chrono_tz::UTC);
    let formatted = match tz.timestamp_millis_opt(ms) {
        LocalResult::Single(datetime) => datetime.to_rfc3339(),
        LocalResult::Ambiguous(datetime, _) => datetime.to_rfc3339(),
        LocalResult::None => "invalid-timestamp".to_owned(),
    };
    format!("{formatted} ({tz_name})")
}

fn parse_naive_iso_datetime(value: &str) -> Result<NaiveDateTime, String> {
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| {
            format!(
                "Error: invalid ISO datetime format '{value}'. Expected format: YYYY-MM-DDTHH:MM:SS"
            )
        })
}

fn format_every(ms: i64) -> String {
    if ms <= 0 {
        return "every".to_owned();
    }
    if ms % 3_600_000 == 0 {
        format!("every {}h", ms / 3_600_000)
    } else if ms % 60_000 == 0 {
        format!("every {}m", ms / 60_000)
    } else if ms % 1000 == 0 {
        format!("every {}s", ms / 1000)
    } else {
        format!("every {ms}ms")
    }
}

fn system_job_purpose(job: &CronJob) -> &'static str {
    if job.name == "dream" {
        "Dream memory consolidation for long-term memory."
    } else {
        "System-managed internal job."
    }
}
