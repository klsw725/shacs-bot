use super::browser_lock::{
    self, BrowserFolder, BrowserLockHold, LockSignal, ProfileUse, WatchedLocks,
};
use super::browser_notices::{self, BrowserNotices};
use super::browser_paste::{PasteListener, PasteOutcome, PasteTarget};
use super::browser_session_marker::SessionMarker;
use super::{
    browser_action_binding, browser_state, browser_viewer, BrowserCancellation, BrowserControl,
    BrowserLifecycleFactory, BrowserOwnerBinding, BrowserProcessRunner, BrowserProcesses,
    BrowserSnapshotDestination, BrowserStartup, BrowserState, BrowserStateSnapshot, BrowserStatus,
    BrowserViewState, BrowserViewStatus, BrowserViewerFactory, CdpDocument, ConfigBrowserStartup,
    LinuxBrowserProcessRunner, ViewerRoutes, ViewerWait,
};
use serde_json::{Map, Value};
use shacs_config::ConfigBundle;
use shacs_core::runtime::{
    BrowserAction, BrowserArrival, BrowserCallContext, BrowserCapture, BrowserCloseOutcome,
    BrowserClosureNotice, BrowserControlDescription, BrowserDescription, BrowserDescriptionState,
    BrowserDocument, BrowserError, BrowserHandoffEpisode, BrowserHost, BrowserPendingResume,
    BrowserPreparedAction, BrowserRead, BrowserResumeNotifier, BrowserReturnOutcome,
    BrowserReturnTarget, BrowserSession, BrowserTakeover, BrowserTakeoverDescription,
    BrowserTakeoverError, BrowserTakeoverRequest, BrowserUnavailableReason, BrowserViewDescription,
    ProcessExecutionReceipt, ProcessGate, ProcessSpawnReport, ProcessTerminalOutcome,
    RenderedWebFailure, RenderedWebPage, RenderedWebReader, RenderedWebRequest, WebCheckFailure,
    WebFetchLimits,
};
use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError, Weak};
use std::thread;
use std::time::{Duration, Instant};

/// How many ended episodes are kept for diagnostics.
const SETTLED_EPISODES: usize = 8;
/// How often a request that waits for the slot looks again.
const SLOT_POLL_MS: u64 = 5;
const SLOT_POLL: Duration = Duration::from_millis(SLOT_POLL_MS);
/// How often maintenance looks at the browser lock files.
const LOCK_CHECK_MS: u64 = 1_000;
/// How long a Chromium that was asked to end may take before it is killed.
const QUIT_LIMIT_MS: u64 = 5_000;
const QUIT_POLL_MS: u64 = 50;
/// The status reason of a browser closed because its lock paths changed.
const LOCK_REPLACED: &str = "browser_lock_replaced";

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenerationPurpose {
    Conversation,
    Watch,
}

pub trait BrowserClock: Send + Sync {
    fn now_ms(&self) -> u64;
    /// Lets `ms` of this clock's time pass between two tries at something.
    fn pause(&self, ms: u64) {
        thread::sleep(Duration::from_millis(ms));
    }
}
pub struct SystemBrowserClock;
impl BrowserClock for SystemBrowserClock {
    fn now_ms(&self) -> u64 {
        crate::now_millis()
    }
}

struct Generation {
    processes: BrowserProcesses,
    document: CdpDocument,
    session: String,
    last_used: u64,
    /// When the turn of this session's user ended with the browser open.
    /// While set, the reply wait replaces the idle timeout.
    waiting_since: Option<u64>,
    purpose: GenerationPurpose,
    /// Locked for as long as this generation lives.
    marker: Option<SessionMarker>,
    /// The browser lock files. The last field, so they are released only
    /// after the processes and the temporary folder above are gone.
    locks: Option<BrowserLockHold>,
}

/// What maintenance watches of the lock files (Spec 050 PRD 020 scopes 3
/// and 5). Never held while the slot or the deferred requests are taken.
#[derive(Default)]
struct LockWatch {
    /// The lock files of the generation that is open or starting.
    watched: Option<(u64, WatchedLocks)>,
    checked_ms: u64,
    /// The generation whose lock paths were seen replaced.
    replaced: Option<u64>,
}

#[derive(Clone, Copy)]
enum TurnEdge {
    Started,
    Finished,
}

struct TurnMark {
    session: String,
    edge: TurnEdge,
    at_ms: u64,
}

/// The generation a close can be asked of while the slot is held: the one
/// that is open or still starting.
struct Live {
    number: u64,
    session: String,
    /// A close was accepted for this generation and answered `Scheduled`.
    close: bool,
    /// A watch has its own cancellation signal; it never poisons a later generation.
    watch_cancellation: Option<BrowserCancellation>,
}

/// What arrived while an operation held the slot. Whoever holds the slot
/// next applies it. Accepting a close and ending the generation it is for
/// both happen under this one lock, so a close is either taken along by the
/// end of its generation or answered as having nothing to close.
#[derive(Default)]
struct Deferred {
    turns: Vec<TurnMark>,
    live: Option<Live>,
    /// Sessions whose owner turn runs now, kept as the edges arrive so that
    /// it is right whenever control comes back from the user.
    running: HashSet<String>,
}

/// Why a takeover episode ended (Spec 050 PRD 010 scope 2).
#[derive(Clone, Copy, PartialEq, Eq)]
enum EpisodeEnd {
    Answered,
    Returned,
    Limit,
    Cancelled,
    Closed,
    ViewFailed,
    Failed,
}

impl EpisodeEnd {
    const fn reason(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Returned => "returned",
            Self::Limit => "limit",
            Self::Cancelled => "cancelled",
            Self::Closed => "closed",
            Self::ViewFailed => "view_failed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Opening,
    Open,
    Closing,
}

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Open => "open",
            Self::Closing => "closing",
        }
    }
}

/// One takeover. The browser is under user control exactly while this
/// record exists.
struct Episode {
    number: String,
    label: String,
    /// The session that took the browser over, and where it did.
    session: String,
    channel: String,
    chat_id: String,
    /// Where in that room the asking turn was answering.
    reply_route: Map<String, Value>,
    /// Set from the start for an episode a `handoff` call opened.
    tool_call_id: Option<String>,
    stage: Stage,
    deadline_ms: u64,
    input_spawned: bool,
    /// The `handoff` call really paused: turned on only when the stage
    /// becomes `open`.
    paused: bool,
    /// The paused call is closed by whoever ends the episode, or was closed
    /// some other way, so nothing waits to continue it.
    call_closed: bool,
    asked: bool,
    notice_ids: Vec<String>,
}

/// An ended episode as it is kept in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEpisodeRecord {
    pub episode: String,
    pub reason: String,
    pub session: String,
    pub channel: String,
    pub chat_id: String,
    /// The topic or thread of that room a continued turn answers in.
    pub reply_route: Map<String, Value>,
    pub tool_call_id: Option<String>,
    pub paused: bool,
}

/// The episode that is open now, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserOpenEpisode {
    pub episode: String,
    pub label: String,
    pub stage: &'static str,
    pub input_spawned: bool,
    pub paused: bool,
    pub asked: bool,
    pub notice_ids: Vec<String>,
}

/// Every episode record this process holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserEpisodeRecords {
    pub open: Option<BrowserOpenEpisode>,
    /// Ended with a paused `handoff` call that still waits to continue.
    pub waiting: Vec<BrowserEpisodeRecord>,
    /// The most recent other endings, oldest first.
    pub settled: Vec<BrowserEpisodeRecord>,
}

/// The episode records. This lock is a leaf: nothing else is locked while
/// it is held. The stage changes only under the slot, so whoever holds the
/// slot sees a stage that stays put; the readers that must not wait for the
/// slot (arrival stamps, notice deliveries, `/browser status`, the refusal
/// of an action) read here.
struct Episodes {
    nonce: String,
    counter: u64,
    current: Option<Episode>,
    waiting: Vec<BrowserEpisodeRecord>,
    settled: VecDeque<BrowserEpisodeRecord>,
    /// Continuations not yet handed to the notifier.
    outbox: Vec<BrowserPendingResume>,
}

impl Episodes {
    fn new(seed: u64) -> Self {
        let mut random = [0; 8];
        let nonce =
            match File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut random)) {
                Ok(()) => random.iter().map(|byte| format!("{byte:02x}")).collect(),
                Err(_) => format!(
                    "{:016x}",
                    seed.rotate_left(17) ^ u64::from(std::process::id())
                ),
            };
        Self {
            nonce,
            counter: 0,
            current: None,
            waiting: Vec::new(),
            settled: VecDeque::new(),
            outbox: Vec::new(),
        }
    }

    fn open(&mut self, request: &BrowserTakeoverRequest, deadline_ms: u64) -> (String, String) {
        self.counter += 1;
        let number = format!("{}-{}", self.nonce, self.counter);
        let label = format!("{}-{}", &self.nonce[..4], self.counter);
        self.current = Some(Episode {
            number: number.clone(),
            label: label.clone(),
            session: request.session_id.clone(),
            channel: request.channel.clone(),
            chat_id: request.chat_id.clone(),
            reply_route: request.reply_route.clone(),
            tool_call_id: request.tool_call_id.clone(),
            stage: Stage::Opening,
            deadline_ms,
            input_spawned: false,
            paused: false,
            call_closed: false,
            asked: false,
            notice_ids: Vec::new(),
        });
        (number, label)
    }

    /// The episode, when it is the one open now.
    fn opened(&self, number: &str) -> Option<&Episode> {
        self.current
            .as_ref()
            .filter(|episode| episode.stage == Stage::Open && episode.number == number)
    }

    /// Marks the open episode `number` as ending, for the first request that
    /// asks. A request reads the number before it waits for the slot, so the
    /// episode it names may be over by now: it must not take the one that
    /// opened in between.
    fn claim(&mut self, number: &str) -> bool {
        let Some(episode) = self
            .current
            .as_mut()
            .filter(|episode| episode.stage == Stage::Open && episode.number == number)
        else {
            return false;
        };
        episode.stage = Stage::Closing;
        true
    }

    /// The open episode a session's `handoff` call started.
    fn handoff(&self, session: &str, tool_call_id: &str) -> Option<&Episode> {
        self.current.as_ref().filter(|episode| {
            episode.stage == Stage::Open
                && episode.session == session
                && episode.tool_call_id.as_deref() == Some(tool_call_id)
        })
    }

    fn settle(&mut self, record: BrowserEpisodeRecord) {
        self.settled.push_back(record);
        while self.settled.len() > SETTLED_EPISODES {
            self.settled.pop_front();
        }
    }

    /// The episode is over: the record goes away, and with it user control.
    fn finish(&mut self, end: EpisodeEnd) {
        let Some(episode) = self.current.take() else {
            return;
        };
        let record = BrowserEpisodeRecord {
            episode: episode.number,
            reason: end.reason().to_owned(),
            session: episode.session,
            channel: episode.channel,
            chat_id: episode.chat_id,
            reply_route: episode.reply_route,
            tool_call_id: episode.tool_call_id,
            paused: episode.paused,
        };
        let waits = episode.paused
            && !episode.call_closed
            && !matches!(end, EpisodeEnd::Answered | EpisodeEnd::Cancelled);
        match record.tool_call_id.clone().filter(|_| waits) {
            Some(tool_call_id) => {
                self.outbox.push(BrowserPendingResume {
                    episode: record.episode.clone(),
                    reason: record.reason.clone(),
                    session_id: record.session.clone(),
                    tool_call_id,
                    channel: record.channel.clone(),
                    chat_id: record.chat_id.clone(),
                    reply_route: record.reply_route.clone(),
                });
                self.waiting.push(record);
            }
            None => self.settle(record),
        }
    }

    fn waits(&self, number: &str) -> bool {
        self.waiting.iter().any(|record| record.episode == number)
    }

    /// Nothing waits any more for the records `closed` picks: they become
    /// diagnostics and their continuations are not sent.
    fn close_waiting(&mut self, closed: impl Fn(&BrowserEpisodeRecord) -> bool) {
        let (done, waiting) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition::<Vec<_>, _>(|record| closed(record));
        self.waiting = waiting;
        for record in done {
            self.outbox
                .retain(|resume| resume.episode != record.episode);
            self.settle(record);
        }
    }
}

