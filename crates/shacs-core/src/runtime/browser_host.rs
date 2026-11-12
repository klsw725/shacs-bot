use crate::runtime::PermissionModeSnapshot;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

/// Inbound metadata the ingress writes when a message first arrives: the
/// episode open then, whether its notice had been delivered and the message
/// the inbound replies to (Spec 050 PRD 010 scope 8).
pub const BROWSER_ARRIVAL_METADATA: &str = "browser_arrival";
/// Outbound metadata naming the episode a notice belongs to.
pub const BROWSER_EPISODE_NOTICE_METADATA: &str = "browser_episode_notice";
/// `injected_event` of the inbound that continues a paused `handoff` call.
pub const BROWSER_RESUME_EVENT: &str = "browser_takeover_end";
/// Durable work id prefix of that inbound; the episode number follows.
pub const BROWSER_RESUME_WORK_PREFIX: &str = "browser-resume-";
/// Dedupe hint prefix of that inbound; the episode number follows.
pub const BROWSER_RESUME_HINT_PREFIX: &str = "browser.resume:";
/// Inbound metadata that says where in a room a reply goes: a Telegram
/// topic, an email subject, a Discord thread, a Slack thread.
pub const REPLY_ROUTE_METADATA_KEYS: [&str; 5] = [
    "message_thread_id",
    "subject",
    "parent_channel_id",
    "thread_id",
    "slack",
];

/// The part of an inbound's metadata that says where in its room a reply
/// goes.
pub fn reply_route(metadata: &Map<String, Value>) -> Map<String, Value> {
    REPLY_ROUTE_METADATA_KEYS
        .iter()
        .filter_map(|key| Some(((*key).to_owned(), metadata.get(*key)?.clone())))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCallContext {
    pub effective_session_id: String,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    pub permission_interactive: bool,
    pub browser_screenshots: Option<bool>,
    pub native_image_input_supported: bool,
    /// Where `capture` stores its screenshot (Spec 050 PRD 000 scope 3).
    pub media_root: Option<PathBuf>,
}

/// Host-created decision material. Only its digest crosses the execution boundary;
/// the description must pass permission redaction before it is exposed.
#[derive(Clone, PartialEq)]
pub struct BrowserPreparedAction {
    pub description: Value,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserDocument {
    pub target_id: String,
    pub session_id: String,
    pub frame_id: String,
    pub loader_id: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserElement {
    pub element: usize,
    pub role: String,
    pub name: String,
    pub value: String,
}

/// A numbered snapshot released only after the host's document guard succeeds.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserRead {
    pub document: BrowserDocument,
    pub accessibility: Value,
    pub visible_text: String,
    pub title: String,
    pub boxes: Vec<Value>,
    pub screenshot: Option<String>,
    pub revision: String,
    pub elements: Vec<BrowserElement>,
    pub omitted_elements: usize,
    pub notice: Option<String>,
}

/// A screenshot the user asked for: the verified document and its PNG bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserCapture {
    pub document: BrowserDocument,
    pub png: Vec<u8>,
}

impl BrowserCapture {
    /// Decodes the base64 text DevTools returns for a screenshot.
    pub fn from_base64(document: BrowserDocument, encoded: &str) -> Result<Self, BrowserError> {
        let png = STANDARD
            .decode(encoded)
            .map_err(|_| BrowserError::CaptureFailed)?;
        if png.is_empty() {
            return Err(BrowserError::CaptureFailed);
        }
        Ok(Self { document, png })
    }
}

/// A closure the session has not been told about yet. The token names this
/// notice only; it carries no session key and no page content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserClosureNotice {
    pub error: BrowserError,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCloseOutcome {
    Closed,
    Scheduled,
    NothingOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserDescriptionState {
    Open,
    Closed,
    Disabled,
    Unavailable,
}

/// What `/browser status` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserDescription {
    pub state: BrowserDescriptionState,
    pub session: Option<String>,
    /// Minutes left of the reply wait, rounded up.
    pub reply_wait_remaining_minutes: Option<u64>,
    /// Who controls the browser and where the screen viewer is
    /// (Spec 050 PRD 010 scope 11).
    pub control: BrowserControlDescription,
}

impl BrowserDescription {
    /// A description with no browser and no takeover.
    pub fn without_browser(state: BrowserDescriptionState) -> Self {
        Self {
            state,
            session: None,
            reply_wait_remaining_minutes: None,
            control: BrowserControlDescription::default(),
        }
    }
}

/// The takeover part of `/browser status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserControlDescription {
    /// The open takeover, when the user controls the browser.
    pub takeover: Option<BrowserTakeoverDescription>,
    pub view: BrowserViewDescription,
}

