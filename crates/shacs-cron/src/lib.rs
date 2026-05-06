use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration as StdDuration;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronScheduleKind {
    At,
    Every,
    Cron,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronSchedule {
    pub kind: CronScheduleKind,
    #[serde(alias = "at_ms")]
    pub at_ms: Option<i64>,
    #[serde(alias = "every_ms")]
    pub every_ms: Option<i64>,
    pub expr: Option<String>,
    pub tz: Option<String>,
}

impl CronSchedule {
    pub fn every(every_ms: i64) -> Self {
        Self {
            kind: CronScheduleKind::Every,
            at_ms: None,
            every_ms: Some(every_ms),
            expr: None,
            tz: None,
        }
    }

    pub fn cron(expr: impl Into<String>, tz: impl Into<String>) -> Self {
        Self {
            kind: CronScheduleKind::Cron,
            at_ms: None,
            every_ms: None,
            expr: Some(expr.into()),
            tz: Some(tz.into()),
        }
    }

    pub fn at(at_ms: i64) -> Self {
        Self {
            kind: CronScheduleKind::At,
            at_ms: Some(at_ms),
            every_ms: None,
            expr: None,
            tz: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronPayloadKind {
    SystemEvent,
    AgentTurn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronPayload {
    pub kind: CronPayloadKind,
    pub message: String,
    pub deliver: bool,
    pub channel: Option<String>,
    pub to: Option<String>,
    #[serde(alias = "channel_meta")]
    pub channel_meta: Value,
    #[serde(alias = "session_key")]
    pub session_key: Option<String>,
}

impl Default for CronPayload {
    fn default() -> Self {
        Self {
            kind: CronPayloadKind::AgentTurn,
            message: String::new(),
            deliver: false,
            channel: None,
            to: None,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronRunStatus {
    Ok,
    Error,
    Skipped,
}

impl CronRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronRunRecord {
    #[serde(alias = "run_at_ms")]
    pub run_at_ms: i64,
    pub status: CronRunStatus,
    #[serde(alias = "duration_ms")]
    pub duration_ms: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobState {
    #[serde(alias = "next_run_at_ms")]
    pub next_run_at_ms: Option<i64>,
    #[serde(alias = "last_run_at_ms")]
    pub last_run_at_ms: Option<i64>,
    #[serde(alias = "last_status")]
    pub last_status: Option<CronRunStatus>,
    #[serde(alias = "last_error")]
    pub last_error: Option<String>,
    #[serde(alias = "run_history")]
    pub run_history: Vec<CronRunRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: CronSchedule,
    pub payload: CronPayload,
    pub state: CronJobState,
    #[serde(alias = "created_at_ms")]
    pub created_at_ms: i64,
    #[serde(alias = "updated_at_ms")]
    pub updated_at_ms: i64,
    #[serde(alias = "delete_after_run")]
    pub delete_after_run: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AddCronJob {
    pub name: String,
    pub schedule: CronSchedule,
    pub message: String,
    pub deliver: bool,
    pub channel: Option<String>,
    pub to: Option<String>,
    pub delete_after_run: bool,
    pub channel_meta: Value,
    pub session_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronStore {
    pub version: u32,
    pub jobs: Vec<CronJob>,
}

impl Default for CronStore {
    fn default() -> Self {
        Self {
            version: 1,
            jobs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveJobResult {
    Removed,
    Protected,
    NotFound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CronJobUpdate {
    pub name: Option<String>,
    pub schedule: Option<CronSchedule>,
    pub message: Option<String>,
    pub deliver: Option<bool>,
    pub channel: Option<Option<String>>,
    pub to: Option<Option<String>>,
    pub delete_after_run: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateJobResult {
    Updated(Box<CronJob>),
    Protected,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronStatusSnapshot {
    pub running: bool,
    pub jobs: usize,
    pub next_wake_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronRunOutcome {
    pub job_id: String,
    pub status: CronRunStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronScheduleError {
    InvalidInterval,
    InvalidAt,
    InvalidCronExpression(String),
    UnknownTimezone(String),
}

impl std::fmt::Display for CronScheduleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInterval => write!(formatter, "every schedule requires every_ms > 0"),
            Self::InvalidAt => write!(formatter, "at schedule requires a future at_ms"),
            Self::InvalidCronExpression(error) => {
                write!(formatter, "invalid cron expression: {error}")
            }
            Self::UnknownTimezone(tz) => write!(formatter, "unknown timezone `{tz}`"),
        }
    }
}

impl std::error::Error for CronScheduleError {}

pub trait CronJobExecutor: Send + Sync {
    fn execute(&self, job: &CronJob) -> Result<Option<String>, String>;
}

impl<F> CronJobExecutor for F
where
    F: Fn(&CronJob) -> Result<Option<String>, String> + Send + Sync,
{
    fn execute(&self, job: &CronJob) -> Result<Option<String>, String> {
        self(job)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CronActionKind {
    Add,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CronActionRecord {
    action: String,
    params: Value,
}

pub trait CronService: Send + Sync {
    fn add_job(&self, request: AddCronJob) -> CronJob;
    fn list_jobs(&self) -> Vec<CronJob>;
    fn remove_job(&self, job_id: &str) -> RemoveJobResult;
    fn get_job(&self, job_id: &str) -> Option<CronJob>;

    fn register_system_job(&self, job: CronJob) -> CronJob {
        job
    }

    fn enable_job(&self, _job_id: &str, _enabled: bool) -> Option<CronJob> {
        None
    }

    fn update_job(&self, _job_id: &str, _update: CronJobUpdate) -> UpdateJobResult {
        UpdateJobResult::NotFound
    }

    fn status(&self) -> CronStatusSnapshot {
        CronStatusSnapshot {
            running: false,
            jobs: self.list_jobs().len(),
            next_wake_at_ms: self
                .list_jobs()
                .into_iter()
                .filter_map(|job| job.state.next_run_at_ms)
                .min(),
        }
    }
}

#[derive(Debug, Default)]
pub struct InMemoryCronService {
    jobs: Mutex<Vec<CronJob>>,
    next_id: Mutex<u64>,
}

impl InMemoryCronService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_jobs(jobs: Vec<CronJob>) -> Self {
        Self {
            jobs: Mutex::new(jobs),
            next_id: Mutex::new(1),
        }
    }

    fn next_id(&self) -> String {
        let mut next_id = recover_lock(&self.next_id);
        let id = format!("job{:04}", *next_id);
        *next_id += 1;
        id
    }
}

impl CronService for InMemoryCronService {
    fn add_job(&self, request: AddCronJob) -> CronJob {
        let now_ms = 0;
        let job = CronJob {
            id: self.next_id(),
            name: request.name,
            enabled: true,
            schedule: request.schedule,
            payload: CronPayload {
                kind: CronPayloadKind::AgentTurn,
                message: request.message,
                deliver: request.deliver,
                channel: request.channel,
                to: request.to,
                channel_meta: request.channel_meta,
                session_key: request.session_key,
            },
            state: CronJobState::default(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            delete_after_run: request.delete_after_run,
        };
        recover_lock(&self.jobs).push(job.clone());
        job
    }

    fn list_jobs(&self) -> Vec<CronJob> {
        let mut jobs = recover_lock(&self.jobs).clone();
        jobs.retain(|job| job.enabled);
        jobs.sort_by_key(|job| job.state.next_run_at_ms.unwrap_or(i64::MAX));
        jobs
    }

    fn remove_job(&self, job_id: &str) -> RemoveJobResult {
        let mut jobs = recover_lock(&self.jobs);
        let Some(position) = jobs.iter().position(|job| job.id == job_id) else {
            return RemoveJobResult::NotFound;
        };
        if jobs[position].payload.kind == CronPayloadKind::SystemEvent {
            return RemoveJobResult::Protected;
        }
        jobs.remove(position);
        RemoveJobResult::Removed
    }

    fn get_job(&self, job_id: &str) -> Option<CronJob> {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|job| job.id == job_id)
            .cloned()
    }

    fn register_system_job(&self, mut job: CronJob) -> CronJob {
        let now_ms = now_ms();
        job.state.next_run_at_ms = compute_next_run(&job.schedule, now_ms).ok().flatten();
        job.created_at_ms = now_ms;
        job.updated_at_ms = now_ms;
        let mut jobs = recover_lock(&self.jobs);
        jobs.retain(|existing| existing.id != job.id);
        jobs.push(job.clone());
        job
    }

    fn enable_job(&self, job_id: &str, enabled: bool) -> Option<CronJob> {
        let now_ms = now_ms();
        let mut jobs = recover_lock(&self.jobs);
        let job = jobs.iter_mut().find(|job| job.id == job_id)?;
        job.enabled = enabled;
        job.updated_at_ms = now_ms;
        job.state.next_run_at_ms = if enabled {
            compute_next_run(&job.schedule, now_ms).ok().flatten()
        } else {
            None
        };
        Some(job.clone())
    }

    fn update_job(&self, job_id: &str, update: CronJobUpdate) -> UpdateJobResult {
        let now_ms = now_ms();
        let mut jobs = recover_lock(&self.jobs);
        let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) else {
            return UpdateJobResult::NotFound;
        };
        if job.payload.kind == CronPayloadKind::SystemEvent {
            return UpdateJobResult::Protected;
        }
        apply_job_update(job, update, now_ms);
        UpdateJobResult::Updated(Box::new(job.clone()))
    }
}

#[derive(Debug)]
pub struct PersistentCronService {
    store_path: PathBuf,
    action_path: PathBuf,
    store: Mutex<CronStore>,
    running: Mutex<bool>,
    next_id: Mutex<u64>,
    wake: Arc<(Mutex<u64>, Condvar)>,
}

pub struct CronSupervisorConfig {
    pub max_sleep_ms: u64,
}

impl Default for CronSupervisorConfig {
    fn default() -> Self {
        Self {
            max_sleep_ms: 300_000,
        }
    }
}

pub struct CronSupervisor {
    service: Arc<PersistentCronService>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl PersistentCronService {
    pub fn new(store_path: impl Into<PathBuf>) -> io::Result<Self> {
        let store_path = store_path.into();
        let action_path = store_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("action.jsonl");
        let store = load_store_file(&store_path)?;
        let service = Self {
            store_path,
            action_path,
            store: Mutex::new(store),
            running: Mutex::new(false),
            next_id: Mutex::new(1),
            wake: Arc::new((Mutex::new(0), Condvar::new())),
        };
        let _ = service.refresh_store();
        Ok(service)
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    pub fn action_path(&self) -> &Path {
        &self.action_path
    }

    pub fn start(&self) -> io::Result<()> {
        *recover_lock(&self.running) = true;
        let mut store = self.refresh_store()?;
        recompute_next_runs(&mut store, now_ms());
        self.save_store(&store)?;
        *recover_lock(&self.store) = store;
        Ok(())
    }

    pub fn stop(&self) {
        *recover_lock(&self.running) = false;
        self.wake_worker();
    }

    pub fn is_running(&self) -> bool {
        *recover_lock(&self.running)
    }

    pub fn refresh_store(&self) -> io::Result<CronStore> {
        let mut store = load_store_file(&self.store_path)?;
        let changed = self.merge_actions(&mut store)?;
        if self.is_running() && changed {
            self.save_store(&store)?;
            fs::write(&self.action_path, "")?;
        }
        *recover_lock(&self.store) = store.clone();
        Ok(store)
    }

    pub fn next_wake_at_ms(&self) -> Option<i64> {
        self.refresh_store()
            .ok()
            .and_then(|store| next_wake_at_ms(&store))
    }

    pub fn tick_due(
        &self,
        due_now_ms: i64,
        executor: &dyn CronJobExecutor,
    ) -> io::Result<Vec<CronRunOutcome>> {
        let mut store = self.refresh_store()?;
        let mut outcomes = Vec::new();
        let mut index = 0;
        while index < store.jobs.len() {
            let due = store.jobs[index].enabled
                && store.jobs[index]
                    .state
                    .next_run_at_ms
                    .is_some_and(|next_run_at_ms| due_now_ms >= next_run_at_ms);
            if !due {
                index += 1;
                continue;
            }

            let (outcome, removed) = execute_store_job(&mut store, index, due_now_ms, executor);
            if !removed {
                index += 1;
            }
            outcomes.push(outcome);
        }
        self.save_store(&store)?;
        *recover_lock(&self.store) = store;
        Ok(outcomes)
    }

    pub fn run_job(
        &self,
        job_id: &str,
        force: bool,
        executor: &dyn CronJobExecutor,
    ) -> io::Result<bool> {
        let mut store = self.refresh_store()?;
        let Some(position) = store.jobs.iter().position(|job| job.id == job_id) else {
            return Ok(false);
        };
        if !force && !store.jobs[position].enabled {
            return Ok(false);
        }
        let now = now_ms();
        let _ = execute_store_job(&mut store, position, now, executor);
        self.save_store(&store)?;
        *recover_lock(&self.store) = store;
        Ok(true)
    }

    fn next_id(&self) -> String {
        let now = now_ms() as u64;
        let mut next_id = recover_lock(&self.next_id);
        let id = format!("{:08x}", now.wrapping_add(*next_id) & 0xffff_ffff);
        *next_id += 1;
        id
    }

    fn mutate_store(&self, job: CronJob, action: CronActionKind) -> io::Result<()> {
        let running = self.is_running();
        let mut store = self.refresh_store()?;
        match action {
            CronActionKind::Add | CronActionKind::Update => {
                store.jobs.retain(|existing| existing.id != job.id);
                store.jobs.push(job.clone());
                if running {
                    self.save_store(&store)?;
                } else {
                    self.append_action(action, serde_json::to_value(&job)?)?;
                }
            }
            CronActionKind::Delete => {
                store.jobs.retain(|existing| existing.id != job.id);
                if running {
                    self.save_store(&store)?;
                } else {
                    self.append_action(action, serde_json::json!({ "job_id": job.id }))?;
                }
            }
        }
        *recover_lock(&self.store) = store;
        self.wake_worker();
        Ok(())
    }

    pub fn wake_worker(&self) {
        let (lock, condvar) = &*self.wake;
        let mut generation = recover_lock(lock);
        *generation = generation.wrapping_add(1);
        condvar.notify_all();
    }

    fn append_action(&self, action: CronActionKind, params: Value) -> io::Result<()> {
        if let Some(parent) = self.action_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let action = match action {
            CronActionKind::Add => "add",
            CronActionKind::Update => "update",
            CronActionKind::Delete => "del",
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.action_path)?;
        writeln!(
            file,
            "{}",
            serde_json::to_string(&CronActionRecord {
                action: action.to_owned(),
                params,
            })?
        )
    }

    fn merge_actions(&self, store: &mut CronStore) -> io::Result<bool> {
        let Ok(text) = fs::read_to_string(&self.action_path) else {
            return Ok(false);
        };
        let mut changed = false;
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let Ok(action) = serde_json::from_str::<CronActionRecord>(line) else {
                continue;
            };
            match action.action.as_str() {
                "del" => {
                    if let Some(job_id) = action
                        .params
                        .get("job_id")
                        .or_else(|| action.params.get("jobId"))
                        .and_then(Value::as_str)
                    {
                        store.jobs.retain(|job| job.id != job_id);
                        changed = true;
                    }
                }
                "add" | "update" => {
                    if let Ok(job) = serde_json::from_value::<CronJob>(action.params) {
                        store.jobs.retain(|existing| existing.id != job.id);
                        store.jobs.push(job);
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        Ok(changed)
    }

    fn save_store(&self, store: &CronStore) -> io::Result<()> {
        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(store)?;
        fs::write(&self.store_path, text)
    }
}

impl CronSupervisor {
    pub fn start(
        service: Arc<PersistentCronService>,
        executor: Arc<dyn CronJobExecutor>,
        config: CronSupervisorConfig,
    ) -> io::Result<Self> {
        service.start()?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_service = Arc::clone(&service);
        let wake = Arc::clone(&service.wake);
        let max_sleep_ms = config.max_sleep_ms.max(1);
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                let now = now_ms();
                let next = thread_service.next_wake_at_ms();
                let sleep_ms = next
                    .map(|next| next.saturating_sub(now).max(0) as u64)
                    .unwrap_or(max_sleep_ms)
                    .min(max_sleep_ms);
                let (lock, condvar) = &*wake;
                let generation = *recover_lock(lock);
                let guard = recover_lock(lock);
                let _ = condvar.wait_timeout_while(
                    guard,
                    StdDuration::from_millis(sleep_ms),
                    |current| *current == generation && !thread_stop.load(Ordering::SeqCst),
                );
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let _ = thread_service.tick_due(now_ms(), executor.as_ref());
            }
            thread_service.stop();
        });
        Ok(Self {
            service,
            stop,
            handle: Some(handle),
        })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.service.wake_worker();
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

impl Drop for CronSupervisor {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl CronService for PersistentCronService {
    fn add_job(&self, request: AddCronJob) -> CronJob {
        let now = now_ms();
        let job = CronJob {
            id: self.next_id(),
            name: request.name,
            enabled: true,
            state: CronJobState {
                next_run_at_ms: compute_next_run(&request.schedule, now).ok().flatten(),
                ..CronJobState::default()
            },
            schedule: request.schedule,
            payload: CronPayload {
                kind: CronPayloadKind::AgentTurn,
                message: request.message,
                deliver: request.deliver,
                channel: request.channel,
                to: request.to,
                channel_meta: request.channel_meta,
                session_key: request.session_key,
            },
            created_at_ms: now,
            updated_at_ms: now,
            delete_after_run: request.delete_after_run,
        };
        let _ = self.mutate_store(job.clone(), CronActionKind::Add);
        job
    }

    fn list_jobs(&self) -> Vec<CronJob> {
        let mut jobs = self
            .refresh_store()
            .map_or_else(|_| Vec::new(), |store| store.jobs);
        jobs.retain(|job| job.enabled);
        jobs.sort_by_key(|job| job.state.next_run_at_ms.unwrap_or(i64::MAX));
        jobs
    }

    fn remove_job(&self, job_id: &str) -> RemoveJobResult {
        let store = self.refresh_store().unwrap_or_default();
        let Some(job) = store.jobs.iter().find(|job| job.id == job_id).cloned() else {
            return RemoveJobResult::NotFound;
        };
        if job.payload.kind == CronPayloadKind::SystemEvent {
            return RemoveJobResult::Protected;
        }
        if self.mutate_store(job, CronActionKind::Delete).is_ok() {
            RemoveJobResult::Removed
        } else {
            RemoveJobResult::NotFound
        }
    }

    fn get_job(&self, job_id: &str) -> Option<CronJob> {
        self.refresh_store()
            .ok()?
            .jobs
            .into_iter()
            .find(|job| job.id == job_id)
    }

    fn register_system_job(&self, mut job: CronJob) -> CronJob {
        let now = now_ms();
        job.state = CronJobState {
            next_run_at_ms: compute_next_run(&job.schedule, now).ok().flatten(),
            ..CronJobState::default()
        };
        job.created_at_ms = now;
        job.updated_at_ms = now;
        let mut store = self.refresh_store().unwrap_or_default();
        store.jobs.retain(|existing| existing.id != job.id);
        store.jobs.push(job.clone());
        let _ = self.save_store(&store);
        *recover_lock(&self.store) = store;
        self.wake_worker();
        job
    }

    fn enable_job(&self, job_id: &str, enabled: bool) -> Option<CronJob> {
        let now = now_ms();
        let mut store = self.refresh_store().ok()?;
        let job = store.jobs.iter_mut().find(|job| job.id == job_id)?;
        job.enabled = enabled;
        job.updated_at_ms = now;
        job.state.next_run_at_ms = if enabled {
            compute_next_run(&job.schedule, now).ok().flatten()
        } else {
            None
        };
        let updated = job.clone();
        if self.is_running() {
            let _ = self.save_store(&store);
        } else {
            let _ =
                self.append_action(CronActionKind::Update, serde_json::to_value(&updated).ok()?);
        }
        *recover_lock(&self.store) = store;
        self.wake_worker();
        Some(updated)
    }

    fn update_job(&self, job_id: &str, update: CronJobUpdate) -> UpdateJobResult {
        let now = now_ms();
        let mut store = match self.refresh_store() {
            Ok(store) => store,
            Err(_) => return UpdateJobResult::NotFound,
        };
        let Some(job) = store.jobs.iter_mut().find(|job| job.id == job_id) else {
            return UpdateJobResult::NotFound;
        };
        if job.payload.kind == CronPayloadKind::SystemEvent {
            return UpdateJobResult::Protected;
        }
        apply_job_update(job, update, now);
        let updated = job.clone();
        if self.is_running() {
            let _ = self.save_store(&store);
        } else if let Ok(value) = serde_json::to_value(&updated) {
            let _ = self.append_action(CronActionKind::Update, value);
        }
        *recover_lock(&self.store) = store;
        self.wake_worker();
        UpdateJobResult::Updated(Box::new(updated))
    }

    fn status(&self) -> CronStatusSnapshot {
        let store = self.refresh_store().unwrap_or_default();
        CronStatusSnapshot {
            running: self.is_running(),
            jobs: store.jobs.len(),
            next_wake_at_ms: next_wake_at_ms(&store),
        }
    }
}

pub fn system_job(
    id: impl Into<String>,
    name: impl Into<String>,
    schedule: CronSchedule,
) -> CronJob {
    CronJob {
        id: id.into(),
        name: name.into(),
        enabled: true,
        schedule,
        payload: CronPayload {
            kind: CronPayloadKind::SystemEvent,
            ..CronPayload::default()
        },
        state: CronJobState::default(),
        created_at_ms: 0,
        updated_at_ms: 0,
        delete_after_run: false,
    }
}

pub type ChannelMetadata = HashMap<String, Value>;

pub fn compute_next_run(
    schedule: &CronSchedule,
    now_ms: i64,
) -> Result<Option<i64>, CronScheduleError> {
    match schedule.kind {
        CronScheduleKind::At => Ok(schedule.at_ms.filter(|at_ms| *at_ms > now_ms)),
        CronScheduleKind::Every => {
            let every_ms = schedule
                .every_ms
                .filter(|every_ms| *every_ms > 0)
                .ok_or(CronScheduleError::InvalidInterval)?;
            Ok(Some(now_ms.saturating_add(every_ms)))
        }
        CronScheduleKind::Cron => compute_next_cron_run(schedule, now_ms),
    }
}

pub fn validate_schedule_for_add(schedule: &CronSchedule) -> Result<(), CronScheduleError> {
    if schedule.tz.is_some() && schedule.kind != CronScheduleKind::Cron {
        return Err(CronScheduleError::UnknownTimezone(
            schedule.tz.clone().unwrap_or_default(),
        ));
    }
    match schedule.kind {
        CronScheduleKind::At => {
            if schedule.at_ms.is_some() {
                Ok(())
            } else {
                Err(CronScheduleError::InvalidAt)
            }
        }
        CronScheduleKind::Every => {
            if schedule.every_ms.is_some_and(|every_ms| every_ms > 0) {
                Ok(())
            } else {
                Err(CronScheduleError::InvalidInterval)
            }
        }
        CronScheduleKind::Cron => {
            let _ = compute_next_cron_run(schedule, now_ms())?;
            Ok(())
        }
    }
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn load_store_file(path: &Path) -> io::Result<CronStore> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(CronStore::default()),
        Err(error) => Err(error),
    }
}

fn recompute_next_runs(store: &mut CronStore, now_ms: i64) {
    for job in &mut store.jobs {
        if job.enabled {
            job.state.next_run_at_ms = compute_next_run(&job.schedule, now_ms).ok().flatten();
        }
    }
}

fn next_wake_at_ms(store: &CronStore) -> Option<i64> {
    store
        .jobs
        .iter()
        .filter(|job| job.enabled)
        .filter_map(|job| job.state.next_run_at_ms)
        .min()
}

fn trim_run_history(history: &mut Vec<CronRunRecord>) {
    const MAX_RUN_HISTORY: usize = 20;
    if history.len() > MAX_RUN_HISTORY {
        history.drain(0..history.len() - MAX_RUN_HISTORY);
    }
}

fn apply_job_update(job: &mut CronJob, update: CronJobUpdate, now_ms: i64) {
    if let Some(name) = update.name {
        job.name = name;
    }
    if let Some(schedule) = update.schedule {
        job.schedule = schedule;
    }
    if let Some(message) = update.message {
        job.payload.message = message;
    }
    if let Some(deliver) = update.deliver {
        job.payload.deliver = deliver;
    }
    if let Some(channel) = update.channel {
        job.payload.channel = channel;
    }
    if let Some(to) = update.to {
        job.payload.to = to;
    }
    if let Some(delete_after_run) = update.delete_after_run {
        job.delete_after_run = delete_after_run;
    }
    job.updated_at_ms = now_ms;
    if job.enabled {
        job.state.next_run_at_ms = compute_next_run(&job.schedule, now_ms).ok().flatten();
    }
}

fn execute_store_job(
    store: &mut CronStore,
    index: usize,
    start_ms: i64,
    executor: &dyn CronJobExecutor,
) -> (CronRunOutcome, bool) {
    let result = executor.execute(&store.jobs[index]);
    let end_ms = now_ms();
    let (status, error) = match result {
        Ok(_) => (CronRunStatus::Ok, None),
        Err(error) => (CronRunStatus::Error, Some(error)),
    };
    let job_id = store.jobs[index].id.clone();
    {
        let job = &mut store.jobs[index];
        job.state.last_run_at_ms = Some(start_ms);
        job.state.last_status = Some(status.clone());
        job.state.last_error = error.clone();
        job.updated_at_ms = end_ms;
        job.state.run_history.push(CronRunRecord {
            run_at_ms: start_ms,
            status: status.clone(),
            duration_ms: end_ms.saturating_sub(start_ms),
            error: error.clone(),
        });
        trim_run_history(&mut job.state.run_history);
    }

    let removed = if store.jobs[index].schedule.kind == CronScheduleKind::At {
        if store.jobs[index].delete_after_run {
            store.jobs.remove(index);
            true
        } else {
            store.jobs[index].enabled = false;
            store.jobs[index].state.next_run_at_ms = None;
            false
        }
    } else {
        store.jobs[index].state.next_run_at_ms =
            compute_next_run(&store.jobs[index].schedule, end_ms)
                .ok()
                .flatten();
        false
    };

    (
        CronRunOutcome {
            job_id,
            status,
            error,
        },
        removed,
    )
}

#[derive(Debug, Clone)]
struct CronField {
    values: Vec<u32>,
    wildcard: bool,
}

fn compute_next_cron_run(
    schedule: &CronSchedule,
    now_ms: i64,
) -> Result<Option<i64>, CronScheduleError> {
    let expr = schedule
        .expr
        .as_deref()
        .filter(|expr| !expr.trim().is_empty())
        .ok_or_else(|| CronScheduleError::InvalidCronExpression("missing expression".to_owned()))?;
    let tz: Tz = match schedule.tz.as_deref().unwrap_or("UTC").parse() {
        Ok(tz) => tz,
        Err(_) => {
            return Err(CronScheduleError::UnknownTimezone(
                schedule.tz.clone().unwrap_or_default(),
            ))
        }
    };
    let parts = expr.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 5 {
        return Err(CronScheduleError::InvalidCronExpression(
            "expected five fields".to_owned(),
        ));
    }
    let minute = parse_cron_field(parts[0], 0, 59)?;
    let hour = parse_cron_field(parts[1], 0, 23)?;
    let day = parse_cron_field(parts[2], 1, 31)?;
    let month = parse_cron_field(parts[3], 1, 12)?;
    let weekday = parse_cron_field(parts[4], 0, 7)?;
    let start = Utc.timestamp_millis_opt(now_ms).single().ok_or_else(|| {
        CronScheduleError::InvalidCronExpression("invalid base timestamp".to_owned())
    })?;
    let mut candidate = truncate_to_minute(start) + Duration::minutes(1);
    let max_candidate = candidate + Duration::days(366 * 5);
    while candidate <= max_candidate {
        let local = candidate.with_timezone(&tz);
        let weekday_value = local.weekday().num_days_from_sunday();
        let weekday_matches = weekday.values.contains(&weekday_value)
            || (weekday_value == 0 && weekday.values.contains(&7));
        let day_matches = day.values.contains(&local.day());
        let calendar_day_matches = if day.wildcard || weekday.wildcard {
            day_matches && weekday_matches
        } else {
            day_matches || weekday_matches
        };
        if minute.values.contains(&local.minute())
            && hour.values.contains(&local.hour())
            && month.values.contains(&local.month())
            && calendar_day_matches
        {
            return Ok(Some(candidate.timestamp_millis()));
        }
        candidate += Duration::minutes(1);
    }
    Ok(None)
}

fn truncate_to_minute(value: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(
        value.year(),
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        0,
    )
    .single()
    .unwrap_or(value)
}

fn parse_cron_field(input: &str, min: u32, max: u32) -> Result<CronField, CronScheduleError> {
    let mut values = Vec::new();
    let mut wildcard = false;
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(CronScheduleError::InvalidCronExpression(
                "empty field segment".to_owned(),
            ));
        }
        let (range, step) = if let Some((range, step)) = part.split_once('/') {
            let step = step.parse::<u32>().map_err(|_| {
                CronScheduleError::InvalidCronExpression(format!("invalid step `{step}`"))
            })?;
            if step == 0 {
                return Err(CronScheduleError::InvalidCronExpression(
                    "step must be > 0".to_owned(),
                ));
            }
            (range, step)
        } else {
            (part, 1)
        };
        let (start, end) = if range == "*" {
            wildcard = true;
            (min, max)
        } else if let Some((start, end)) = range.split_once('-') {
            (
                parse_cron_number(start, min, max)?,
                parse_cron_number(end, min, max)?,
            )
        } else {
            let value = parse_cron_number(range, min, max)?;
            (value, value)
        };
        if start > end {
            return Err(CronScheduleError::InvalidCronExpression(format!(
                "range start {start} exceeds end {end}"
            )));
        }
        let mut value = start;
        while value <= end {
            if !values.contains(&value) {
                values.push(value);
            }
            value = value.saturating_add(step);
            if step == 0 {
                break;
            }
        }
    }
    values.sort_unstable();
    Ok(CronField { values, wildcard })
}

fn parse_cron_number(input: &str, min: u32, max: u32) -> Result<u32, CronScheduleError> {
    let value = input.parse::<u32>().map_err(|_| {
        CronScheduleError::InvalidCronExpression(format!("invalid number `{input}`"))
    })?;
    if value < min || value > max {
        return Err(CronScheduleError::InvalidCronExpression(format!(
            "value {value} outside {min}..={max}"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration as TestDuration, Instant};

    #[test]
    fn cron_store_serializes_with_python_compatible_field_names() -> Result<(), Box<dyn Error>> {
        let store = CronStore {
            version: 1,
            jobs: vec![CronJob {
                id: "job1".to_owned(),
                name: "reminder".to_owned(),
                enabled: true,
                schedule: CronSchedule::cron("0 9 * * *", "Asia/Seoul"),
                payload: CronPayload {
                    kind: CronPayloadKind::AgentTurn,
                    message: "hello".to_owned(),
                    deliver: true,
                    channel: Some("telegram".to_owned()),
                    to: Some("chat-1".to_owned()),
                    channel_meta: json!({ "thread": "abc" }),
                    session_key: Some("session-1".to_owned()),
                },
                state: CronJobState {
                    next_run_at_ms: Some(1_700_000_000_000),
                    last_run_at_ms: Some(1_699_999_000_000),
                    last_status: Some(CronRunStatus::Ok),
                    last_error: None,
                    run_history: vec![CronRunRecord {
                        run_at_ms: 1_699_999_000_000,
                        status: CronRunStatus::Ok,
                        duration_ms: 42,
                        error: None,
                    }],
                },
                created_at_ms: 1,
                updated_at_ms: 2,
                delete_after_run: false,
            }],
        };

        let value = serde_json::to_value(&store)?;
        assert_eq!(value["jobs"][0]["schedule"]["everyMs"], Value::Null);
        assert_eq!(value["jobs"][0]["schedule"]["expr"], "0 9 * * *");
        assert_eq!(value["jobs"][0]["payload"]["channelMeta"]["thread"], "abc");
        assert_eq!(value["jobs"][0]["payload"]["sessionKey"], "session-1");
        assert_eq!(
            value["jobs"][0]["state"]["nextRunAtMs"],
            1_700_000_000_000i64
        );
        assert_eq!(value["jobs"][0]["state"]["runHistory"][0]["durationMs"], 42);
        assert_eq!(value["jobs"][0]["deleteAfterRun"], false);

        let decoded: CronStore = serde_json::from_value(value)?;
        assert_eq!(decoded, store);
        Ok(())
    }

    #[test]
    fn schedule_next_run_supports_at_every_and_timezone_cron() -> Result<(), Box<dyn Error>> {
        let base = Utc
            .with_ymd_and_hms(2026, 5, 4, 0, 0, 0)
            .single()
            .ok_or("invalid base time")?
            .timestamp_millis();

        assert_eq!(
            compute_next_run(&CronSchedule::at(base + 1_000), base)?,
            Some(base + 1_000)
        );
        assert_eq!(compute_next_run(&CronSchedule::at(base - 1), base)?, None);
        assert_eq!(
            compute_next_run(&CronSchedule::every(5_000), base)?,
            Some(base + 5_000)
        );

        let next = compute_next_run(&CronSchedule::cron("0 9 * * *", "Asia/Seoul"), base)?
            .ok_or("missing cron next run")?;
        let expected = chrono_tz::Asia::Seoul
            .with_ymd_and_hms(2026, 5, 5, 9, 0, 0)
            .single()
            .ok_or("invalid expected time")?
            .timestamp_millis();
        assert_eq!(next, expected);
        assert!(validate_schedule_for_add(&CronSchedule::cron("*/15 9-18 * * 1-5", "UTC")).is_ok());
        assert!(validate_schedule_for_add(&CronSchedule::cron("bad", "UTC")).is_err());
        Ok(())
    }

    #[test]
    fn persistent_service_merges_action_log_and_writes_store() -> Result<(), Box<dyn Error>> {
        let root = temp_dir("cron-actions")?;
        let store_path = root.join("jobs.json");
        let service = PersistentCronService::new(&store_path)?;
        let job = service.add_job(AddCronJob {
            name: "offline add".to_owned(),
            schedule: CronSchedule::every(60_000),
            message: "ping".to_owned(),
            deliver: true,
            channel: Some("telegram".to_owned()),
            to: Some("chat".to_owned()),
            delete_after_run: false,
            channel_meta: json!({"thread": "t1"}),
            session_key: Some("telegram:chat".to_owned()),
        });

        assert!(service.action_path().exists());
        assert_eq!(service.list_jobs().len(), 1);
        assert!(!store_path.exists());

        service.start()?;
        assert!(store_path.exists());
        assert_eq!(fs::read_to_string(service.action_path())?, "");
        let saved: CronStore = serde_json::from_str(&fs::read_to_string(&store_path)?)?;
        assert_eq!(saved.jobs[0].id, job.id);
        assert_eq!(saved.jobs[0].payload.channel_meta["thread"], "t1");

        fs::write(
            service.action_path(),
            format!(
                "{}\n",
                json!({
                    "action": "update",
                    "params": {
                        "id": job.id,
                        "name": "snake update",
                        "enabled": true,
                        "schedule": {"kind": "every", "every_ms": 120000},
                        "payload": {"kind": "agent_turn", "message": "pong", "deliver": false, "channel_meta": {"x": 1}, "session_key": "s"},
                        "state": {"next_run_at_ms": 123, "run_history": [{"run_at_ms": 1, "status": "ok", "duration_ms": 2}]},
                        "created_at_ms": 1,
                        "updated_at_ms": 2,
                        "delete_after_run": false
                    }
                })
            ),
        )?;

        let updated = service.get_job(&job.id).ok_or("missing updated job")?;
        assert_eq!(updated.name, "snake update");
        assert_eq!(updated.schedule.every_ms, Some(120_000));
        assert_eq!(updated.payload.session_key.as_deref(), Some("s"));
        assert_eq!(updated.state.run_history[0].duration_ms, 2);
        Ok(())
    }

    #[test]
    fn persistent_tick_due_updates_history_and_handles_one_shot_delete(
    ) -> Result<(), Box<dyn Error>> {
        let root = temp_dir("cron-tick")?;
        let service = PersistentCronService::new(root.join("jobs.json"))?;
        let now = now_ms().saturating_add(10_000);
        let recurring = service.add_job(AddCronJob {
            name: "recurring".to_owned(),
            schedule: CronSchedule::every(60_000),
            message: "again".to_owned(),
            deliver: false,
            channel: None,
            to: None,
            delete_after_run: false,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        });
        let one_shot = service.add_job(AddCronJob {
            name: "once".to_owned(),
            schedule: CronSchedule::at(now + 1_000),
            message: "once".to_owned(),
            deliver: false,
            channel: None,
            to: None,
            delete_after_run: true,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        });

        service.start()?;
        let mut recurring_job = service.get_job(&recurring.id).ok_or("missing recurring")?;
        recurring_job.state.next_run_at_ms = Some(now);
        service.update_job(
            &recurring.id,
            CronJobUpdate {
                name: Some(recurring_job.name.clone()),
                schedule: Some(recurring_job.schedule.clone()),
                message: Some(recurring_job.payload.message.clone()),
                deliver: Some(recurring_job.payload.deliver),
                channel: Some(recurring_job.payload.channel.clone()),
                to: Some(recurring_job.payload.to.clone()),
                delete_after_run: Some(recurring_job.delete_after_run),
            },
        );
        {
            let mut store = service.refresh_store()?;
            if let Some(job) = store.jobs.iter_mut().find(|job| job.id == recurring.id) {
                job.state.next_run_at_ms = Some(now);
            }
            service.save_store(&store)?;
        }

        let outcomes = service.tick_due(now + 1_000, &|job: &CronJob| {
            if job.id == recurring.id || job.id == one_shot.id {
                Ok(Some("done".to_owned()))
            } else {
                Err("unexpected job".to_owned())
            }
        })?;

        assert_eq!(outcomes.len(), 2);
        assert!(service.get_job(&one_shot.id).is_none());
        let recurring = service.get_job(&recurring.id).ok_or("recurring removed")?;
        assert_eq!(recurring.state.last_status, Some(CronRunStatus::Ok));
        assert_eq!(recurring.state.run_history.len(), 1);
        assert!(recurring.state.next_run_at_ms.unwrap_or_default() > now);
        Ok(())
    }

    #[test]
    fn persistent_run_job_force_executes_disabled_job() -> Result<(), Box<dyn Error>> {
        let root = temp_dir("cron-force")?;
        let service = PersistentCronService::new(root.join("jobs.json"))?;
        let job = service.add_job(AddCronJob {
            name: "disabled".to_owned(),
            schedule: CronSchedule::every(60_000),
            message: "manual".to_owned(),
            deliver: false,
            channel: None,
            to: None,
            delete_after_run: false,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        });
        service.start()?;
        let disabled = service
            .enable_job(&job.id, false)
            .ok_or("missing job to disable")?;
        assert!(!disabled.enabled);

        let calls = Mutex::new(0usize);
        assert!(!service.run_job(&job.id, false, &|_job: &CronJob| {
            *recover_lock(&calls) += 1;
            Ok(Some("should not run".to_owned()))
        })?);
        assert_eq!(*recover_lock(&calls), 0);

        assert!(service.run_job(&job.id, true, &|_job: &CronJob| {
            *recover_lock(&calls) += 1;
            Ok(Some("forced".to_owned()))
        })?);

        let updated = service.get_job(&job.id).ok_or("forced job missing")?;
        assert!(!updated.enabled);
        assert_eq!(updated.state.last_status, Some(CronRunStatus::Ok));
        assert_eq!(updated.state.run_history.len(), 1);
        assert_eq!(*recover_lock(&calls), 1);
        Ok(())
    }

    #[test]
    fn persistent_system_jobs_are_idempotent_and_protected() -> Result<(), Box<dyn Error>> {
        let root = temp_dir("cron-system")?;
        let service = PersistentCronService::new(root.join("jobs.json"))?;
        let dream = system_job("dream", "Dream", CronSchedule::every(60_000));
        service.register_system_job(dream.clone());
        service.register_system_job(dream);

        assert_eq!(service.status().jobs, 1);
        assert_eq!(service.remove_job("dream"), RemoveJobResult::Protected);
        assert!(matches!(
            service.update_job(
                "dream",
                CronJobUpdate {
                    name: Some("new".to_owned()),
                    schedule: None,
                    message: None,
                    deliver: None,
                    channel: None,
                    to: None,
                    delete_after_run: None,
                }
            ),
            UpdateJobResult::Protected
        ));
        Ok(())
    }

    #[test]
    fn supervisor_wakes_for_added_due_job_and_stops() -> Result<(), Box<dyn Error>> {
        let root = temp_dir("cron-supervisor")?;
        let service = Arc::new(PersistentCronService::new(root.join("jobs.json"))?);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_executor = Arc::clone(&calls);
        let executor: Arc<dyn CronJobExecutor> = Arc::new(move |_job: &CronJob| {
            calls_for_executor.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        });
        let supervisor = CronSupervisor::start(
            Arc::clone(&service),
            executor,
            CronSupervisorConfig {
                max_sleep_ms: 60_000,
            },
        )?;

        service.add_job(AddCronJob {
            name: "wake soon".to_owned(),
            schedule: CronSchedule::every(1),
            message: "ping".to_owned(),
            deliver: false,
            channel: None,
            to: None,
            delete_after_run: false,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        });

        let deadline = Instant::now() + TestDuration::from_secs(2);
        while calls.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(TestDuration::from_millis(10));
        }
        assert!(calls.load(Ordering::SeqCst) > 0);
        supervisor.join().map_err(|_| "cron supervisor panicked")?;
        assert!(!service.is_running());
        Ok(())
    }

    #[test]
    fn supervisor_wakes_for_enable_update_and_system_registration() -> Result<(), Box<dyn Error>> {
        let root = temp_dir("cron-supervisor-mutations")?;
        let service = Arc::new(PersistentCronService::new(root.join("jobs.json"))?);
        let disabled = service.add_job(AddCronJob {
            name: "disabled".to_owned(),
            schedule: CronSchedule::every(1),
            message: "enable".to_owned(),
            deliver: false,
            channel: None,
            to: None,
            delete_after_run: false,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        });
        service
            .enable_job(&disabled.id, false)
            .ok_or("disable job")?;
        let slow = service.add_job(AddCronJob {
            name: "slow".to_owned(),
            schedule: CronSchedule::every(60_000),
            message: "update".to_owned(),
            deliver: false,
            channel: None,
            to: None,
            delete_after_run: false,
            channel_meta: Value::Object(Default::default()),
            session_key: None,
        });
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_for_executor = Arc::clone(&calls);
        let executor: Arc<dyn CronJobExecutor> = Arc::new(move |job: &CronJob| {
            recover_lock(&calls_for_executor).push(job.id.clone());
            Ok(None)
        });
        let supervisor = CronSupervisor::start(
            Arc::clone(&service),
            executor,
            CronSupervisorConfig {
                max_sleep_ms: 60_000,
            },
        )?;

        service.enable_job(&disabled.id, true).ok_or("enable job")?;
        wait_for_call(&calls, &disabled.id)?;

        service.update_job(
            &slow.id,
            CronJobUpdate {
                name: None,
                schedule: Some(CronSchedule::every(1)),
                message: None,
                deliver: None,
                channel: None,
                to: None,
                delete_after_run: None,
            },
        );
        wait_for_call(&calls, &slow.id)?;

        let system = system_job("system-wake", "System Wake", CronSchedule::every(1));
        service.register_system_job(system);
        wait_for_call(&calls, "system-wake")?;

        supervisor.join().map_err(|_| "cron supervisor panicked")?;
        Ok(())
    }

    fn wait_for_call(calls: &Mutex<Vec<String>>, job_id: &str) -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + TestDuration::from_secs(2);
        while Instant::now() < deadline {
            if recover_lock(calls).iter().any(|id| id == job_id) {
                return Ok(());
            }
            thread::sleep(TestDuration::from_millis(10));
        }
        Err(format!("job `{job_id}` was not executed after wake").into())
    }

    fn temp_dir(name: &str) -> io::Result<PathBuf> {
        let path = std::env::temp_dir().join(format!(
            "shacs-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}