/// Counts a takeover that waits for the slot. While one waits, a new action
/// is answered busy so the takeover runs right after the action in flight.
struct Reservation<'a>(&'a AtomicUsize);

impl<'a> Reservation<'a> {
    fn new(count: &'a AtomicUsize) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Self(count)
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Why the switch to the input viewer stopped.
enum SwitchFailure {
    /// An old viewer process could not be confirmed gone in time.
    Unconfirmed,
    Failed(String),
}

type ResumeNotifier = Arc<dyn Fn(&BrowserPendingResume) -> bool + Send + Sync>;

/// The session that owns the browser, readable without the slot.
struct Occupant {
    session: String,
    waiting_since: Option<u64>,
}

struct Slot {
    /// Listens only while an episode is open. It is declared first so that
    /// it closes before the processes it leads to.
    paste: Option<PasteListener>,
    generation: Option<Generation>,
    number: u64,
    receipt: Option<ProcessExecutionReceipt>,
    owner: Option<BrowserOwnerBinding>,
    /// The control revision every action binding carries. It goes up when
    /// the user takes the browser and when it comes back, and never down,
    /// whatever generation runs.
    control: u64,
}

pub struct BrowserManager {
    me: Weak<BrowserManager>,
    slot: Mutex<Slot>,
    status: Mutex<BrowserStatus>,
    startup: Box<dyn BrowserStartup>,
    runner: Box<dyn BrowserProcessRunner>,
    clock: Arc<dyn BrowserClock>,
    snapshot: Option<BrowserSnapshotDestination>,
    viewer: Option<Box<dyn BrowserLifecycleFactory>>,
    cancellation: BrowserCancellation,
    notices: BrowserNotices,
    deferred: Mutex<Deferred>,
    occupant: Mutex<Option<Occupant>>,
    episodes: Mutex<Episodes>,
    /// Takeovers that wait for the slot.
    reserved: AtomicUsize,
    /// `Slot::control` as last written, readable while the slot is held:
    /// a viewer switch runs under the slot and is observed from outside it.
    revision: AtomicU64,
    /// Hands an ended episode's continuation to the runtime that owns the
    /// bus. A run without one sends none.
    notifier: Mutex<Option<ResumeNotifier>>,
    locks: Mutex<LockWatch>,
}

#[derive(Clone)]
pub struct ManagedBrowserHost {
    manager: Arc<BrowserManager>,
}

impl ManagedBrowserHost {
    pub fn new(
        startup: Box<dyn BrowserStartup>,
        runner: Box<dyn BrowserProcessRunner>,
        clock: Arc<dyn BrowserClock>,
        snapshot: Option<BrowserSnapshotDestination>,
        viewer: Option<Box<dyn BrowserLifecycleFactory>>,
    ) -> Self {
        Self::with_owner_mode(startup, runner, clock, snapshot, viewer, None)
    }

    /// `owner_mode` is the mode of the runtime owner this process is, if any.
    /// It keeps the session markers of a gateway, an API server and a one-off
    /// run apart.
    pub fn with_owner_mode(
        startup: Box<dyn BrowserStartup>,
        runner: Box<dyn BrowserProcessRunner>,
        clock: Arc<dyn BrowserClock>,
        snapshot: Option<BrowserSnapshotDestination>,
        viewer: Option<Box<dyn BrowserLifecycleFactory>>,
        owner_mode: Option<&str>,
    ) -> Self {
        let view = viewer
            .as_ref()
            .map_or_else(BrowserViewStatus::disabled, |factory| {
                factory.view(runner.as_ref())
            });
        let config = startup.config();
        let status = BrowserStatus::new(config.enable, runner.available(), clock.now_ms())
            .with_view(view)
            .with_reply_wait(config.reply_wait_minutes)
            .with_takeover(config.takeover_minutes);
        let episodes = Episodes::new(clock.now_ms());
        let notices = BrowserNotices::new(
            snapshot
                .as_ref()
                .map(|destination| destination.data_dir.clone()),
            match owner_mode {
                Some("gateway") => "gateway",
                Some("serve") => "api",
                _ => "local",
            },
            clock.now_ms(),
        );
        let manager = Arc::new_cyclic(|me| BrowserManager {
            me: me.clone(),
            slot: Mutex::new(Slot {
                paste: None,
                generation: None,
                number: 0,
                receipt: None,
                owner: None,
                control: 0,
            }),
            status: Mutex::new(status),
            startup,
            runner,
            clock,
            snapshot,
            viewer,
            cancellation: BrowserCancellation::default(),
            notices,
            deferred: Mutex::new(Deferred::default()),
            occupant: Mutex::new(None),
            episodes: Mutex::new(episodes),
            reserved: AtomicUsize::new(0),
            revision: AtomicU64::new(0),
            notifier: Mutex::new(None),
            locks: Mutex::new(LockWatch::default()),
        });
        let weak = Arc::downgrade(&manager);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(100));
            let Some(manager) = weak.upgrade() else {
                return;
            };
            let _ = manager.maintenance();
        });
        Self { manager }
    }

    pub fn status(&self) -> Result<BrowserStatus, BrowserError> {
        self.manager
            .status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| BrowserError::StateUnavailable)
    }

    pub fn receipt(&self) -> Result<Option<ProcessExecutionReceipt>, BrowserError> {
        self.manager
            .slot
            .lock()
            .map(|slot| slot.receipt.clone())
            .map_err(|_| BrowserError::StateUnavailable)
    }

    pub fn maintenance(&self) -> Result<(), BrowserError> {
        self.manager.maintenance()
    }

    pub fn shutdown(&self) -> Result<(), BrowserError> {
        // Signal before taking the slot: an in-flight pipe write/read owns it.
        // The transport observes cancellation within its bounded poll interval.
        self.manager.cancellation.cancel();
        self.manager.cancel_watch()?;
        let mut slot = self
            .manager
            .slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        let noticed = self
            .manager
            .close_with_notice(&mut slot, BrowserError::Closed);
        self.manager
            .publish(&slot, BrowserState::Idle, None)
            .and(noticed)
    }

    /// How many sessions have a closure they were not told about yet, as
    /// this process remembers them.
    pub fn pending_closure_notices(&self) -> Result<usize, BrowserError> {
        self.manager.notices.pending()
    }

    /// Installs what hands an ended episode's continuation to the session
    /// that paused on it. It returns whether the continuation was taken; one
    /// that was not is offered again.
    pub fn set_resume_notifier(
        &self,
        notifier: impl Fn(&BrowserPendingResume) -> bool + Send + Sync + 'static,
    ) {
        if let Ok(mut installed) = self.manager.notifier.lock() {
            *installed = Some(Arc::new(notifier));
        }
        let _ = self.manager.flush();
    }

    /// The episode records, for diagnostics and tests. They are never
    /// written to the status file.
    pub fn episode_records(&self) -> BrowserEpisodeRecords {
        let episodes = self.manager.episodes();
        BrowserEpisodeRecords {
            open: episodes.current.as_ref().map(|episode| BrowserOpenEpisode {
                episode: episode.number.clone(),
                label: episode.label.clone(),
                stage: episode.stage.name(),
                input_spawned: episode.input_spawned,
                paused: episode.paused,
                asked: episode.asked,
                notice_ids: episode.notice_ids.clone(),
            }),
            waiting: episodes.waiting.clone(),
            settled: episodes.settled.iter().cloned().collect(),
        }
    }

    /// The control revision action bindings carry now. It never waits for
    /// the slot, so it can be read while a viewer switch runs.
    pub fn control_revision(&self) -> u64 {
        self.manager.revision.load(Ordering::Acquire)
    }
}

