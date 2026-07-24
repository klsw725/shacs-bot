use shacs_providers::ProviderEvent;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Default, Clone)]
pub struct SessionTurnLock {
    state: Arc<Mutex<SessionTurnState>>,
}

#[derive(Debug, Default)]
struct SessionTurnState {
    active_sessions: BTreeMap<String, CancellationToken>,
    pending_cancellations: BTreeSet<String>,
    reserved_sessions: BTreeMap<String, ReservedSessionTurn>,
    next_reservation_id: u64,
}

#[derive(Debug)]
struct ReservedSessionTurn {
    id: u64,
    owner: Option<std::thread::ThreadId>,
    cancelled: bool,
}

impl SessionTurnLock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(
        &self,
        session_key: impl Into<String>,
    ) -> Result<SessionTurnGuard, SessionTurnAcquireError> {
        self.acquire_with_pending_cancellation(session_key.into(), true)
    }

    pub fn acquire_priority(
        &self,
        session_key: impl Into<String>,
    ) -> Result<SessionTurnGuard, SessionTurnAcquireError> {
        self.acquire_with_pending_cancellation(session_key.into(), false)
    }

    fn acquire_with_pending_cancellation(
        &self,
        session_key: String,
        consume_pending_cancellation: bool,
    ) -> Result<SessionTurnGuard, SessionTurnAcquireError> {
        let mut state = recover_lock(&self.state);
        let pending_cancellation = if consume_pending_cancellation {
            match state.reserved_sessions.get(&session_key) {
                Some(reservation) if reservation.owner == Some(std::thread::current().id()) => {
                    let cancelled = reservation.cancelled;
                    state.reserved_sessions.remove(&session_key);
                    cancelled
                }
                Some(_) => {
                    return Err(SessionTurnAcquireError::AlreadyActive { session_key });
                }
                None => state.pending_cancellations.remove(&session_key),
            }
        } else {
            false
        };
        if state.active_sessions.contains_key(&session_key) {
            return Err(SessionTurnAcquireError::AlreadyActive { session_key });
        }
        let cancellation = CancellationToken::new();
        if pending_cancellation {
            cancellation.cancel();
        }
        state
            .active_sessions
            .insert(session_key.clone(), cancellation);
        Ok(SessionTurnGuard {
            state: self.state.clone(),
            session_key,
        })
    }

    pub fn active_session_keys(&self) -> Vec<String> {
        recover_lock(&self.state)
            .active_sessions
            .keys()
            .cloned()
            .collect()
    }

    pub fn busy_session_keys(&self) -> Vec<String> {
        let state = recover_lock(&self.state);
        let mut keys = state
            .active_sessions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        keys.extend(state.reserved_sessions.keys().cloned());
        keys.into_iter().collect()
    }

    pub fn is_active(&self, session_key: &str) -> bool {
        recover_lock(&self.state)
            .active_sessions
            .contains_key(session_key)
    }

    pub fn is_busy(&self, session_key: &str) -> bool {
        let state = recover_lock(&self.state);
        state.active_sessions.contains_key(session_key)
            || state.reserved_sessions.contains_key(session_key)
    }

    pub fn reserve(&self, session_key: impl Into<String>) -> SessionTurnReservation {
        let session_key = session_key.into();
        let mut state = recover_lock(&self.state);
        state.next_reservation_id = state.next_reservation_id.wrapping_add(1);
        let id = state.next_reservation_id;
        state.reserved_sessions.insert(
            session_key.clone(),
            ReservedSessionTurn {
                id,
                owner: None,
                cancelled: false,
            },
        );
        SessionTurnReservation {
            state: self.state.clone(),
            session_key,
            id,
        }
    }

    pub fn cancellation_token(&self, session_key: &str) -> Option<CancellationToken> {
        recover_lock(&self.state)
            .active_sessions
            .get(session_key)
            .cloned()
    }

    pub fn cancel(&self, session_key: &str) -> bool {
        let mut state = recover_lock(&self.state);
        if let Some(token) = state.active_sessions.get(session_key) {
            token.cancel();
            true
        } else {
            state.pending_cancellations.insert(session_key.to_owned())
        }
    }

    pub fn cancel_active_or_reserved(&self, session_key: &str) -> bool {
        let mut state = recover_lock(&self.state);
        if let Some(token) = state.active_sessions.get(session_key) {
            token.cancel();
            true
        } else if let Some(reservation) = state.reserved_sessions.get_mut(session_key) {
            reservation.cancelled = true;
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub struct SessionTurnReservation {
    state: Arc<Mutex<SessionTurnState>>,
    session_key: String,
    id: u64,
}

impl SessionTurnReservation {
    pub fn bind_to_current_thread(&self) {
        let mut state = recover_lock(&self.state);
        if let Some(reservation) = state.reserved_sessions.get_mut(&self.session_key) {
            if reservation.id == self.id {
                reservation.owner = Some(std::thread::current().id());
            }
        }
    }
}

impl Drop for SessionTurnReservation {
    fn drop(&mut self) {
        let mut state = recover_lock(&self.state);
        if state
            .reserved_sessions
            .get(&self.session_key)
            .is_some_and(|reservation| reservation.id == self.id)
        {
            state.reserved_sessions.remove(&self.session_key);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTurnAcquireError {
    AlreadyActive { session_key: String },
}

#[derive(Debug)]
pub struct SessionTurnGuard {
    state: Arc<Mutex<SessionTurnState>>,
    session_key: String,
}

impl SessionTurnGuard {
    pub fn session_key(&self) -> &str {
        &self.session_key
    }
}

impl Drop for SessionTurnGuard {
    fn drop(&mut self) {
        recover_lock(&self.state)
            .active_sessions
            .remove(&self.session_key);
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