/// The screen viewer line of `/browser status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewDescription {
    /// The address the user opens.
    Open(String),
    /// Off, with the viewer's reason.
    Off(String),
}

impl Default for BrowserViewDescription {
    fn default() -> Self {
        Self::Off("disabled".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTakeoverDescription {
    pub session: String,
    pub label: String,
    /// Minutes left before the browser is taken back, rounded up.
    pub remaining_minutes: u64,
}

/// Who asks to hand the browser to the user. `tool_call_id` is set only for
/// the model's `handoff` call; `/browser takeover` has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTakeoverRequest {
    pub session_id: String,
    pub channel: String,
    pub chat_id: String,
    /// Where in the room the turn that asks was answering
    /// ([`reply_route`]); the continued `handoff` turn answers there.
    pub reply_route: Map<String, Value>,
    pub tool_call_id: Option<String>,
}

/// An opened takeover episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTakeover {
    /// `<16 hex process nonce>-<n>`.
    pub episode: String,
    /// The short form shown to the user (`9f2c-3`).
    pub label: String,
    pub view_url: String,
    pub minutes: u64,
}

/// Why the browser was not handed over. No episode stays open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserTakeoverError {
    /// The user already controls the browser.
    AlreadyTaken,
    /// The screen viewer is not open; carries the viewer's reason.
    ViewUnavailable(String),
    /// The switch to the input viewer failed and was undone.
    Failed(String),
    Browser(BrowserError),
}

/// What the browser looked like when a message first reached the runtime:
/// the open episode, and whether its notice had been delivered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserArrival {
    pub episode: Option<String>,
    pub asked: bool,
}

/// The episode a paused `handoff` call opened, as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserHandoffEpisode {
    Open {
        episode: String,
        channel: String,
        chat_id: String,
        /// Channel message ids of the delivered notice, one per chunk.
        notice_ids: Vec<String>,
    },
    /// Ended, or never known to this process.
    Over,
}