impl BrowserManager {
    /// The episode records, or `StateUnavailable` once a panic left their
    /// lock poisoned: who controls the browser is never read through it.
    /// Everything a `Drop` reaches comes this way.
    fn try_episodes(&self) -> Result<MutexGuard<'_, Episodes>, BrowserError> {
        self.episodes
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)
    }

    /// The episode records where no error can be answered. Only bookkeeping
    /// runs under the lock, so a poisoned one stops the caller.
    fn episodes(&self) -> MutexGuard<'_, Episodes> {
        self.episodes
            .lock()
            .expect("browser episodes lock poisoned")
    }

    /// The user controls the browser exactly while an episode record exists.
    fn user_control(&self) -> Result<bool, BrowserError> {
        Ok(self.try_episodes()?.current.is_some())
    }

    /// Whether `episode` is the open one.
    fn episode_open(&self, episode: &str) -> Result<bool, BrowserError> {
        Ok(self.try_episodes()?.opened(episode).is_some())
    }

    /// The next control revision: every binding made before it is void.
    fn revise(&self, slot: &mut Slot) -> u64 {
        slot.control += 1;
        self.revision.store(slot.control, Ordering::Release);
        slot.control
    }

    /// Refuses everything the assistant does while the user is in control,
    /// before anything else is looked at.
    fn assistant(&self) -> Result<(), BrowserError> {
        if self.watch_active()? {
            return Err(BrowserError::Busy);
        }
        if self.user_control()? {
            return Err(BrowserError::UserControl);
        }
        Ok(())
    }

    /// The slot for an action of the assistant. It never waits: a slot in
    /// use is `browser_busy`, and so is a slot a takeover is waiting for,
    /// so that the takeover runs right after the action in flight.
    fn hold(&self, mark: bool) -> Result<MutexGuard<'_, Slot>, BrowserError> {
        let contended = || {
            if mark {
                self.busy()
            } else {
                BrowserError::Busy
            }
        };
        self.assistant()?;
        if self.reserved.load(Ordering::Acquire) > 0 {
            return Err(contended());
        }
        match self.slot.try_lock() {
            Ok(slot) => {
                // An episode opens only under the slot, so this answer holds
                // for as long as the slot is held.
                self.assistant()?;
                if self.reserved.load(Ordering::Acquire) > 0 {
                    return Err(contended());
                }
                Ok(slot)
            }
            Err(TryLockError::WouldBlock) => {
                self.assistant()?;
                Err(contended())
            }
            Err(TryLockError::Poisoned(_)) => Err(BrowserError::StateUnavailable),
        }
    }

    fn publish(
        &self,
        slot: &Slot,
        state: BrowserState,
        reason: Option<String>,
    ) -> Result<(), BrowserError> {
        let waiting_since = slot
            .generation
            .as_ref()
            .and_then(|generation| generation.waiting_since);
        let (control, takeover_deadline_ms) = match self.try_episodes()?.current.as_ref() {
            Some(episode) => (
                BrowserControl::User,
                (episode.stage == Stage::Open).then_some(episode.deadline_ms),
            ),
            None => (BrowserControl::Assistant, None),
        };
        *self
            .occupant
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)? =
            slot.generation.as_ref().map(|generation| Occupant {
                session: generation.session.clone(),
                waiting_since,
            });
        let mut status = self
            .status
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        status.state = state;
        status.reason = reason;
        status.updated_at_ms = self.clock.now_ms();
        // The browser's own timers stand still while the user is in control.
        status.reply_wait_deadline_ms = waiting_since
            .filter(|_| control == BrowserControl::Assistant)
            .map(|since| since + self.reply_wait_ms());
        status.control = control;
        status.takeover_deadline_ms = takeover_deadline_ms;
        status.owner_observed = slot.owner.is_some();
        status.view = self.view(slot, state);
        if let (Some(destination), Some(owner)) = (&self.snapshot, &slot.owner) {
            browser_state::write_browser_snapshot(
                &destination.data_dir,
                &BrowserStateSnapshot {
                    owner: owner.clone(),
                    generation: slot.number,
                    holder_present: slot.generation.is_some() || state == BrowserState::Starting,
                    status: status.clone(),
                },
            )
            .map_err(|_| BrowserError::StateUnavailable)?;
        }
        Ok(())
    }

    /// The viewer facts of the live generation, or of the configuration when
    /// none runs. The viewer starts inside the same startup closure as the
    /// browser, so a usable viewer is `starting` while the browser is.
    fn view(&self, slot: &Slot, state: BrowserState) -> BrowserViewStatus {
        let live = slot
            .generation
            .as_ref()
            .and_then(|generation| generation.processes.view());
        let mut view = live.unwrap_or_else(|| {
            self.viewer
                .as_ref()
                .map_or_else(BrowserViewStatus::disabled, |factory| {
                    factory.view(self.runner.as_ref())
                })
        });
        if state == BrowserState::Starting
            && slot.generation.is_none()
            && view.state == BrowserViewState::Closed
        {
            view.state = BrowserViewState::Starting;
        }
        view
    }

    fn busy(&self) -> BrowserError {
        if let Ok(mut status) = self.status.lock() {
            status.state = BrowserState::Busy;
            status.reason = Some("browser_busy".to_owned());
            status.updated_at_ms = self.clock.now_ms();
        }
        BrowserError::Busy
    }

    fn maintenance(&self) -> Result<(), BrowserError> {
        let watched = self.watch_locks();
        let result = self.tick();
        let flushed = self.flush();
        watched.and(result).and(flushed)
    }

    /// The lock files a starting generation takes before any process runs.
    /// A manager without a data folder has none, and cannot keep a login.
    fn lock_files(
        &self,
        number: u64,
        profile: ProfileUse,
    ) -> Result<Option<BrowserLockHold>, BrowserError> {
        let Some(destination) = self.snapshot.as_ref() else {
            return match profile {
                ProfileUse::Temporary => Ok(None),
                ProfileUse::Saved => Err(BrowserError::StateUnavailable),
            };
        };
        let hold = browser_lock::acquire(
            &BrowserFolder::new(&destination.data_dir),
            profile,
            self.runner.as_ref(),
            self.clock.as_ref(),
        )?;
        *self
            .locks
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)? = LockWatch {
            watched: Some((number, hold.watched())),
            checked_ms: self.clock.now_ms(),
            replaced: None,
        };
        Ok(Some(hold))
    }

    fn unwatch(&self, number: u64) {
        if let Ok(mut locks) = self.locks.lock() {
            locks.watched.take_if(|(watched, _)| *watched == number);
        }
    }

    /// What the status says once generation `number` was closed from outside.
    fn closed_as(&self, number: u64) -> (BrowserState, Option<String>) {
        let replaced = self
            .locks
            .lock()
            .is_ok_and(|locks| locks.replaced == Some(number));
        if replaced {
            (BrowserState::Failed, Some(LOCK_REPLACED.to_owned()))
        } else {
            (BrowserState::Idle, None)
        }
    }

    /// Once a second: closes the browser when `browser forget` asks for it,
    /// or when a lock path no longer leads to the file this generation
    /// holds. A browser whose slot is held is closed by the holder, exactly
    /// like a close the user asked for.
    fn watch_locks(&self) -> Result<(), BrowserError> {
        let now = self.clock.now_ms();
        let (number, watched) = {
            let mut locks = self
                .locks
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?;
            let Some((number, watched)) = locks.watched.clone() else {
                return Ok(());
            };
            if now.saturating_sub(locks.checked_ms) < LOCK_CHECK_MS {
                return Ok(());
            }
            locks.checked_ms = now;
            (number, watched)
        };
        let signal = watched.check();
        if signal == LockSignal::Keep {
            return Ok(());
        }
        if signal == LockSignal::Replaced {
            self.locks
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?
                .replaced = Some(number);
        }
        let mut slot = match self.slot.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => {
                let mut deferred = self
                    .deferred
                    .lock()
                    .map_err(|_| BrowserError::StateUnavailable)?;
                if let Some(live) = deferred.live.as_mut().filter(|live| live.number == number) {
                    live.close = true;
                    if let Some(cancellation) = &live.watch_cancellation {
                        cancellation.cancel();
                    }
                }
                return Ok(());
            }
            Err(TryLockError::Poisoned(_)) => return Err(BrowserError::StateUnavailable),
        };
        if slot.number != number || slot.generation.is_none() {
            return Ok(());
        }
        let noticed = self.close_with_notice(&mut slot, BrowserError::Closed);
        let (state, reason) = self.closed_as(number);
        self.publish(&slot, state, reason).and(noticed)
    }

    /// Lets a Chromium on the saved profile end by itself before the kill
    /// that follows (scope 4): it is asked once and given five seconds. One
    /// that already ended, or cannot be asked, goes straight to the kill.
    fn quit(&self, generation: &mut Generation) {
        let saved = generation
            .locks
            .as_ref()
            .is_some_and(|locks| locks.profile().is_some());
        if !saved || generation.processes.browser_exited() {
            return;
        }
        let asked = self.clock.now_ms();
        if generation
            .document
            .quit(Instant::now() + Duration::from_millis(QUIT_LIMIT_MS))
            .is_err()
        {
            return;
        }
        let mut waited = self.clock.now_ms().saturating_sub(asked);
        while waited < QUIT_LIMIT_MS && !generation.processes.browser_exited() {
            self.clock.pause(QUIT_POLL_MS);
            waited += QUIT_POLL_MS;
        }
    }

    /// Hands the continuations of ended episodes to the notifier, outside
    /// every lock. One the notifier did not take is offered again while its
    /// record still waits. Maintenance reports a failure; every other caller
    /// already has its one answer.
    fn flush(&self) -> Result<(), BrowserError> {
        if self.cancellation.check().is_err() {
            return Ok(());
        }
        let Some(notifier) = self
            .notifier
            .lock()
            .ok()
            .and_then(|notifier| notifier.clone())
        else {
            return Ok(());
        };
        let pending = std::mem::take(&mut self.try_episodes()?.outbox);
        for resume in pending {
            if !self.try_episodes()?.waits(&resume.episode) {
                continue;
            }
            if !notifier(&resume) {
                let mut episodes = self.try_episodes()?;
                if episodes.waits(&resume.episode) {
                    episodes.outbox.push(resume);
                }
            }
        }
        Ok(())
    }

    /// What maintenance does while the user is in control: the document is
    /// still guarded and the processes are still watched, but neither the
    /// idle timeout nor the reply wait counts. Only the takeover limit and a
    /// viewer that died end the episode.
    fn watch(&self, slot: &mut Slot, number: &str, deadline_ms: u64) -> Result<(), BrowserError> {
        let Some(generation) = slot.generation.as_mut() else {
            self.try_episodes()?.finish(EpisodeEnd::Closed);
            return self.publish(slot, BrowserState::Idle, None);
        };
        let failure = match generation.processes.core_alive() {
            Ok(true) => generation
                .document
                .poll(Instant::now() + Duration::from_secs(1))
                .err(),
            Ok(false) => Some(BrowserError::ProcessExited),
            Err(error) => Some(error),
        };
        if let Some(error) = failure {
            self.discard(slot);
            return self.publish(slot, BrowserState::Failed, Some(error.to_string()));
        }
        if generation.processes.viewer_exited() {
            self.end_episode(slot, number, EpisodeEnd::ViewFailed);
        } else if self.clock.now_ms() >= deadline_ms {
            self.end_episode(slot, number, EpisodeEnd::Limit);
        }
        Ok(())
    }

    fn tick(&self) -> Result<(), BrowserError> {
        let mut slot = match self.slot.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(BrowserError::StateUnavailable),
        };
        self.settle(&mut slot)?;
        let taken = match self.try_episodes() {
            Ok(episodes) => episodes
                .current
                .as_ref()
                .map(|episode| (episode.number.clone(), episode.deadline_ms)),
            Err(error) => {
                // Nobody can tell who controls this browser any more, so it
                // does not stay.
                self.discard(&mut slot);
                return Err(error);
            }
        };
        if let Some((number, deadline_ms)) = taken {
            return self.watch(&mut slot, &number, deadline_ms);
        }
        let config = self.startup.config();
        let Some(generation) = slot.generation.as_mut() else {
            return Ok(());
        };
        let now = self.clock.now_ms();
        // While the session's user has the next move, only the reply wait
        // counts; the idle timeout returns with their next turn.
        let expired = match generation.waiting_since {
            Some(since) => (now.saturating_sub(since) >= self.reply_wait_ms())
                .then_some(BrowserError::ReplyWaitTimeout),
            None => (now.saturating_sub(generation.last_used)
                >= config.idle_timeout_seconds * 1000)
                .then_some(BrowserError::IdleTimeout),
        };
        let failure = expired.or_else(|| match generation.processes.alive() {
            Ok(true) => generation
                .document
                .poll(Instant::now() + Duration::from_secs(1))
                .err(),
            Ok(false) => Some(BrowserError::ProcessExited),
            Err(error) => Some(error),
        });
        if let Some(error) = failure {
            let noticed = if error == BrowserError::ReplyWaitTimeout {
                self.close_with_notice(&mut slot, error)
            } else {
                self.discard(&mut slot);
                Ok(())
            };
            self.publish(&slot, BrowserState::Failed, Some(error.to_string()))?;
            noticed?;
        } else if generation.processes.viewer_failed {
            self.publish_or_dispose(
                &mut slot,
                BrowserState::Running,
                Some("viewer_process_exited".to_owned()),
            )?;
        }
        Ok(())
    }

    fn with_generation<T>(
        &self,
        owner: &str,
        context: &BrowserCallContext,
        operation: impl FnOnce(&mut CdpDocument, Instant) -> Result<T, BrowserError>,
    ) -> Result<T, BrowserError> {
        if context.effective_session_id != owner {
            return Err(BrowserError::ContextUnavailable);
        }
        self.cancellation.check()?;
        let mut slot = self.hold(true)?;
        self.run(&mut slot, owner, operation)
    }

    /// One operation on the owner's generation, with what was deferred
    /// applied before it and again after it: a close asked for while the
    /// operation held the slot runs as soon as the operation is done.
    fn run<T>(
        &self,
        slot: &mut Slot,
        owner: &str,
        operation: impl FnOnce(&mut CdpDocument, Instant) -> Result<T, BrowserError>,
    ) -> Result<T, BrowserError> {
        self.settle(slot)?;
        let result = self.operate(slot, owner, operation);
        let _ = self.settle(slot);
        result
    }

    fn operate<T>(
        &self,
        slot: &mut Slot,
        owner: &str,
        operation: impl FnOnce(&mut CdpDocument, Instant) -> Result<T, BrowserError>,
    ) -> Result<T, BrowserError> {
        let Some(generation) = slot.generation.as_mut() else {
            // A close that was waiting for the slot may just have run.
            return Err(self
                .notices
                .remembered(owner)?
                .map_or(BrowserError::ProcessExited, |notice| notice.error));
        };
        if generation.session != owner {
            return Err(self.busy());
        }
        generation.last_used = self.clock.now_ms();
        let start = generation.last_used;
        let timeout = self.startup.config().page_timeout_seconds;
        let result = match generation.processes.alive() {
            Ok(true) => operation(
                &mut generation.document,
                Instant::now() + Duration::from_secs(timeout),
            ),
            Ok(false) => Err(BrowserError::ProcessExited),
            Err(error) => Err(error),
        };
        generation.last_used = self.clock.now_ms();
        let viewer_failed = generation.processes.viewer_failed;
        let result = if generation.last_used.saturating_sub(start) >= timeout * 1000 {
            Err(BrowserError::PageTimeout)
        } else {
            result
        };
        let result = self.cancellation.check().and(result);
        let result = self.dispose_on_fatal(slot, result)?;
        self.publish_or_dispose(
            slot,
            BrowserState::Running,
            viewer_failed.then(|| "viewer_process_exited".to_owned()),
        )?;
        Ok(result)
    }

    fn reply_wait_ms(&self) -> u64 {
        self.startup.config().reply_wait_minutes * 60_000
    }

    /// End the generation with nothing to tell its session: the assistant's
    /// own close, the idle timeout and every browser error.
    fn discard(&self, slot: &mut Slot) {
        let _ = self.end(slot, None);
    }

    /// End the generation and leave `error` for its session's next call.
    fn close_with_notice(&self, slot: &mut Slot, error: BrowserError) -> Result<(), BrowserError> {
        self.end(slot, Some(error))
    }

    /// Make generation `number`, open or starting, the one a close can be
    /// asked of.
    fn admit(&self, number: u64, session: &str) -> Result<(), BrowserError> {
        let mut deferred = self
            .deferred
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        deferred.live = Some(Live {
            number,
            session: session.to_owned(),
            close: false,
            watch_cancellation: None,
        });
        Ok(())
    }

    fn watch_active(&self) -> Result<bool, BrowserError> {
        Ok(self
            .deferred
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .live
            .as_ref()
            .is_some_and(|live| live.watch_cancellation.is_some()))
    }

    fn cancel_watch(&self) -> Result<(), BrowserError> {
        let mut deferred = self
            .deferred
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        if let Some(live) = deferred.live.as_mut() {
            if let Some(cancellation) = &live.watch_cancellation {
                live.close = true;
                cancellation.cancel();
            }
        }
        Ok(())
    }

    /// Stop accepting closes for generation `number` and say whether one
    /// was accepted. Every close after this is answered `NothingOpen`.
    fn retire(&self, number: u64) -> bool {
        let Ok(mut deferred) = self.deferred.lock() else {
            return false;
        };
        deferred
            .live
            .take_if(|live| live.number == number)
            .is_some_and(|live| live.close)
    }

    /// The one way a generation ends. A close that was accepted for it is
    /// told to its session as `Closed` whatever else ended it; otherwise the
    /// session is told `told`, or nothing.
    fn end(&self, slot: &mut Slot, told: Option<BrowserError>) -> Result<(), BrowserError> {
        let watch = self.watch_active()?;
        let asked = !watch && self.retire(slot.number);
        if let Some(paste) = slot.paste.take() {
            paste.browser_closed();
        }
        let Some(mut generation) = slot.generation.take() else {
            if watch {
                self.unwatch(slot.number);
                self.retire(slot.number);
            }
            return Ok(());
        };
        if generation.purpose == GenerationPurpose::Watch {
            // Keep occupancy and filesystem locks through every child, proxy
            // and temporary directory destructor. Watches leave no session notice.
            let saved = generation
                .locks
                .as_ref()
                .is_some_and(|locks| locks.profile().is_some());
            if saved {
                self.quit(&mut generation);
            } else {
                generation
                    .document
                    .close(Instant::now() + Duration::from_secs(1));
            }
            drop(generation);
            self.unwatch(slot.number);
            self.retire(slot.number);
            return Ok(());
        }
        let told = if asked {
            Some(BrowserError::Closed)
        } else {
            told
        };
        let recorded = self.record_end(&generation.session, generation.marker.take(), told);
        self.unwatch(slot.number);
        self.quit(&mut generation);
        // The processes, the input viewer among them, are gone before the
        // episode is: control never returns to the assistant while a viewer
        // that takes input may still run. The control revision already went
        // up when the episode opened.
        drop(generation);
        let finished = self
            .try_episodes()
            .map(|mut episodes| episodes.finish(EpisodeEnd::Closed));
        recorded.and(finished)
    }

    /// Leave `told` for the session of a generation that ended, or no
    /// marker when there is nothing to tell.
    fn record_end(
        &self,
        session: &str,
        marker: Option<SessionMarker>,
        told: Option<BrowserError>,
    ) -> Result<(), BrowserError> {
        let Some(error) = told else {
            if let Some(marker) = marker {
                let _ = marker.remove();
            }
            return Ok(());
        };
        if let Some(marker) = marker {
            // A marker that could not be rewritten still says `open`, which a
            // restarted process reads as a closed browser.
            let _ = marker.leave(browser_notices::marker_state(error), self.clock.now_ms());
        }
        self.notices.leave(session, error)
    }

    /// Apply what arrived while the slot was held: the turn edges of the
    /// owning session, then a close that had to wait.
    fn settle(&self, slot: &mut Slot) -> Result<(), BrowserError> {
        let (turns, close) = {
            let mut deferred = self
                .deferred
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?;
            let close = deferred
                .live
                .as_ref()
                .is_some_and(|live| live.number == slot.number && live.close);
            (std::mem::take(&mut deferred.turns), close)
        };
        let Some(generation) = slot.generation.as_mut() else {
            return Ok(());
        };
        // While the user is in control the edges move no timer; whether the
        // session's turn runs is read when control comes back.
        let user = self.user_control()?;
        let mut marked = false;
        for mark in &turns {
            if user || mark.session != generation.session {
                continue;
            }
            marked = true;
            match mark.edge {
                TurnEdge::Started => {
                    generation.waiting_since = None;
                    generation.last_used = generation.last_used.max(mark.at_ms);
                }
                TurnEdge::Finished => generation.waiting_since = Some(mark.at_ms),
            }
        }
        if close {
            let noticed = self.close_with_notice(slot, BrowserError::Closed);
            let (state, reason) = self.closed_as(slot.number);
            return self.publish(slot, state, reason).and(noticed);
        }
        if marked {
            let (state, reason) = {
                let status = self
                    .status
                    .lock()
                    .map_err(|_| BrowserError::StateUnavailable)?;
                (status.state, status.reason.clone())
            };
            self.publish_or_dispose(slot, state, reason)?;
        }
        Ok(())
    }

    fn turn(&self, session: &str, edge: TurnEdge) {
        let mark = TurnMark {
            session: session.to_owned(),
            edge,
            at_ms: self.clock.now_ms(),
        };
        match self.deferred.lock() {
            Ok(mut deferred) => {
                match edge {
                    TurnEdge::Started => deferred.running.insert(session.to_owned()),
                    TurnEdge::Finished => deferred.running.remove(session),
                };
                deferred.turns.push(mark);
            }
            Err(_) => return,
        }
        if let Ok(mut slot) = self.slot.try_lock() {
            let _ = self.settle(&mut slot);
        }
    }

    /// Close now when the slot is free. When it is held, the request is
    /// accepted only for a generation whose end has not begun, and whoever
    /// holds the slot carries it out. `session` limits the close to that
    /// session's browser.
    fn close(&self, session: Option<&str>) -> BrowserCloseOutcome {
        let outcome = self.close_now(session);
        // A browser closed under the user ends the episode, and a paused
        // call may wait for that.
        let _ = self.flush();
        outcome
    }

    fn close_now(&self, session: Option<&str>) -> BrowserCloseOutcome {
        let mut slot = match self.slot.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::WouldBlock) => {
                let Ok(mut deferred) = self.deferred.lock() else {
                    return BrowserCloseOutcome::NothingOpen;
                };
                let Some(live) = deferred
                    .live
                    .as_mut()
                    .filter(|live| session.is_none_or(|session| live.session == session))
                else {
                    return BrowserCloseOutcome::NothingOpen;
                };
                live.close = true;
                if let Some(cancellation) = &live.watch_cancellation {
                    cancellation.cancel();
                }
                return BrowserCloseOutcome::Scheduled;
            }
            Err(TryLockError::Poisoned(_)) => return BrowserCloseOutcome::NothingOpen,
        };
        let owned = slot
            .generation
            .as_ref()
            .is_some_and(|generation| session.is_none_or(|session| generation.session == session));
        if !owned {
            return BrowserCloseOutcome::NothingOpen;
        }
        let _ = self.close_with_notice(&mut slot, BrowserError::Closed);
        let (state, reason) = self.closed_as(slot.number);
        let _ = self.publish(&slot, state, reason);
        BrowserCloseOutcome::Closed
    }

    /// What stands in the way of `session` when the slot holds no generation
    /// of it: its own closure notice first, then another session's browser.
    fn without_generation(&self, slot: &Slot, session: &str) -> Result<(), BrowserError> {
        if let Some(notice) = self.notices.stored(session)? {
            return Err(notice.error);
        }
        if slot.generation.is_some() {
            return Err(BrowserError::Busy);
        }
        Ok(())
    }

    /// A notice this process already knows of stands before everything else,
    /// a browser another session is using included.
    fn refuse_noticed(&self, session: &str) -> Result<(), BrowserError> {
        match self.notices.remembered(session)? {
            Some(notice) => Err(notice.error),
            None => Ok(()),
        }
    }

    /// Record a state. A browser whose state cannot be recorded is not kept:
    /// the generation ends and the in-memory status (which the file write
    /// cannot undo) says why.
    fn publish_or_dispose(
        &self,
        slot: &mut Slot,
        state: BrowserState,
        reason: Option<String>,
    ) -> Result<(), BrowserError> {
        if let Err(error) = self.publish(slot, state, reason) {
            self.discard(slot);
            let _ = self.publish(slot, BrowserState::Failed, Some(error.to_string()));
            return Err(error);
        }
        Ok(())
    }

    /// The one outcome rule for every operation on a generation: a stale
    /// approval, a covered target or a refused input keep the browser; any
    /// other error ends the generation at once (processes, proxy, profile)
    /// and records the failure.
    fn dispose_on_fatal<T>(
        &self,
        slot: &mut Slot,
        result: Result<T, BrowserError>,
    ) -> Result<T, BrowserError> {
        if let Err(error) = &result {
            if super::recoverable(error) {
                self.publish_or_dispose(slot, BrowserState::Running, None)?;
            } else {
                // The generation is gone either way; the original error is the
                // one worth reporting even when its record cannot be written.
                self.discard(slot);
                let _ = self.publish(slot, BrowserState::Failed, Some(error.to_string()));
            }
        }
        result
    }

    /// Starts a generation for `session` in an empty slot. It is the one
    /// start path: a session's first action and a takeover of a closed
    /// browser both come through it.
    fn start(
        &self,
        slot: &mut Slot,
        session: &str,
        purpose: GenerationPurpose,
        overall_deadline: Option<Instant>,
        watch_profile: Option<ProfileUse>,
    ) -> Result<(), BrowserError> {
        let config = self.startup.config();
        // A browser whose session cannot be recorded is not started.
        let marker = match purpose {
            GenerationPurpose::Conversation => self.notices.claim(session, self.clock.now_ms())?,
            GenerationPurpose::Watch => None,
        };
        if purpose == GenerationPurpose::Conversation {
            slot.number += 1;
        }
        let mut starting = Starting {
            manager: self,
            number: slot.number,
            session,
            marker,
            started: false,
            purpose,
        };
        let cancellation = match purpose {
            GenerationPurpose::Conversation => {
                self.admit(slot.number, session)?;
                self.cancellation.clone()
            }
            GenerationPurpose::Watch => self
                .deferred
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?
                .live
                .as_ref()
                .filter(|live| live.number == slot.number)
                .and_then(|live| live.watch_cancellation.clone())
                .ok_or(BrowserError::StateUnavailable)?,
        };
        self.cancellation.check()?;
        cancellation.check()?;
        slot.owner = self.snapshot.as_ref().and_then(|destination| {
            destination
                .owner
                .clone()
                .or_else(|| browser_state::current_owner(&destination.data_dir))
                .filter(|owner| {
                    std::path::Path::new(&owner.config_path) == destination.config_path
                        && std::path::Path::new(&owner.workspace) == destination.workspace
                })
        });
        let input = match self.startup.gate(slot.number) {
            Ok(input) => input,
            Err(error) => {
                self.publish(slot, BrowserState::Failed, Some(error.to_string()))?;
                return Err(
                    if purpose == GenerationPurpose::Watch && error == BrowserError::Unavailable {
                        BrowserError::StartDenied
                    } else {
                        error
                    },
                );
            }
        };
        let expected = input.envelope.clone();
        self.publish(slot, BrowserState::Starting, None)?;
        let started_at = self.clock.now_ms();
        let startup_deadline = Instant::now() + Duration::from_secs(config.startup_timeout_seconds);
        let deadline =
            overall_deadline.map_or(startup_deadline, |overall| overall.min(startup_deadline));
        let mut result = Err(BrowserError::StartDenied);
        // When the browser core (Xvfb, Chromium, DevTools) finished. The
        // optional viewer starts after this inside the same approved
        // closure with its own budget; its time is not browser time.
        let mut core_finished_ms = None;
        let number = slot.number;
        // The value read here is the one this generation runs with until it
        // closes.
        let profile = watch_profile.unwrap_or(if config.persist_login {
            ProfileUse::Saved
        } else {
            ProfileUse::Temporary
        });
        let receipt = ProcessGate::new()
            .evaluate_and_maybe_spawn(input, |_| {
                // The locks come before any process and outlive every one of
                // them: a start that fails below drops its processes inside
                // the inner closure, and the locks after it.
                result = self.lock_files(number, profile).and_then(|locks| {
                    self.cancellation.check()?;
                    cancellation.check()?;
                    let saved = locks.as_ref().and_then(BrowserLockHold::profile);
                    BrowserProcesses::start_on(
                        self.runner.as_ref(),
                        match purpose {
                            GenerationPurpose::Conversation => self.startup.network_guard(),
                            GenerationPurpose::Watch => crate::web_watch_profile::web_watch_guard(),
                        },
                        saved,
                        deadline,
                        &cancellation,
                    )
                    .and_then(|(mut processes, mut peer)| {
                        self.cancellation.check()?;
                        cancellation.check()?;
                        peer.set_cancellation(cancellation.clone());
                        let document = match purpose {
                            GenerationPurpose::Conversation => {
                                CdpDocument::initialize(peer, deadline)
                            }
                            GenerationPurpose::Watch => {
                                CdpDocument::initialize_rendered(peer, deadline)
                            }
                        }
                        .map_err(|error| {
                            if error == BrowserError::PageTimeout {
                                BrowserError::StartupTimeout
                            } else {
                                error
                            }
                        })?;
                        core_finished_ms = Some(self.clock.now_ms());
                        cancellation.check()?;
                        let viewer = self.viewer.as_ref().map(|factory| factory.create());
                        let viewer_deadline = Instant::now() + browser_viewer::VIEWER_START_LIMIT;
                        processes.start_viewer(
                            self.runner.as_ref(),
                            viewer,
                            overall_deadline
                                .map_or(viewer_deadline, |overall| overall.min(viewer_deadline)),
                        );
                        cancellation.check()?;
                        Ok((processes, document))
                    })
                    .map(|(processes, document)| Generation {
                        processes,
                        document,
                        session: session.to_owned(),
                        last_used: self.clock.now_ms(),
                        waiting_since: None,
                        purpose,
                        marker: None,
                        locks,
                    })
                });
                ProcessSpawnReport::terminal(if result.is_ok() {
                    ProcessTerminalOutcome::Succeeded
                } else {
                    ProcessTerminalOutcome::Failed
                })
            })
            .map_err(|_| BrowserError::StartDenied)?;
        let valid = receipt.adapter == expected.adapter
            && receipt.identity == expected.identity
            && receipt.policy_safety_snapshot_ref == expected.policy_safety_snapshot_ref
            && receipt.dispatch_count == 1
            && receipt.terminal_outcome == ProcessTerminalOutcome::Succeeded;
        slot.receipt = Some(receipt);
        let core_elapsed_ms = core_finished_ms
            .unwrap_or_else(|| self.clock.now_ms())
            .saturating_sub(started_at);
        if core_elapsed_ms >= config.startup_timeout_seconds * 1000 {
            result = Err(BrowserError::StartupTimeout);
        }
        result = self
            .cancellation
            .check()
            .and(cancellation.check())
            .and(result);
        match result {
            Ok(mut generation) if valid => {
                generation.marker = starting.marker.take();
                generation.document.set_control(slot.control);
                starting.started = true;
                slot.generation = Some(generation);
            }
            Ok(_) => return Err(BrowserError::StartDenied),
            Err(error) => {
                self.publish(slot, BrowserState::Failed, Some(error.to_string()))?;
                return Err(error);
            }
        }
        self.publish_or_dispose(slot, BrowserState::Running, None)?;
        Ok(())
    }

    /// Hands the browser to the user (Spec 050 PRD 010 scope 3).
    fn takeover(
        &self,
        request: &BrowserTakeoverRequest,
    ) -> Result<BrowserTakeover, BrowserTakeoverError> {
        if request.session_id.is_empty() {
            return Err(BrowserTakeoverError::Browser(
                BrowserError::ContextUnavailable,
            ));
        }
        self.cancellation
            .check()
            .map_err(BrowserTakeoverError::Browser)?;
        let result = {
            let mut slot = self.reserve()?;
            let result = self.take(&mut slot, request);
            let _ = self.settle(&mut slot);
            result
        };
        let _ = self.flush();
        result
    }

    /// The slot for a takeover. An action in flight is waited for, and while
    /// this waits every new action is answered busy, so the takeover is the
    /// next thing to run. The wait ends after the page timeout and five
    /// seconds on the browser clock.
    fn reserve(&self) -> Result<MutexGuard<'_, Slot>, BrowserTakeoverError> {
        let _reservation = {
            let deferred = self
                .deferred
                .lock()
                .map_err(|_| BrowserTakeoverError::Browser(BrowserError::StateUnavailable))?;
            if deferred
                .live
                .as_ref()
                .is_some_and(|live| live.watch_cancellation.is_some())
            {
                return Err(BrowserTakeoverError::Browser(BrowserError::Busy));
            }
            Reservation::new(&self.reserved)
        };
        let started = self.clock.now_ms();
        let limit = (self.startup.config().page_timeout_seconds + 5) * 1000;
        loop {
            if self.user_control().map_err(BrowserTakeoverError::Browser)? {
                return Err(BrowserTakeoverError::AlreadyTaken);
            }
            match self.slot.try_lock() {
                Ok(slot) => return Ok(slot),
                Err(TryLockError::Poisoned(_)) => {
                    return Err(BrowserTakeoverError::Browser(
                        BrowserError::StateUnavailable,
                    ))
                }
                Err(TryLockError::WouldBlock) => {}
            }
            self.cancellation
                .check()
                .map_err(BrowserTakeoverError::Browser)?;
            if self.clock.now_ms().saturating_sub(started) >= limit {
                return Err(BrowserTakeoverError::Browser(BrowserError::Busy));
            }
            self.clock.pause(SLOT_POLL_MS);
        }
    }

    fn take(
        &self,
        slot: &mut Slot,
        request: &BrowserTakeoverRequest,
    ) -> Result<BrowserTakeover, BrowserTakeoverError> {
        let browser = BrowserTakeoverError::Browser;
        self.settle(slot).map_err(browser)?;
        self.cancellation.check().map_err(browser)?;
        if self.user_control().map_err(browser)? {
            return Err(BrowserTakeoverError::AlreadyTaken);
        }
        let session = request.session_id.as_str();
        // `handoff` is the assistant's call and is told what any other of its
        // calls would be. `/browser takeover` is the user's: it takes the
        // browser whichever session uses it.
        let handoff = request.tool_call_id.is_some();
        if handoff {
            self.refuse_noticed(session).map_err(browser)?;
        }
        let config = self.startup.config();
        let mut started = false;
        match slot.generation.as_ref() {
            Some(generation) => {
                if handoff && generation.session != session {
                    self.without_generation(slot, session).map_err(browser)?;
                }
            }
            None => {
                // Reading the stored notice keeps it for the session's next
                // call even though the marker is claimed again below.
                let stored = self.notices.stored(session).map_err(browser)?;
                if let Some(notice) = stored.filter(|_| handoff) {
                    return Err(browser(notice.error));
                }
                if !config.enable {
                    return Err(browser(BrowserError::Disabled));
                }
                if !self.runner.available() {
                    return Err(browser(BrowserError::Unavailable));
                }
                // A viewer that cannot open is known before anything starts.
                let view = self
                    .viewer
                    .as_ref()
                    .map_or_else(BrowserViewStatus::disabled, |factory| {
                        factory.view(self.runner.as_ref())
                    });
                if view.state != BrowserViewState::Closed {
                    return Err(BrowserTakeoverError::ViewUnavailable(view_reason(&view)));
                }
                self.start(slot, session, GenerationPurpose::Conversation, None, None)
                    .map_err(browser)?;
                started = true;
            }
        }
        let alive = slot
            .generation
            .as_mut()
            .map_or(Ok(false), |generation| generation.processes.alive())
            .and_then(|alive| {
                if alive {
                    Ok(())
                } else {
                    Err(BrowserError::ProcessExited)
                }
            });
        self.dispose_on_fatal(slot, alive).map_err(browser)?;
        let view = self.view(slot, BrowserState::Running);
        let open = (view.state == BrowserViewState::Open)
            .then(|| view.url.clone())
            .flatten();
        if open.is_none() {
            if started {
                // Refusing changes nothing: the browser this request started
                // goes with it.
                self.discard(slot);
                self.publish(slot, BrowserState::Idle, None)
                    .map_err(browser)?;
            } else {
                let failed = slot
                    .generation
                    .as_ref()
                    .is_some_and(|generation| generation.processes.viewer_failed);
                self.publish_or_dispose(
                    slot,
                    BrowserState::Running,
                    failed.then(|| "viewer_process_exited".to_owned()),
                )
                .map_err(browser)?;
            }
            return Err(BrowserTakeoverError::ViewUnavailable(view_reason(&view)));
        }

        // Step 1: from here every action of the assistant is refused.
        let limit = config.takeover_minutes * 60_000;
        let (number, label) = self
            .try_episodes()
            .map_err(browser)?
            .open(request, self.clock.now_ms() + limit);
        let control = self.revise(slot);
        if let Some(generation) = slot.generation.as_mut() {
            generation.document.set_control(control);
        }
        self.publish_or_dispose(slot, BrowserState::Running, None)
            .map_err(browser)?;
        let switched = self.guarded(slot, |slot| self.switch(slot, &number, &label));
        let view_url = match switched {
            Ok(view_url) => view_url,
            Err(SwitchFailure::Unconfirmed) => {
                // Nobody knows whether the old viewer is gone, so nothing may
                // be started next to it: the generation ends (scope 4 step 4).
                let _ = self.close_with_notice(slot, BrowserError::Closed);
                let _ = self.publish(slot, BrowserState::Idle, None);
                return Err(BrowserTakeoverError::Failed(
                    "input_shutdown_unconfirmed".to_owned(),
                ));
            }
            Err(SwitchFailure::Failed(reason)) => {
                self.give_back(slot, EpisodeEnd::Failed);
                return Err(BrowserTakeoverError::Failed(reason));
            }
        };

        // Step 9: the call pauses, and the limit counts, only from here.
        let opened = self.try_episodes().map(|mut episodes| {
            if let Some(episode) = episodes.current.as_mut() {
                episode.stage = Stage::Open;
                episode.deadline_ms = self.clock.now_ms() + limit;
                episode.paused = episode.tool_call_id.is_some();
            }
        });
        if let Err(error) = opened.and_then(|()| self.publish(slot, BrowserState::Running, None)) {
            // The caller gets this error as its one result, so no
            // continuation may follow it.
            if let Ok(mut episodes) = self.try_episodes() {
                if let Some(episode) = episodes.current.as_mut() {
                    episode.call_closed = true;
                }
            }
            self.discard(slot);
            let _ = self.publish(slot, BrowserState::Failed, Some(error.to_string()));
            return Err(browser(error));
        }
        Ok(BrowserTakeover {
            episode: number,
            label,
            view_url,
            minutes: config.takeover_minutes,
        })
    }

    /// Steps 2 to 8 of a takeover: the view-only viewer goes, and only once
    /// it is confirmed gone does one that takes input start. The viewer port
    /// opens last.
    fn switch(&self, slot: &mut Slot, number: &str, label: &str) -> Result<String, SwitchFailure> {
        let failed = |reason: &str| SwitchFailure::Failed(reason.to_owned());
        let runner = self.runner.as_ref();
        let clock = self.clock.as_ref();
        let wait = ViewerWait {
            clock,
            until_ms: clock.now_ms() + browser_viewer::VIEWER_STOP_LIMIT_MS,
        };
        let generation = slot
            .generation
            .as_mut()
            .ok_or_else(|| failed("browser_closed"))?;
        let (viewer, environment) = generation
            .processes
            .viewer_parts()
            .ok_or_else(|| failed("view_unavailable"))?;
        viewer
            .routes(environment, ViewerRoutes::Empty)
            .map_err(|_| failed("view_routes_unavailable"))?;
        if !viewer.stop_websockify(runner, &wait) || !viewer.stop_screen(runner, &wait) {
            return Err(SwitchFailure::Unconfirmed);
        }
        if let Some(episode) = self
            .try_episodes()
            .map_err(|_| failed("state_unavailable"))?
            .current
            .as_mut()
        {
            episode.input_spawned = true;
        }
        viewer
            .start_screen(environment, runner, true, clock)
            .map_err(SwitchFailure::Failed)?;
        let secret = viewer
            .password()
            .ok_or_else(|| failed("view_password_unavailable"))?
            .to_owned();
        let target: Weak<dyn PasteTarget> = self.me.clone();
        let listener = PasteListener::open(
            environment.root().join(browser_viewer::PASTE_SOCKET),
            number,
            label,
            &secret,
            Arc::clone(&self.clock),
            target,
        )
        .map_err(SwitchFailure::Failed)?;
        slot.paste = Some(listener);
        viewer
            .routes(environment, ViewerRoutes::ViewAndPaste)
            .map_err(|_| failed("view_routes_unavailable"))?;
        viewer
            .start_websockify(environment, runner, clock)
            .map_err(SwitchFailure::Failed)?;
        generation
            .processes
            .view()
            .filter(|view| view.state == BrowserViewState::Open)
            .and_then(|view| view.url)
            .ok_or_else(|| failed("view_unavailable"))
    }

    /// Runs a viewer switch. One that unwinds must not leave a viewer that
    /// takes input behind a slot nobody can lock again, so the generation
    /// ends with it.
    fn guarded<T>(&self, slot: &mut Slot, switch: impl FnOnce(&mut Slot) -> T) -> T {
        struct Unwound<'a> {
            manager: &'a BrowserManager,
            slot: &'a mut Slot,
        }
        impl Drop for Unwound<'_> {
            fn drop(&mut self) {
                if thread::panicking() {
                    let _ = self.manager.end(self.slot, Some(BrowserError::Closed));
                }
            }
        }
        let guard = Unwound {
            manager: self,
            slot,
        };
        switch(&mut *guard.slot)
    }

    /// Ends episode `number` when it is the open one. Whoever moves the stage
    /// from `open` to `closing` under the slot is the one that ends it, so
    /// an episode ends once and a request for another episode changes
    /// nothing.
    fn end_episode(&self, slot: &mut Slot, number: &str, end: EpisodeEnd) -> bool {
        let claimed = self.episodes().claim(number);
        if !claimed {
            return false;
        }
        self.give_back(slot, end);
        true
    }

    /// The one way input goes off and control comes back (scope 4). Control
    /// returns to the assistant last, and never over a browser whose input
    /// viewer is not confirmed gone.
    fn give_back(&self, slot: &mut Slot, end: EpisodeEnd) {
        self.guarded(slot, |slot| self.give_back_now(slot, end));
    }

    fn give_back_now(&self, slot: &mut Slot, end: EpisodeEnd) {
        if !self.input_off(slot) {
            let _ = self.close_with_notice(slot, BrowserError::Closed);
            let _ = self.publish(slot, BrowserState::Idle, None);
            return;
        }
        let control = self.revise(slot);
        let Some(generation) = slot.generation.as_mut() else {
            // No browser is left to close when the record cannot be read.
            let _ = self
                .try_episodes()
                .map(|mut episodes| episodes.finish(EpisodeEnd::Closed));
            return;
        };
        self.restore_view(generation, end);
        let timeout = self.startup.config().page_timeout_seconds;
        generation
            .document
            .resume_control(control, Instant::now() + Duration::from_secs(timeout));
        // The edges that came while the user was in control moved no timer;
        // what they add up to is read here, in the lock they arrive under.
        let running = self.deferred.lock().is_ok_and(|mut deferred| {
            deferred
                .turns
                .retain(|mark| mark.session != generation.session);
            deferred.running.contains(&generation.session)
        });
        let now = self.clock.now_ms();
        generation.last_used = now;
        generation.waiting_since = (!running).then_some(now);
        let viewer_failed = generation.processes.viewer_failed;
        let finished = self.try_episodes().map(|mut episodes| episodes.finish(end));
        if finished.is_err() {
            // Control is not handed back through a record nobody can read:
            // the browser goes instead.
            let _ = self.close_with_notice(slot, BrowserError::Closed);
            return;
        }
        let _ = self.publish_or_dispose(
            slot,
            BrowserState::Running,
            viewer_failed.then(|| "viewer_process_exited".to_owned()),
        );
    }

    /// Steps 1 to 3 of scope 4 under one two-second limit. False means some
    /// part could not be confirmed gone.
    fn input_off(&self, slot: &mut Slot) -> bool {
        let runner = self.runner.as_ref();
        let wait = ViewerWait {
            clock: self.clock.as_ref(),
            until_ms: self.clock.now_ms() + browser_viewer::VIEWER_STOP_LIMIT_MS,
        };
        let Some(generation) = slot.generation.as_mut() else {
            slot.paste = None;
            return true;
        };
        let Some((viewer, environment)) = generation.processes.viewer_parts() else {
            slot.paste = None;
            return true;
        };
        let _ = viewer.routes(environment, ViewerRoutes::Empty);
        let unreachable = viewer.stop_websockify(runner, &wait);
        slot.paste = None;
        // The screen viewer is not touched before the way in is confirmed
        // gone: a late websockify worker must find nothing to connect to.
        unreachable && viewer.stop_screen(runner, &wait)
    }

    /// Step 5 of scope 4: the view-only viewer comes back. A failure costs
    /// the viewer, never the browser.
    fn restore_view(&self, generation: &mut Generation, end: EpisodeEnd) {
        let runner = self.runner.as_ref();
        let clock = self.clock.as_ref();
        let Some((viewer, environment)) = generation.processes.viewer_parts() else {
            return;
        };
        let restored = if end == EpisodeEnd::ViewFailed {
            Err("process_exited".to_owned())
        } else {
            viewer
                .start_screen(environment, runner, false, clock)
                .and_then(|()| {
                    viewer
                        .routes(environment, ViewerRoutes::View)
                        .map_err(|_| "startup_failed".to_owned())
                })
                .and_then(|()| viewer.start_websockify(environment, runner, clock))
        };
        if let Err(reason) = restored {
            viewer.fail(environment, &reason);
            generation.processes.viewer_failed = true;
        }
    }

    /// Ends the episode under a slot that is waited for. It answers whether
    /// this call ended it.
    fn finish(&self, number: &str, end: EpisodeEnd) -> bool {
        let ended = match self.slot.lock() {
            Ok(mut slot) => {
                let ended = self.end_episode(&mut slot, number, end);
                let _ = self.settle(&mut slot);
                ended
            }
            Err(_) => false,
        };
        let _ = self.flush();
        ended
    }

    fn finish_handoff(&self, session: &str, tool_call_id: &str, number: &str) -> bool {
        let closes = |episode: &Episode| {
            episode.number == number
                && episode.stage == Stage::Open
                && episode.session == session
                && episode.tool_call_id.as_deref() == Some(tool_call_id)
        };
        // The answer closes the paused call itself, so nothing may wait to
        // continue it whatever ends the episode from here on.
        {
            let mut episodes = self.episodes();
            let Some(episode) = episodes.current.as_mut().filter(|episode| closes(episode)) else {
                return false;
            };
            episode.call_closed = true;
        }
        self.finish(number, EpisodeEnd::Answered)
    }

    fn return_takeover(&self, target: &BrowserReturnTarget) -> BrowserReturnOutcome {
        let number = {
            let episodes = self.episodes();
            let open = episodes
                .current
                .as_ref()
                .filter(|episode| episode.stage == Stage::Open);
            match target {
                BrowserReturnTarget::Label(label) => match open {
                    Some(episode) if &episode.label == label => episode.number.clone(),
                    _ => return BrowserReturnOutcome::Over,
                },
                BrowserReturnTarget::Arrival {
                    channel,
                    chat_id,
                    reply_to: Some(reply_to),
                    ..
                } => match open {
                    Some(episode)
                        if &episode.channel == channel
                            && &episode.chat_id == chat_id
                            && episode.notice_ids.contains(reply_to) =>
                    {
                        episode.number.clone()
                    }
                    _ => return BrowserReturnOutcome::ReplyMismatch,
                },
                BrowserReturnTarget::Arrival {
                    reply_to: None,
                    arrival,
                    ..
                } => {
                    let Some(arrived) = arrival.episode.as_ref() else {
                        return BrowserReturnOutcome::NotTaken;
                    };
                    match open {
                        Some(episode) if &episode.number == arrived => {
                            if !arrival.asked {
                                return BrowserReturnOutcome::NotAsked;
                            }
                            episode.number.clone()
                        }
                        _ => return BrowserReturnOutcome::Over,
                    }
                }
            }
        };
        if self.finish(&number, EpisodeEnd::Returned) {
            BrowserReturnOutcome::Returned
        } else {
            BrowserReturnOutcome::Over
        }
    }

    fn cancel_takeover(&self, session: &str) {
        let number = {
            let mut episodes = self.episodes();
            episodes.close_waiting(|record| record.session == session);
            episodes
                .current
                .as_mut()
                .filter(|episode| episode.session == session)
                .map(|episode| {
                    episode.call_closed = true;
                    episode.number.clone()
                })
        };
        if let Some(number) = number {
            self.finish(&number, EpisodeEnd::Cancelled);
        }
    }

    fn handoff_closed(&self, session: &str, tool_call_id: &str) {
        let mut episodes = self.episodes();
        episodes.close_waiting(|record| {
            record.session == session && record.tool_call_id.as_deref() == Some(tool_call_id)
        });
        if let Some(episode) = episodes.current.as_mut().filter(|episode| {
            episode.session == session && episode.tool_call_id.as_deref() == Some(tool_call_id)
        }) {
            episode.call_closed = true;
        }
    }
}

