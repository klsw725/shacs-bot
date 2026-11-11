use crate::runtime::PermissionModeSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCallContext {
    pub effective_session_id: String,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    pub permission_interactive: bool,
    pub browser_screenshots: Option<bool>,
    pub native_image_input_supported: bool,
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
        };
        f.write_str(code)
    }
}

impl Error for BrowserError {}

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