/// Which episode `/browser return` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserReturnTarget {
    /// `/browser return <label>`.
    Label(String),
    /// `/browser return`: the reply target when the message is a reply,
    /// otherwise the episode recorded when the message arrived.
    Arrival {
        channel: String,
        chat_id: String,
        reply_to: Option<String>,
        arrival: BrowserArrival,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserReturnOutcome {
    Returned,
    /// A reply that does not point at the open episode's notice.
    ReplyMismatch,
    /// Sent before the open episode's notice was delivered.
    NotAsked,
    /// No episode was open when the message arrived.
    NotTaken,
    /// The named episode is no longer open.
    Over,
}

/// An ended episode whose paused `handoff` call still waits to continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserPendingResume {
    pub episode: String,
    pub reason: String,
    pub session_id: String,
    pub tool_call_id: String,
    pub channel: String,
    pub chat_id: String,
    /// The reply route recorded when the takeover started.
    pub reply_route: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrowserAction {
    Navigate {
        url: String,
    },
    Back,
    Click {
        x: f64,
        y: f64,
    },
    Type {
        text: String,
    },
    Press {
        key: String,
    },
    Scroll {
        x: f64,
        y: f64,
    },
    ClickElement {
        element: usize,
        revision: String,
    },
    TypeElement {
        element: usize,
        revision: String,
        text: String,
        submit: bool,
    },
    ScrollElement {
        element: Option<usize>,
        revision: Option<String>,
        direction: String,
    },
    Wait {
        text: Option<String>,
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserError {
    Disabled,
    Unavailable,
    StartDenied,
    Busy,
    ContextUnavailable,
    StartupTimeout,
    PageTimeout,
    Cancelled,
    IdleTimeout,
    ProcessExited,
    TransportClosed,
    Protocol,
    GeometryUnavailable,
    DocumentChanged,
    FileTransferDenied,
    StateUnavailable,
    StaleRevision,
    TargetCovered,
    InputDenied,
    NotOpen,
    ReplyWaitTimeout,
    Closed,
    CaptureFailed,
    UserControl,
    /// `browser forget` holds the lock files (Spec 050 PRD 020 scope 3).
    ForgetInProgress,
    ProfileUnreadable,
    ProfileInUse,
    ProfileLocked,
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::Busy => "browser_busy: 다른 세션이 브라우저를 쓰는 중입니다",
            Self::DocumentChanged => "browser_document_changed: 페이지를 다시 여세요",
            Self::FileTransferDenied => "browser_file_transfer_denied: 페이지를 다시 여세요",
            Self::ContextUnavailable => "browser_context_unavailable",
            Self::StaleRevision => "browser_stale_revision: 다시 look하세요",
            Self::TargetCovered => "browser_target_covered: 다시 look하세요",
            Self::InputDenied => "browser_input_denied: password/file 입력은 지원하지 않습니다",
            Self::Disabled => "browser_disabled: tools.browser.enable is false",
            Self::Unavailable => "browser_unavailable: official container browser is unavailable",
            Self::StartDenied => "browser_start_denied: 브라우저 시작이 허락되지 않았습니다",
            Self::StartupTimeout => {
                "browser_startup_timeout: 브라우저가 시간 안에 시작되지 않았습니다"
            }
            Self::PageTimeout => {
                "browser_page_timeout: 페이지가 시간 안에 응답하지 않아 브라우저를 닫았습니다. 페이지를 다시 여세요"
            }
            Self::Cancelled => "browser_cancelled: 실행이 중단되었습니다",
            Self::IdleTimeout => {
                "browser_idle_timeout: 오래 쓰지 않아 브라우저를 닫았습니다. 페이지를 다시 여세요"
            }
            Self::ProcessExited => {
                "browser_process_exited: 브라우저가 끝났습니다. 페이지를 다시 여세요"
            }
            Self::TransportClosed => {
                "browser_transport_closed: 브라우저 연결이 끊어졌습니다. 페이지를 다시 여세요"
            }
            Self::Protocol => {
                "browser_protocol: 브라우저가 예상하지 않은 응답을 보냈습니다. 페이지를 다시 여세요"
            }
            Self::GeometryUnavailable => "browser_geometry_unavailable: 다시 look하세요",
            Self::StateUnavailable => "browser_state_unavailable: 브라우저 상태를 읽을 수 없습니다",
            Self::NotOpen => "browser_not_open: 열린 브라우저가 없습니다",
            Self::ReplyWaitTimeout => {
                "browser_reply_wait_timeout: 답을 기다리다 브라우저가 닫혔습니다. 페이지를 다시 여세요"
            }
            Self::Closed => "browser_closed: 브라우저가 닫혔습니다. 페이지를 다시 여세요",
            Self::CaptureFailed => "browser_capture_failed: 화면을 찍지 못했습니다",
            Self::UserControl => {
                "browser_user_control: 사용자가 조작 중입니다. 돌려받을 때까지 기다리세요"
            }
            Self::ForgetInProgress => {
                "browser_forget_in_progress: 로그인 기록을 지우는 중입니다. 잠시 뒤 다시 하세요"
            }
            Self::ProfileUnreadable => {
                "browser_profile_unreadable: 로그인 기록 폴더를 쓸 수 없습니다"
            }
            Self::ProfileInUse => {
                "browser_profile_in_use: 로그인 기록을 다른 곳에서 쓰는 중입니다"
            }
            Self::ProfileLocked => {
                "browser_profile_locked: 로그인 기록이 잠겨 있습니다. docs/USAGE.md의 브라우저 로그인 유지를 보세요"
            }
        };
        f.write_str(code)
    }
}

impl Error for BrowserError {}

/// What a [`BrowserHost`] calls with the continuation of an ended episode.
pub type BrowserResumeNotifier = Box<dyn Fn(&BrowserPendingResume) -> bool + Send + Sync>;

pub trait BrowserHost: Send + Sync {
    fn close_prepared(
        &self,
        _context: &BrowserCallContext,
        _digest: &str,
    ) -> Result<(), BrowserError> {
        Err(BrowserError::ContextUnavailable)
    }

    fn prepare(
        &self,
        _session_id: &str,
        _arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        Err(BrowserError::ContextUnavailable)
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError>;

    /// Takes a fresh screenshot of the document `digest` was prepared for.
    /// Never starts a browser.
    fn capture_prepared(
        &self,
        _context: &BrowserCallContext,
        _digest: &str,
    ) -> Result<BrowserCapture, BrowserError> {
        Err(BrowserError::ContextUnavailable)
    }

    /// A turn started by this session's user began.
    fn turn_started(&self, _session_id: &str) {}

    /// That turn ended, by any path.
    fn turn_finished(&self, _session_id: &str) {}

    /// The session's undelivered closure notice. Reading never removes it.
    fn closure_notice(&self, _session_id: &str) -> Option<BrowserClosureNotice> {
        None
    }

    /// Removes the session's notice once its text has been recorded as a tool
    /// result. A stale or unknown token removes nothing.
    fn acknowledge_closure_notice(&self, _session_id: &str, _token: &str) {}

    /// Closes the browser when this session is using it. Returns whether it
    /// closed or scheduled a close.
    fn close_for_session(&self, _session_id: &str) -> bool {
        false
    }

    /// Closes the process's browser whichever session is using it.
    fn close_by_user(&self) -> BrowserCloseOutcome {
        BrowserCloseOutcome::NothingOpen
    }

    fn describe(&self) -> BrowserDescription {
        BrowserDescription::without_browser(BrowserDescriptionState::Unavailable)
    }

    /// Hands the browser to the user (Spec 050 PRD 010 scope 3). Returns
    /// only once the episode is open or has been undone.
    fn takeover(
        &self,
        _request: &BrowserTakeoverRequest,
    ) -> Result<BrowserTakeover, BrowserTakeoverError> {
        Err(BrowserTakeoverError::Browser(
            BrowserError::ContextUnavailable,
        ))
    }

    /// The episode's notice really left: called once per delivered chunk,
    /// with the channel's message id when the channel gives one. An episode
    /// that is not open now is left unchanged.
    fn episode_notice_delivered(&self, _episode: &str, _message_id: Option<&str>) {}

    /// The value an arriving message is stamped with.
    fn arrival(&self) -> BrowserArrival {
        BrowserArrival::default()
    }

    /// Whether the episode the session's `handoff` call opened is open now.
    fn handoff_episode(&self, _session_id: &str, _tool_call_id: &str) -> BrowserHandoffEpisode {
        BrowserHandoffEpisode::Over
    }

    /// Ends that episode as answered. False when it is no longer the open
    /// episode of that call, in which case nothing changed.
    fn finish_handoff(&self, _session_id: &str, _tool_call_id: &str, _episode: &str) -> bool {
        false
    }

    /// `/browser return`: ends the named episode as returned.
    fn return_takeover(&self, target: &BrowserReturnTarget) -> BrowserReturnOutcome {
        match target {
            BrowserReturnTarget::Label(_) => BrowserReturnOutcome::Over,
            BrowserReturnTarget::Arrival { .. } => BrowserReturnOutcome::NotTaken,
        }
    }

    /// `/stop` and `/new`: ends the session's open episode as cancelled and
    /// forgets its calls that wait to continue.
    fn cancel_takeover(&self, _session_id: &str) {}

    /// The ended episode whose paused call still waits to continue.
    fn pending_resume(&self, _episode: &str) -> Option<BrowserPendingResume> {
        None
    }

    /// Installs what hands an ended episode's continuation to the session
    /// that paused on it. It returns whether the continuation was taken; one
    /// that was not is offered again while its call still waits.
    fn set_resume_notifier(&self, _notifier: BrowserResumeNotifier) {}

    /// The continuation for that episode was recorded in the session.
    fn resume_settled(&self, _episode: &str) {}

    /// The session's paused `handoff` call was closed some other way, so
    /// nothing waits to continue it any more.
    fn handoff_closed(&self, _session_id: &str, _tool_call_id: &str) {}
}

pub trait BrowserSession: Send + Sync {
    fn execute_prepared(
        &self,
        _context: &BrowserCallContext,
        _arguments: &Value,
        _digest: &str,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        Err(BrowserError::ContextUnavailable)
    }

    fn current_document(
        &self,
        context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError>;
    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError>;
    /// Execute and collect its verified result under one page deadline.
    fn act(
        &self,
        context: &BrowserCallContext,
        action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError>;
    fn close(&self, context: &BrowserCallContext) -> Result<(), BrowserError>;
}