impl RenderedWebReader for ManagedBrowserHost {
    fn enabled(&self) -> bool {
        self.manager.startup.config().enable
    }

    fn saved_login_enabled(&self) -> bool {
        self.manager.startup.config().persist_login
    }

    fn read(
        &self,
        request: &RenderedWebRequest<'_>,
    ) -> Result<RenderedWebPage, RenderedWebFailure> {
        let url = request.url;
        let limits = request.limits;
        let manager = &self.manager;
        manager.cancellation.check().map_err(watch_start_failure)?;
        // No maintenance here: a watch may never expire somebody else's browser.
        let (mut slot, cancellation) = {
            // Close and takeover use this same admission lock. Holding it
            // across the nonblocking slot attempt publishes occupancy atomically,
            // before even startup availability can run or block. Never wait for
            // the slot here: ordinary holders take deferred in the other order.
            let mut deferred = manager
                .deferred
                .lock()
                .map_err(|_| watch_start_failure(BrowserError::StateUnavailable))?;
            if deferred.live.is_some() || manager.reserved.load(Ordering::Acquire) > 0 {
                return Err(watch_start_failure(BrowserError::Busy));
            }
            let mut slot = match manager.slot.try_lock() {
                Ok(slot) => slot,
                Err(TryLockError::WouldBlock) => {
                    return Err(watch_start_failure(BrowserError::Busy))
                }
                Err(TryLockError::Poisoned(_)) => {
                    return Err(watch_start_failure(BrowserError::StateUnavailable))
                }
            };
            if slot.generation.is_some() {
                return Err(watch_start_failure(BrowserError::Busy));
            }
            slot.number += 1;
            let cancellation = BrowserCancellation::default();
            deferred.live = Some(Live {
                number: slot.number,
                session: String::new(),
                close: false,
                watch_cancellation: Some(cancellation.clone()),
            });
            (slot, cancellation)
        };
        let started = manager.clock.now_ms();
        let deadline = Instant::now() + Duration::from_secs(60);
        // The slot guard stays outside the existing panic boundary, so cleanup
        // finishes before it is released and a caught panic cannot poison it.
        let result = crate::isolate_panic(|| {
            if !manager.startup.config().enable {
                return Err(watch_start_failure(BrowserError::Disabled));
            }
            if request.use_saved_login && !manager.startup.config().persist_login {
                return Err(RenderedWebFailure {
                    failure: WebCheckFailure::BrowserUnavailable,
                    browser_reason: Some(BrowserUnavailableReason::PersistLoginDisabled),
                    response: None,
                });
            }
            if !manager.runner.available() {
                return Err(watch_start_failure(BrowserError::Unavailable));
            }
            manager
                .start(
                    &mut slot,
                    "",
                    GenerationPurpose::Watch,
                    Some(deadline),
                    Some(if request.use_saved_login {
                        ProfileUse::Saved
                    } else {
                        ProfileUse::Temporary
                    }),
                )
                .map_err(watch_start_failure)?;
            let remaining_ms =
                60_000u64.saturating_sub(manager.clock.now_ms().saturating_sub(started));
            if remaining_ms == 0 || Instant::now() >= deadline {
                return Err(watch_start_failure(BrowserError::StartupTimeout));
            }
            let generation = slot
                .generation
                .as_mut()
                .ok_or_else(|| watch_start_failure(BrowserError::Closed))?;
            let bounded = WebFetchLimits {
                timeout: limits
                    .timeout
                    .min(Duration::from_secs(20))
                    .min(Duration::from_millis(remaining_ms)),
                max_bytes: limits.max_bytes.min(1024 * 1024),
            };
            let read_deadline = deadline.min(Instant::now() + bounded.timeout);
            generation
                .document
                .read_rendered(url, &bounded, read_deadline, manager.clock.as_ref())
        });
        let cleanup = manager.end(&mut slot, None);
        let (state, reason) = manager.closed_as(slot.number);
        let published = manager.publish(&slot, state, reason);
        drop(slot);
        let result = match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        cleanup.and(published).map_err(watch_start_failure)?;
        manager.cancellation.check().map_err(watch_start_failure)?;
        cancellation.check().map_err(watch_start_failure)?;
        result
    }
}

