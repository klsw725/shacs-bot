use shacs_providers::ProviderEvent;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Default, Clone)]
pub struct SessionTurnLock {
    active_sessions: Arc<Mutex<BTreeSet<String>>>,
}

impl SessionTurnLock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(
        &self,
        session_key: impl Into<String>,
    ) -> Result<SessionTurnGuard, SessionTurnAcquireError> {
        let session_key = session_key.into();
        let mut active_sessions = recover_lock(&self.active_sessions);
        if active_sessions.contains(&session_key) {
            return Err(SessionTurnAcquireError::AlreadyActive { session_key });
        }
        active_sessions.insert(session_key.clone());
        Ok(SessionTurnGuard {
            active_sessions: self.active_sessions.clone(),
            session_key,
        })
    }

    pub fn active_session_keys(&self) -> Vec<String> {
        recover_lock(&self.active_sessions)
            .iter()
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTurnAcquireError {
    AlreadyActive { session_key: String },
}

#[derive(Debug)]
pub struct SessionTurnGuard {
    active_sessions: Arc<Mutex<BTreeSet<String>>>,
    session_key: String,
}

impl SessionTurnGuard {
    pub fn session_key(&self) -> &str {
        &self.session_key
    }
}

impl Drop for SessionTurnGuard {
    fn drop(&mut self) {
        recover_lock(&self.active_sessions).remove(&self.session_key);
    }
}

#[derive(Debug, Default, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopTaskStatus {
    Running,
    CancellationRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLoopTaskSnapshot {
    pub session_key: String,
    pub task_id: String,
    pub status: LoopTaskStatus,
}

#[derive(Debug, Clone)]
pub struct ActiveLoopTask {
    session_key: String,
    task_id: String,
    cancellation: CancellationToken,
}

impl ActiveLoopTask {
    pub fn new(
        session_key: impl Into<String>,
        task_id: impl Into<String>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            session_key: session_key.into(),
            task_id: task_id.into(),
            cancellation,
        }
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn snapshot(&self) -> ActiveLoopTaskSnapshot {
        ActiveLoopTaskSnapshot {
            session_key: self.session_key.clone(),
            task_id: self.task_id.clone(),
            status: if self.cancellation.is_cancelled() {
                LoopTaskStatus::CancellationRequested
            } else {
                LoopTaskStatus::Running
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopTaskRegisterResult {
    Registered,
    DuplicateActive { session_key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopTaskCancelResult {
    NoAsyncTask,
    CancellationRequested(ActiveLoopTaskSnapshot),
}

#[derive(Debug, Default, Clone)]
pub struct LoopTaskRegistry {
    tasks: Arc<Mutex<BTreeMap<String, ActiveLoopTask>>>,
}

impl LoopTaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, task: ActiveLoopTask) -> LoopTaskRegisterResult {
        let mut tasks = recover_lock(&self.tasks);
        if tasks.contains_key(task.session_key()) {
            return LoopTaskRegisterResult::DuplicateActive {
                session_key: task.session_key().to_owned(),
            };
        }
        tasks.insert(task.session_key().to_owned(), task);
        LoopTaskRegisterResult::Registered
    }

    pub fn complete(&self, session_key: &str) -> Option<ActiveLoopTaskSnapshot> {
        recover_lock(&self.tasks)
            .remove(session_key)
            .map(|task| task.snapshot())
    }

    pub fn cancel(&self, session_key: &str) -> LoopTaskCancelResult {
        let tasks = recover_lock(&self.tasks);
        let Some(task) = tasks.get(session_key) else {
            return LoopTaskCancelResult::NoAsyncTask;
        };
        task.cancellation.cancel();
        LoopTaskCancelResult::CancellationRequested(task.snapshot())
    }

    pub fn snapshot(&self, session_key: &str) -> Option<ActiveLoopTaskSnapshot> {
        recover_lock(&self.tasks)
            .get(session_key)
            .map(ActiveLoopTask::snapshot)
    }

    pub fn cancellation_token(&self, session_key: &str) -> Option<CancellationToken> {
        recover_lock(&self.tasks)
            .get(session_key)
            .map(ActiveLoopTask::cancellation_token)
    }

    pub fn snapshots(&self) -> Vec<ActiveLoopTaskSnapshot> {
        recover_lock(&self.tasks)
            .values()
            .map(ActiveLoopTask::snapshot)
            .collect()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StreamDeltaBatch {
    pub text: String,
    pub reasoning: String,
}

#[derive(Debug, Default, Clone)]
pub struct StreamDeltaCoalescer {
    text: String,
    reasoning: String,
}

impl StreamDeltaCoalescer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: &ProviderEvent) -> Option<StreamDeltaBatch> {
        match event {
            ProviderEvent::TextDelta { text } => {
                self.text.push_str(text);
                None
            }
            ProviderEvent::ReasoningDelta { text } => {
                self.reasoning.push_str(text);
                None
            }
            ProviderEvent::Finish { .. }
            | ProviderEvent::ToolCallStart { .. }
            | ProviderEvent::ToolCallDelta { .. }
            | ProviderEvent::ToolCallReady { .. } => self.flush(),
        }
    }

    pub fn flush(&mut self) -> Option<StreamDeltaBatch> {
        if self.text.is_empty() && self.reasoning.is_empty() {
            return None;
        }
        Some(StreamDeltaBatch {
            text: std::mem::take(&mut self.text),
            reasoning: std::mem::take(&mut self.reasoning),
        })
    }
}

fn recover_lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