fn watch_start_failure(error: BrowserError) -> RenderedWebFailure {
    let (failure, reason) = match error {
        BrowserError::Busy | BrowserError::UserControl => (WebCheckFailure::BrowserBusy, None),
        BrowserError::Disabled => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::Disabled),
        ),
        BrowserError::StartDenied => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::PermissionDenied),
        ),
        BrowserError::Cancelled | BrowserError::Closed => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::Closed),
        ),
        BrowserError::ProfileInUse => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::ProfileInUse),
        ),
        BrowserError::ProfileUnreadable => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::ProfileUnreadable),
        ),
        BrowserError::ProfileLocked => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::ProfileLocked),
        ),
        _ => (
            WebCheckFailure::BrowserUnavailable,
            Some(BrowserUnavailableReason::StartupFailed),
        ),
    };
    RenderedWebFailure {
        failure,
        browser_reason: reason,
        response: None,
    }
}

impl BrowserHost for ManagedBrowserHost {
    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        if session_id.is_empty() {
            return Err(BrowserError::ContextUnavailable);
        }
        // While the user is in control nothing else is looked at, a notice
        // included (Spec 050 PRD 010 scope 1).
        self.manager.assistant()?;
        // Showing a notice never removes it: the classifier and the
        // diagnostics prepare the same call too (Spec 050 PRD 000 scope 6).
        self.manager.refuse_noticed(session_id)?;
        let slot = self.manager.hold(false)?;
        if let Some(generation) = slot
            .generation
            .as_ref()
            .filter(|generation| generation.session == session_id)
        {
            return generation.document.prepare(arguments);
        }
        self.manager.without_generation(&slot, session_id)?;
        if arguments["action"] == "capture" {
            return Err(BrowserError::NotOpen);
        }
        browser_action_binding::empty(arguments, slot.control)
    }

    fn close_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<(), BrowserError> {
        let arguments = serde_json::json!({"action":"close"});
        self.manager.assistant()?;
        self.manager.refuse_noticed(&context.effective_session_id)?;
        let mut slot = self.manager.hold(false)?;
        if !slot
            .generation
            .as_ref()
            .is_some_and(|generation| generation.session == context.effective_session_id)
        {
            self.manager
                .without_generation(&slot, &context.effective_session_id)?;
        }
        match slot.generation.as_mut() {
            Some(generation) => {
                let deadline = Instant::now()
                    + Duration::from_secs(self.manager.startup.config().page_timeout_seconds);
                let result = generation
                    .document
                    .execute_prepared(&arguments, digest, false, deadline)
                    .map(|_| ());
                // A fatal error while closing disposes of the generation like
                // any other action would; a stale approval keeps it.
                self.manager.dispose_on_fatal(&mut slot, result)?;
            }
            None => {
                if browser_action_binding::empty(&arguments, slot.control)?.digest != digest {
                    return Err(BrowserError::StaleRevision);
                }
            }
        }
        self.manager.discard(&mut slot);
        self.manager.publish(&slot, BrowserState::Idle, None)
    }

    fn capture_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<BrowserCapture, BrowserError> {
        let session = context.effective_session_id.as_str();
        if session.is_empty() {
            return Err(BrowserError::ContextUnavailable);
        }
        self.manager.cancellation.check()?;
        self.manager.assistant()?;
        self.manager.refuse_noticed(session)?;
        let mut slot = self.manager.hold(true)?;
        // A capture never starts a browser: without this session's
        // generation the answer is the one its preparation would get.
        if !slot
            .generation
            .as_ref()
            .is_some_and(|generation| generation.session == session)
        {
            self.manager.without_generation(&slot, session)?;
            return Err(BrowserError::NotOpen);
        }
        let arguments = serde_json::json!({"action":"capture"});
        let read = self.manager.run(&mut slot, session, |document, deadline| {
            document.execute_prepared(&arguments, digest, true, deadline)
        })?;
        drop(slot);
        let read = read.ok_or(BrowserError::CaptureFailed)?;
        let screenshot = read.screenshot.ok_or(BrowserError::CaptureFailed)?;
        BrowserCapture::from_base64(read.document, &screenshot)
    }

    fn turn_started(&self, session_id: &str) {
        self.manager.turn(session_id, TurnEdge::Started);
    }

    fn turn_finished(&self, session_id: &str) {
        self.manager.turn(session_id, TurnEdge::Finished);
    }

    fn closure_notice(&self, session_id: &str) -> Option<BrowserClosureNotice> {
        if let Some(notice) = self.manager.notices.remembered(session_id).ok()? {
            return Some(notice);
        }
        // The marker file is this session's only while no generation of it
        // lives here, which the slot alone can tell.
        let slot = self.manager.slot.try_lock().ok()?;
        if slot
            .generation
            .as_ref()
            .is_some_and(|generation| generation.session == session_id)
        {
            return None;
        }
        self.manager.notices.stored(session_id).ok()?
    }

    fn acknowledge_closure_notice(&self, session_id: &str, token: &str) {
        self.manager.notices.acknowledge(session_id, token);
    }

    fn close_for_session(&self, session_id: &str) -> bool {
        self.manager.close(Some(session_id)) != BrowserCloseOutcome::NothingOpen
    }

    fn close_by_user(&self) -> BrowserCloseOutcome {
        self.manager.close(None)
    }

    fn describe(&self) -> BrowserDescription {
        let config = self.manager.startup.config();
        let now = self.manager.clock.now_ms();
        let view = self.manager.status.lock().map_or_else(
            |_| BrowserViewDescription::Off("unknown".to_owned()),
            |status| {
                let view = &status.view;
                match view.url.clone() {
                    Some(url) if view.state == BrowserViewState::Open => {
                        BrowserViewDescription::Open(url)
                    }
                    _ => BrowserViewDescription::Off(view_reason(view)),
                }
            },
        );
        let taken =
            self.manager
                .episodes()
                .current
                .as_ref()
                .map(|episode| BrowserTakeoverDescription {
                    session: episode.session.clone(),
                    label: episode.label.clone(),
                    remaining_minutes: episode.deadline_ms.saturating_sub(now).div_ceil(60_000),
                });
        let user = taken.is_some();
        let control = BrowserControlDescription {
            takeover: taken,
            view,
        };
        let closed = |state| BrowserDescription {
            state,
            session: None,
            reply_wait_remaining_minutes: None,
            control: control.clone(),
        };
        if !config.enable {
            return closed(BrowserDescriptionState::Disabled);
        }
        if !self.manager.runner.available() {
            return closed(BrowserDescriptionState::Unavailable);
        }
        let Ok(occupant) = self.manager.occupant.lock() else {
            return closed(BrowserDescriptionState::Unavailable);
        };
        let Some(occupant) = occupant.as_ref() else {
            return closed(BrowserDescriptionState::Closed);
        };
        BrowserDescription {
            state: BrowserDescriptionState::Open,
            session: Some(occupant.session.clone()),
            // The reply wait stands still while the user is in control.
            reply_wait_remaining_minutes: occupant.waiting_since.filter(|_| !user).map(|since| {
                (since + self.manager.reply_wait_ms())
                    .saturating_sub(now)
                    .div_ceil(60_000)
            }),
            control: control.clone(),
        }
    }

    fn takeover(
        &self,
        request: &BrowserTakeoverRequest,
    ) -> Result<BrowserTakeover, BrowserTakeoverError> {
        self.manager.takeover(request)
    }

    fn episode_notice_delivered(&self, episode: &str, message_id: Option<&str>) {
        let mut episodes = self.manager.episodes();
        let Some(open) = episodes
            .current
            .as_mut()
            .filter(|open| open.stage == Stage::Open && open.number == episode)
        else {
            return;
        };
        open.asked = true;
        if let Some(id) = message_id.filter(|id| !id.is_empty()) {
            if !open.notice_ids.iter().any(|known| known == id) {
                open.notice_ids.push(id.to_owned());
            }
        }
    }

    fn arrival(&self) -> BrowserArrival {
        let episodes = self.manager.episodes();
        episodes
            .current
            .as_ref()
            .filter(|episode| episode.stage == Stage::Open)
            .map_or_else(BrowserArrival::default, |episode| BrowserArrival {
                episode: Some(episode.number.clone()),
                asked: episode.asked,
            })
    }

    fn handoff_episode(&self, session_id: &str, tool_call_id: &str) -> BrowserHandoffEpisode {
        let episodes = self.manager.episodes();
        episodes
            .handoff(session_id, tool_call_id)
            .map_or(BrowserHandoffEpisode::Over, |episode| {
                BrowserHandoffEpisode::Open {
                    episode: episode.number.clone(),
                    channel: episode.channel.clone(),
                    chat_id: episode.chat_id.clone(),
                    notice_ids: episode.notice_ids.clone(),
                }
            })
    }

    fn finish_handoff(&self, session_id: &str, tool_call_id: &str, episode: &str) -> bool {
        self.manager
            .finish_handoff(session_id, tool_call_id, episode)
    }

    fn return_takeover(&self, target: &BrowserReturnTarget) -> BrowserReturnOutcome {
        self.manager.return_takeover(target)
    }

    fn cancel_takeover(&self, session_id: &str) {
        self.manager.cancel_takeover(session_id);
    }

    fn pending_resume(&self, episode: &str) -> Option<BrowserPendingResume> {
        let episodes = self.manager.episodes();
        let record = episodes
            .waiting
            .iter()
            .find(|record| record.episode == episode)?;
        Some(BrowserPendingResume {
            episode: record.episode.clone(),
            reason: record.reason.clone(),
            session_id: record.session.clone(),
            tool_call_id: record.tool_call_id.clone()?,
            channel: record.channel.clone(),
            chat_id: record.chat_id.clone(),
            reply_route: record.reply_route.clone(),
        })
    }

    fn set_resume_notifier(&self, notifier: BrowserResumeNotifier) {
        ManagedBrowserHost::set_resume_notifier(self, notifier);
    }

    fn resume_settled(&self, episode: &str) {
        self.manager
            .episodes()
            .close_waiting(|record| record.episode == episode);
    }

    fn handoff_closed(&self, session_id: &str, tool_call_id: &str) {
        self.manager.handoff_closed(session_id, tool_call_id);
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        if context.effective_session_id.is_empty() {
            return Err(BrowserError::ContextUnavailable);
        }
        self.manager.cancellation.check()?;
        self.manager.assistant()?;
        self.manager.maintenance()?;
        self.manager.refuse_noticed(&context.effective_session_id)?;
        let mut slot = self.manager.hold(true)?;
        self.manager.cancellation.check()?;
        if let Some(generation) = slot.generation.as_ref() {
            if generation.session != context.effective_session_id {
                if let Some(notice) = self.manager.notices.stored(&context.effective_session_id)? {
                    return Err(notice.error);
                }
                self.manager.publish_or_dispose(
                    &mut slot,
                    BrowserState::Busy,
                    Some("browser_busy".to_owned()),
                )?;
                return Err(BrowserError::Busy);
            }
        } else {
            // A browser that closed behind this session's back is reported
            // before a new one starts.
            if let Some(notice) = self.manager.notices.stored(&context.effective_session_id)? {
                return Err(notice.error);
            }
            let config = self.manager.startup.config();
            if !config.enable {
                return Err(BrowserError::Disabled);
            }
            if !self.manager.runner.available() {
                return Err(BrowserError::Unavailable);
            }
            self.manager.start(
                &mut slot,
                &context.effective_session_id,
                GenerationPurpose::Conversation,
                None,
                None,
            )?;
        }
        if let Some(generation) = slot.generation.as_mut() {
            generation.last_used = self.manager.clock.now_ms();
        }
        Ok(Box::new(ManagedBrowserSession {
            manager: Arc::clone(&self.manager),
            owner: context.effective_session_id.clone(),
        }))
    }
}

/// A browser that is still starting. A start that fails leaves no marker
/// behind, unless a close was accepted while it ran: that close is still
/// told to the session.
struct Starting<'a> {
    manager: &'a BrowserManager,
    number: u64,
    session: &'a str,
    marker: Option<SessionMarker>,
    started: bool,
    purpose: GenerationPurpose,
}
impl Drop for Starting<'_> {
    fn drop(&mut self) {
        if self.started {
            return;
        }
        self.manager.unwatch(self.number);
        let told = self
            .manager
            .retire(self.number)
            .then_some(BrowserError::Closed);
        if self.purpose == GenerationPurpose::Conversation {
            let _ = self
                .manager
                .record_end(self.session, self.marker.take(), told);
        }
    }
}

struct ManagedBrowserSession {
    manager: Arc<BrowserManager>,
    owner: String,
}
impl BrowserSession for ManagedBrowserSession {
    fn execute_prepared(
        &self,
        context: &BrowserCallContext,
        arguments: &Value,
        digest: &str,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        self.manager
            .with_generation(&self.owner, context, |document, deadline| {
                document.execute_prepared(arguments, digest, capture, deadline)
            })
    }

    fn current_document(
        &self,
        context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError> {
        self.manager
            .with_generation(&self.owner, context, CdpDocument::current)
    }
    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError> {
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        self.manager
            .with_generation(&self.owner, context, |document, deadline| {
                document.read(capture, deadline)
            })
    }
    fn act(
        &self,
        context: &BrowserCallContext,
        action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError> {
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        self.manager
            .with_generation(&self.owner, context, |document, deadline| {
                document.act(action, deadline)?;
                document.read(capture, deadline)
            })
    }
    fn close(&self, context: &BrowserCallContext) -> Result<(), BrowserError> {
        if context.effective_session_id != self.owner {
            return Err(BrowserError::ContextUnavailable);
        }
        let mut slot = self.manager.hold(true)?;
        if slot
            .generation
            .as_ref()
            .is_some_and(|generation| generation.session != self.owner)
        {
            return Err(self.manager.busy());
        }
        self.manager.discard(&mut slot);
        self.manager.publish(&slot, BrowserState::Idle, None)
    }
}

/// The 049 view reason shown where the viewer is not open.
fn view_reason(view: &BrowserViewStatus) -> String {
    view.reason.clone().unwrap_or_else(|| {
        match view.state {
            BrowserViewState::Disabled => "disabled",
            BrowserViewState::Unavailable => "unavailable",
            BrowserViewState::Starting => "starting",
            BrowserViewState::Open => "open",
            BrowserViewState::Failed => "failed",
            BrowserViewState::Closed => "closed",
            BrowserViewState::Unknown => "unknown",
        }
        .to_owned()
    })
}

impl PasteTarget for BrowserManager {
    /// A connection thread never waits on the slot: whoever closes the
    /// listener holds the slot while it joins this thread, and `stopping`
    /// is how it says so.
    fn paste_insert(&self, episode: &str, text: &str, stopping: &AtomicBool) -> PasteOutcome {
        loop {
            if stopping.load(Ordering::Acquire) {
                return PasteOutcome::NotUser;
            }
            match self.episode_open(episode) {
                Ok(true) => {}
                Ok(false) => return PasteOutcome::NotUser,
                Err(_) => return PasteOutcome::BrowserClosed,
            }
            let mut slot = match self.slot.try_lock() {
                Ok(slot) => slot,
                Err(TryLockError::WouldBlock) => {
                    thread::sleep(SLOT_POLL);
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => return PasteOutcome::BrowserClosed,
            };
            // The stage changes only under the slot, so this holds until the
            // text is in.
            match self.episode_open(episode) {
                Ok(true) => {}
                Ok(false) => return PasteOutcome::NotUser,
                Err(_) => return PasteOutcome::BrowserClosed,
            }
            let timeout = self.startup.config().page_timeout_seconds;
            let Some(generation) = slot.generation.as_mut() else {
                return PasteOutcome::BrowserClosed;
            };
            let result = generation
                .document
                .insert_pasted(text, Instant::now() + Duration::from_secs(timeout));
            // A tab that is gone takes no text; the browser itself is fine.
            if result == Err(BrowserError::NotOpen) {
                return PasteOutcome::BrowserClosed;
            }
            let result = self.cancellation.check().and(result);
            return match self.dispose_on_fatal(&mut slot, result) {
                Ok(()) => PasteOutcome::Done,
                Err(_) => PasteOutcome::BrowserClosed,
            };
        }
    }

    fn paste_lockout(&self, episode: &str, stopping: &AtomicBool) {
        loop {
            if stopping.load(Ordering::Acquire)
                || !self.episode_open(episode).is_ok_and(|open| open)
            {
                return;
            }
            let mut slot = match self.slot.try_lock() {
                Ok(slot) => slot,
                Err(TryLockError::WouldBlock) => {
                    thread::sleep(SLOT_POLL);
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => return,
            };
            if !self.episode_open(episode).is_ok_and(|open| open) {
                return;
            }
            // The listener and its socket go before the route does, so a
            // late websockify worker finds nothing to connect to.
            slot.paste = None;
            if let Some((viewer, environment)) = slot
                .generation
                .as_mut()
                .and_then(|generation| generation.processes.viewer_parts())
            {
                let _ = viewer.routes(environment, ViewerRoutes::View);
            }
            return;
        }
    }
}

static BROWSER: OnceLock<ManagedBrowserHost> = OnceLock::new();

/// Installs real manager ownership over fixture process/DevTools clients before
/// production tooling is constructed. A separate test process owns this slot.
#[cfg(feature = "spec031-test-fixture")]
#[doc(hidden)]
pub fn install_process_browser_fixture(host: ManagedBrowserHost) -> Result<(), BrowserError> {
    BROWSER.set(host).map_err(|_| BrowserError::Busy)
}

pub fn process_browser_host(bundle: &ConfigBundle) -> ManagedBrowserHost {
    BROWSER
        .get_or_init(|| {
            let snapshot = Some(BrowserSnapshotDestination {
                data_dir: bundle.context.data_dir.clone(),
                owner: None,
                config_path: bundle.context.config_path.clone(),
                workspace: bundle.context.workspace.clone(),
            });
            let owner_mode =
                browser_state::current_marker(&bundle.context.data_dir).map(|marker| marker.mode);
            let viewer = BrowserViewerFactory::new(
                bundle.config.tools.browser.view.clone(),
                browser_viewer::published_viewer_port(owner_mode.as_deref()),
            );
            ManagedBrowserHost::with_owner_mode(
                Box::new(ConfigBrowserStartup::new(bundle.clone())),
                Box::new(LinuxBrowserProcessRunner),
                Arc::new(SystemBrowserClock),
                snapshot,
                Some(Box::new(viewer)),
                owner_mode.as_deref(),
            )
        })
        .clone()
}

pub fn shutdown_process_browser() {
    if let Some(host) = BROWSER.get() {
        let _ = host.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{EpisodeEnd, Episodes, Stage};
    use serde_json::Map;
    use shacs_core::runtime::BrowserTakeoverRequest;

    fn opened(episodes: &mut Episodes) -> String {
        let request = BrowserTakeoverRequest {
            session_id: "cli:owner".to_owned(),
            channel: "cli".to_owned(),
            chat_id: "owner".to_owned(),
            reply_route: Map::new(),
            tool_call_id: None,
        };
        let (number, _) = episodes.open(&request, 0);
        if let Some(episode) = episodes.current.as_mut() {
            episode.stage = Stage::Open;
        }
        number
    }

    // A return, an answer or a cancellation reads the episode number and then
    // waits for the slot. Both it and a new takeover wait on the same lock,
    // so the episode may end and another open before it gets there.
    #[test]
    fn stale_ending_cannot_claim_a_newer_episode() {
        let mut episodes = Episodes::new(7);
        let first = opened(&mut episodes);
        assert!(episodes.claim(&first));
        assert!(!episodes.claim(&first));
        episodes.finish(EpisodeEnd::Limit);
        let second = opened(&mut episodes);

        assert!(!episodes.claim(&first));
        assert!(episodes.opened(&second).is_some());
        assert!(episodes.claim(&second));
        assert!(!episodes.claim(&second));
    }
}
