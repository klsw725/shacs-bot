use super::spec049_browser_runtime::{bundle, input as gate_input, Clock, Facts, Runner, Startup};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_cli::browser::{
    BrowserCancellation, BrowserChildKind, BrowserOwnerBinding, BrowserSnapshotDestination,
    BrowserState, BrowserStatus, CdpReply, DevToolsPeer, ManagedBrowserHost, PausedTargetAdmission,
};
use shacs_config::{ConfigBundle, PermissionMode};
use shacs_core::runtime::{
    BrowserCallContext, BrowserCloseOutcome, BrowserControlDescription, BrowserDescription,
    BrowserDescriptionState, BrowserError, BrowserHost, BrowserPreparedAction, BrowserRead,
    BrowserSession, ProcessGateInput,
};
use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_BASE64: &str = "iVBORw0KGgo=";
const IDLE_MS: u64 = 300_000;
const REPLY_WAIT_MS: u64 = 30 * 60_000;
const PAGE_TIMEOUT_MS: u64 = 30_000;
const OWNER: &str = "telegram:chat-owner";
const OTHER: &str = "telegram:chat-other";

struct Page {
    url: String,
    loader: String,
    methods: Vec<String>,
    pending_events: Vec<Value>,
    moves_while_captured: bool,
    during_start: Option<Box<dyn FnOnce() + Send>>,
    during_read: Option<Box<dyn FnOnce() + Send>>,
    /// Runs while the manager takes the browser of this page apart.
    during_teardown: Option<Box<dyn FnOnce() + Send>>,
}

impl Page {
    fn shared() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            url: "https://example.com/".to_owned(),
            loader: "loader-1".to_owned(),
            methods: Vec::new(),
            pending_events: Vec::new(),
            moves_while_captured: false,
            during_start: None,
            during_read: None,
            during_teardown: None,
        }))
    }

    fn navigate(&mut self) {
        self.url = "https://example.com/next".to_owned();
        self.loader = "loader-2".to_owned();
        self.pending_events
            .push(json!({"method":"Page.frameNavigated","params":{"frame":{
            "id":"frame-1","loaderId":self.loader,"url":self.url}}}));
    }
}

fn sent(page: &Arc<Mutex<Page>>, method: &str) -> usize {
    let page = page.lock().expect("page");
    page.methods.iter().filter(|sent| *sent == method).count()
}

struct Screen {
    page: Arc<Mutex<Page>>,
    sequence: u64,
}

impl DevToolsPeer for Screen {
    fn set_cancellation(&mut self, _cancellation: BrowserCancellation) {}

    fn request(
        &mut self,
        method: &str,
        _params: Value,
        _session: Option<&str>,
        _deadline: Instant,
        _admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        self.sequence += 1;
        let mut page = self
            .page
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        page.methods.push(method.to_owned());
        let result = match method {
            "Target.getTargets" => {
                let during_start = page.during_start.take();
                drop(page);
                if let Some(during_start) = during_start {
                    during_start();
                }
                json!({"targetInfos":[{"targetId":"target-1","type":"page"}]})
            }
            "Target.attachToTarget" => json!({"sessionId":"session-1"}),
            "Page.getFrameTree" => {
                json!({"frameTree":{"frame":{"id":"frame-1","loaderId":page.loader,"url":page.url}}})
            }
            "Accessibility.getFullAXTree" => {
                let during_read = page.during_read.take();
                drop(page);
                if let Some(during_read) = during_read {
                    during_read();
                }
                json!({"nodes":[{"backendDOMNodeId":1,"name":{"value":"결제하기"}}]})
            }
            "Runtime.evaluate" => json!({"result":{"value":{"text":"장바구니","title":"Cart"}}}),
            "DOM.getBoxModel" => json!({"model":{"content":[0,0,100,0,100,30,0,30]}}),
            "Page.getLayoutMetrics" => {
                json!({"cssVisualViewport":{"clientWidth":1280,"clientHeight":720}})
            }
            "Page.captureScreenshot" => {
                if page.moves_while_captured {
                    page.loader = "loader-moved".to_owned();
                }
                json!({"data":PNG_BASE64})
            }
            _ => json!({}),
        };
        Ok(CdpReply {
            result,
            sequence: self.sequence,
            document_epoch: 0,
            events: Vec::new(),
        })
    }

    fn drain_events(&mut self) -> Result<Vec<Value>, BrowserError> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        Ok(std::mem::take(&mut page.pending_events))
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let during_teardown = match self.page.lock() {
            Ok(mut page) => page.during_teardown.take(),
            Err(_) => None,
        };
        if let Some(during_teardown) = during_teardown {
            during_teardown();
        }
    }
}

struct Rig {
    host: ManagedBrowserHost,
    facts: Arc<Mutex<Facts>>,
}

impl Rig {
    fn new(
        bundle: &ConfigBundle,
        gate: &ProcessGateInput,
        clock: &Arc<Clock>,
        state_dir: bool,
        page: Option<&Arc<Mutex<Page>>>,
    ) -> Self {
        let mut facts = Facts::default();
        facts.devtools = page.map(|page| {
            Box::new(Screen {
                page: Arc::clone(page),
                sequence: 0,
            }) as Box<dyn DevToolsPeer>
        });
        let facts = Arc::new(Mutex::new(facts));
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                config: bundle.config.tools.browser.clone(),
                input: gate.clone(),
            }),
            Box::new(Runner {
                facts: Arc::clone(&facts),
                chromium_dead: Arc::new(AtomicBool::new(false)),
                xvfb_dead: Arc::new(AtomicBool::new(false)),
                clock: Arc::clone(clock),
                advance_start: 0,
                advance_read: 0,
            }),
            Arc::clone(clock) as Arc<dyn shacs_cli::browser::BrowserClock>,
            state_dir.then(|| BrowserSnapshotDestination {
                data_dir: bundle.context.data_dir.clone(),
                owner: Some(BrowserOwnerBinding {
                    owner_id: "owner".to_owned(),
                    pid: std::process::id(),
                    started_at_ms: 1,
                    config_path: bundle.context.config_path.to_string_lossy().into_owned(),
                    workspace: bundle.context.workspace.to_string_lossy().into_owned(),
                    config_digest: "digest".to_owned(),
                }),
                config_path: bundle.context.config_path.clone(),
                workspace: bundle.context.workspace.clone(),
            }),
            None,
        );
        Self { host, facts }
    }

    fn started(&self) -> usize {
        let facts = self.facts.lock().expect("facts");
        facts
            .spawns
            .iter()
            .filter(|spec| spec.kind == BrowserChildKind::Chromium)
            .count()
    }

    fn stopped(&self) -> usize {
        let facts = self.facts.lock().expect("facts");
        facts
            .stops
            .iter()
            .filter(|kind| **kind == BrowserChildKind::Chromium)
            .count()
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    bundle: ConfigBundle,
    gate: ProcessGateInput,
    clock: Arc<Clock>,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let bundle = bundle(root.path(), PermissionMode::Default)?;
        let gate = gate_input(&bundle)?;
        Ok(Self {
            _root: root,
            bundle,
            gate,
            clock: Arc::new(Clock::default()),
        })
    }

    fn rig(&self, state_dir: bool, page: Option<&Arc<Mutex<Page>>>) -> Rig {
        Rig::new(&self.bundle, &self.gate, &self.clock, state_dir, page)
    }

    fn call(&self, session: &str) -> BrowserCallContext {
        BrowserCallContext {
            effective_session_id: session.to_owned(),
            permission_mode_snapshot: self.gate.envelope.action.permission_mode_snapshot.clone(),
            permission_interactive: true,
            browser_screenshots: Some(false),
            native_image_input_supported: false,
            media_root: None,
        }
    }

    fn markers(&self) -> PathBuf {
        self.bundle
            .context
            .data_dir
            .join("runtime/browser-sessions")
    }

    fn marker(&self, session: &str) -> PathBuf {
        self.markers().join(format!(
            "{:x}.json",
            Sha256::digest(format!("local\n{session}").as_bytes())
        ))
    }

    fn marker_count(&self) -> Result<usize, Box<dyn Error>> {
        Ok(fs::read_dir(self.markers())?.count())
    }

    fn recorded_status(&self) -> Result<Value, Box<dyn Error>> {
        let path = self
            .bundle
            .context
            .data_dir
            .join("runtime/browser-state.json");
        let snapshot: Value = serde_json::from_slice(&fs::read(path)?)?;
        Ok(snapshot["status"].clone())
    }
}

/// The manager's own upkeep thread takes the browser slot for an instant
/// every 100 ms, and a call that meets it is answered `browser_busy`.
fn calm<T>(mut call: impl FnMut() -> Result<T, BrowserError>) -> Result<T, BrowserError> {
    for _ in 0..20 {
        match call() {
            Err(BrowserError::Busy) => thread::sleep(Duration::from_millis(2)),
            other => return other,
        }
    }
    call()
}

fn eventually(mut done: impl FnMut() -> bool) -> Result<(), Box<dyn Error>> {
    for _ in 0..400 {
        if done() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err("the browser manager did not settle".into())
}

fn settled(
    host: &ManagedBrowserHost,
    done: impl Fn(&BrowserStatus) -> bool,
) -> Result<BrowserStatus, Box<dyn Error>> {
    let mut status = host.status()?;
    eventually(|| {
        let _ = host.maintenance();
        match host.status() {
            Ok(current) => {
                status = current;
                done(&status)
            }
            Err(_) => false,
        }
    })?;
    Ok(status)
}

fn open() -> Value {
    json!({"action":"open","url":"https://example.com/"})
}

fn prepare(
    host: &ManagedBrowserHost,
    session: &str,
    arguments: &Value,
) -> Result<BrowserPreparedAction, BrowserError> {
    calm(|| host.prepare(session, arguments))
}

fn refusal<T>(result: Result<T, BrowserError>) -> Option<BrowserError> {
    result.err()
}

fn acquire(
    host: &ManagedBrowserHost,
    context: &BrowserCallContext,
) -> Result<Box<dyn BrowserSession>, BrowserError> {
    calm(|| host.acquire(context))
}

fn read(
    session: &dyn BrowserSession,
    context: &BrowserCallContext,
) -> Result<BrowserRead, BrowserError> {
    calm(|| session.read(context))
}

fn close_by_assistant(
    host: &ManagedBrowserHost,
    context: &BrowserCallContext,
) -> Result<(), BrowserError> {
    let close = prepare(
        host,
        &context.effective_session_id,
        &json!({"action":"close"}),
    )?;
    calm(|| host.close_prepared(context, &close.digest))
}

fn close_by_user(host: &ManagedBrowserHost) -> Result<(), Box<dyn Error>> {
    if host.close_by_user() == BrowserCloseOutcome::NothingOpen {
        return Err("no browser to close".into());
    }
    settled(host, |status| status.state == BrowserState::Idle).map(|_| ())
}

fn wait_for_reply(
    fixture: &Fixture,
    host: &ManagedBrowserHost,
    session: &str,
) -> Result<u64, Box<dyn Error>> {
    host.turn_finished(session);
    let status = settled(host, |status| status.reply_wait_deadline_ms.is_some())?;
    assert_eq!(
        status.reply_wait_deadline_ms,
        Some(fixture.clock.0.load(Ordering::SeqCst) + REPLY_WAIT_MS)
    );
    Ok(fixture.clock.0.load(Ordering::SeqCst))
}

fn let_reply_wait_pass(fixture: &Fixture, host: &ManagedBrowserHost) -> Result<(), Box<dyn Error>> {
    fixture.clock.0.fetch_add(REPLY_WAIT_MS, Ordering::SeqCst);
    closed_by_reply_wait(host)
}

fn closed_by_reply_wait(host: &ManagedBrowserHost) -> Result<(), Box<dyn Error>> {
    let status = settled(host, |status| status.state == BrowserState::Failed)?;
    assert_eq!(
        status.reason,
        Some(BrowserError::ReplyWaitTimeout.to_string())
    );
    assert_eq!(status.reply_wait_deadline_ms, None);
    Ok(())
}

#[test]
fn reply_wait_replaces_idle_timeout_until_next_turn() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let rig = fixture.rig(false, None);
    let host = &rig.host;
    let (owner, other) = (fixture.call(OWNER), fixture.call(OTHER));
    let now = || fixture.clock.0.load(Ordering::SeqCst);

    drop(acquire(host, &owner)?);
    host.turn_finished(OTHER);
    host.maintenance()?;
    assert_eq!(host.status()?.reply_wait_deadline_ms, None);
    fixture.clock.0.store(1_000, Ordering::SeqCst);
    let waiting_since = wait_for_reply(&fixture, host, OWNER)?;
    assert_eq!(host.status()?.reply_wait_minutes, 30);
    assert_eq!(
        host.describe(),
        BrowserDescription {
            state: BrowserDescriptionState::Open,
            session: Some(OWNER.to_owned()),
            reply_wait_remaining_minutes: Some(30),
            control: BrowserControlDescription::default(),
        }
    );

    fixture
        .clock
        .0
        .store(waiting_since + IDLE_MS + 60_000, Ordering::SeqCst);
    host.maintenance()?;
    assert_eq!(host.status()?.state, BrowserState::Running);
    assert_eq!(host.describe().reply_wait_remaining_minutes, Some(24));
    assert_eq!(
        refusal(host.prepare(OTHER, &open())),
        Some(BrowserError::Busy)
    );
    assert_eq!(refusal(host.acquire(&other)), Some(BrowserError::Busy));
    assert_eq!(host.describe().session.as_deref(), Some(OWNER));

    fixture
        .clock
        .0
        .store(waiting_since + REPLY_WAIT_MS - 1, Ordering::SeqCst);
    host.maintenance()?;
    assert_ne!(host.status()?.state, BrowserState::Failed);
    fixture.clock.0.fetch_add(1, Ordering::SeqCst);
    closed_by_reply_wait(host)?;
    assert_eq!(rig.stopped(), 1);
    assert_eq!(host.describe().state, BrowserDescriptionState::Closed);

    let timeout = Some(BrowserError::ReplyWaitTimeout);
    for _ in 0..3 {
        assert_eq!(refusal(prepare(host, OWNER, &open())), timeout);
        assert_eq!(refusal(acquire(host, &owner)), timeout);
        assert_eq!(refusal(host.capture_prepared(&owner, "digest")), timeout);
        assert_eq!(refusal(host.close_prepared(&owner, "digest")), timeout);
    }
    assert_eq!(rig.started(), 1);
    let notice = host.closure_notice(OWNER).ok_or("closure notice")?;
    assert_eq!(notice.error, BrowserError::ReplyWaitTimeout);
    assert!(!notice.token.contains("chat-owner"), "{}", notice.token);
    assert!(host.closure_notice(OTHER).is_none());

    drop(acquire(host, &other)?);
    assert_eq!(rig.started(), 2);
    assert_eq!(refusal(prepare(host, OWNER, &open())), timeout);
    assert_eq!(refusal(acquire(host, &owner)), timeout);

    host.acknowledge_closure_notice(OWNER, "not-a-token");
    host.acknowledge_closure_notice(OTHER, &notice.token);
    assert_eq!(host.closure_notice(OWNER), Some(notice.clone()));
    assert_eq!(host.pending_closure_notices()?, 1);
    host.acknowledge_closure_notice(OWNER, &notice.token);
    assert!(host.closure_notice(OWNER).is_none());
    assert_eq!(host.pending_closure_notices()?, 0);
    assert_eq!(
        refusal(host.prepare(OWNER, &open())),
        Some(BrowserError::Busy)
    );
    close_by_assistant(host, &other)?;
    assert!(prepare(host, OWNER, &open()).is_ok());

    drop(acquire(host, &owner)?);
    wait_for_reply(&fixture, host, OWNER)?;
    let_reply_wait_pass(&fixture, host)?;
    let second = host.closure_notice(OWNER).ok_or("second closure notice")?;
    assert_ne!(second.token, notice.token);
    host.acknowledge_closure_notice(OWNER, &notice.token);
    assert_eq!(host.closure_notice(OWNER), Some(second.clone()));
    assert_eq!(refusal(prepare(host, OWNER, &open())), timeout);
    host.acknowledge_closure_notice(OWNER, &second.token);

    drop(acquire(host, &owner)?);
    wait_for_reply(&fixture, host, OWNER)?;
    fixture.clock.0.fetch_add(10_000, Ordering::SeqCst);
    let turn_started = now();
    host.turn_started(OWNER);
    settled(host, |status| status.reply_wait_deadline_ms.is_none())?;
    assert_eq!(host.describe().reply_wait_remaining_minutes, None);
    fixture
        .clock
        .0
        .store(turn_started + IDLE_MS - 1, Ordering::SeqCst);
    host.maintenance()?;
    assert_eq!(host.status()?.state, BrowserState::Running);
    fixture
        .clock
        .0
        .store(turn_started + IDLE_MS, Ordering::SeqCst);
    let status = settled(host, |status| status.state == BrowserState::Failed)?;
    assert_eq!(status.reason, Some(BrowserError::IdleTimeout.to_string()));
    assert!(host.closure_notice(OWNER).is_none());
    assert!(prepare(host, OWNER, &open()).is_ok());
    Ok(())
}

#[test]
fn capture_forces_verified_screenshot_and_closes_tab_on_failure() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let owner = fixture.call(OWNER);
    let capture = json!({"action":"capture"});

    let rig = fixture.rig(false, None);
    assert_eq!(
        refusal(prepare(&rig.host, OWNER, &capture)),
        Some(BrowserError::NotOpen)
    );
    assert_eq!(
        refusal(calm(|| rig.host.capture_prepared(&owner, "digest"))),
        Some(BrowserError::NotOpen)
    );
    assert_eq!(rig.started(), 0);

    let page = Page::shared();
    let rig = fixture.rig(false, Some(&page));
    let session = acquire(&rig.host, &owner)?;
    let looked = read(session.as_ref(), &owner)?;
    assert!(looked.screenshot.is_none());
    assert_eq!(sent(&page, "Page.captureScreenshot"), 0);
    let prepared = prepare(&rig.host, OWNER, &capture)?;
    let captured = calm(|| rig.host.capture_prepared(&owner, &prepared.digest))?;
    assert_eq!(captured.png, PNG);
    assert_eq!(captured.document, looked.document);
    assert_eq!(sent(&page, "Page.captureScreenshot"), 1);
    assert_eq!(rig.host.status()?.state, BrowserState::Running);
    assert_eq!(sent(&page, "Target.closeTarget"), 0);

    let prepared = prepare(&rig.host, OWNER, &capture)?;
    page.lock().expect("page").url = "file:///secret".to_owned();
    assert_eq!(
        refusal(calm(|| rig.host.capture_prepared(&owner, &prepared.digest))),
        Some(BrowserError::DocumentChanged)
    );
    assert_eq!(sent(&page, "Page.captureScreenshot"), 1);
    assert_eq!(sent(&page, "Target.closeTarget"), 1);
    assert_eq!(rig.host.status()?.state, BrowserState::Failed);
    assert_eq!(rig.stopped(), 1);

    let page = Page::shared();
    let rig = fixture.rig(false, Some(&page));
    let session = acquire(&rig.host, &owner)?;
    read(session.as_ref(), &owner)?;
    let prepared = prepare(&rig.host, OWNER, &capture)?;
    page.lock().expect("page").moves_while_captured = true;
    assert_eq!(
        refusal(calm(|| rig.host.capture_prepared(&owner, &prepared.digest))),
        Some(BrowserError::DocumentChanged)
    );
    assert_eq!(sent(&page, "Page.captureScreenshot"), 1);
    assert_eq!(sent(&page, "Target.closeTarget"), 1);
    assert_eq!(rig.host.status()?.state, BrowserState::Failed);
    assert_eq!(rig.stopped(), 1);
    assert_eq!(
        refusal(prepare(&rig.host, OWNER, &capture)),
        Some(BrowserError::NotOpen)
    );
    Ok(())
}

#[test]
fn capture_after_navigation_since_authorization_is_stale() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let owner = fixture.call(OWNER);
    let capture = json!({"action":"capture"});
    let page = Page::shared();
    let rig = fixture.rig(false, Some(&page));
    let session = acquire(&rig.host, &owner)?;
    read(session.as_ref(), &owner)?;
    let prepared = prepare(&rig.host, OWNER, &capture)?;

    page.lock().expect("page").navigate();

    for _ in 0..2 {
        assert_eq!(
            refusal(calm(|| rig.host.capture_prepared(&owner, &prepared.digest))),
            Some(BrowserError::StaleRevision)
        );
    }
    assert_eq!(sent(&page, "Page.captureScreenshot"), 0);
    assert_eq!(sent(&page, "Target.closeTarget"), 0);
    assert_eq!(rig.host.status()?.state, BrowserState::Running);
    assert_eq!((rig.started(), rig.stopped()), (1, 0));
    assert_eq!(rig.host.describe().session.as_deref(), Some(OWNER));

    let fresh = prepare(&rig.host, OWNER, &capture)?;
    assert_ne!(fresh.digest, prepared.digest);
    let captured = calm(|| rig.host.capture_prepared(&owner, &fresh.digest))?;
    assert_eq!(captured.png, PNG);
    assert_eq!(captured.document.loader_id, "loader-2");
    assert_eq!(captured.document.url, "https://example.com/next");
    assert_eq!(sent(&page, "Page.captureScreenshot"), 1);
    assert_eq!(rig.started(), 1);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum CloseRequest {
    ByUser,
    OwnSession,
    OtherSession,
}

/// What a close asked for right now is answered. A session's own close only
/// says whether it was taken.
fn ask(host: &ManagedBrowserHost, request: CloseRequest) -> BrowserCloseOutcome {
    let session = match request {
        CloseRequest::ByUser => return host.close_by_user(),
        CloseRequest::OwnSession => OWNER,
        CloseRequest::OtherSession => OTHER,
    };
    if host.close_for_session(session) {
        BrowserCloseOutcome::Scheduled
    } else {
        BrowserCloseOutcome::NothingOpen
    }
}

/// Whether the close request was answered as it should be, and which session
/// held the browser then.
type Scheduled = Arc<Mutex<Option<(bool, Option<String>)>>>;

/// Ask for a close from inside the owner's next read, while that read holds
/// the browser, and then let `fail` break the read.
fn schedule_close_during_read(
    rig: &Rig,
    page: &Arc<Mutex<Page>>,
    request: CloseRequest,
    fail: impl FnOnce() + Send + 'static,
) -> Scheduled {
    let scheduled = Arc::new(Mutex::new(None));
    page.lock().expect("page").during_read = Some(Box::new({
        let (host, scheduled) = (rig.host.clone(), Arc::clone(&scheduled));
        move || {
            // Another session has no browser here to close.
            let expected = match request {
                CloseRequest::OtherSession => BrowserCloseOutcome::NothingOpen,
                _ => BrowserCloseOutcome::Scheduled,
            };
            let outcome = ask(&host, request) == expected;
            let running = host.describe().session;
            *scheduled.lock().expect("scheduled") = Some((outcome, running));
            fail();
        }
    }));
    scheduled
}

#[test]
fn close_request_during_action_runs_after_action() -> Result<(), Box<dyn Error>> {
    for request in [
        CloseRequest::ByUser,
        CloseRequest::OwnSession,
        CloseRequest::OtherSession,
    ] {
        let fixture = Fixture::new()?;
        let owner = fixture.call(OWNER);
        let page = Page::shared();
        let rig = fixture.rig(true, Some(&page));
        let session = acquire(&rig.host, &owner)?;
        let waiting_since = wait_for_reply(&fixture, &rig.host, OWNER)?;
        let recorded = fixture.recorded_status()?;
        assert_eq!(recorded["replyWaitMinutes"], 30, "{request:?}");
        assert_eq!(
            recorded["replyWaitDeadlineMs"],
            waiting_since + REPLY_WAIT_MS,
            "{request:?}"
        );

        let scheduled = schedule_close_during_read(&rig, &page, request, || {});
        let looked = read(session.as_ref(), &owner);

        assert!(looked.is_ok(), "{request:?}: {:?}", looked.err());
        assert_eq!(
            scheduled.lock().expect("scheduled").take(),
            Some((true, Some(OWNER.to_owned()))),
            "{request:?}"
        );
        if matches!(request, CloseRequest::OtherSession) {
            assert_eq!(rig.host.status()?.state, BrowserState::Running);
            assert_eq!(rig.stopped(), 0);
            assert_eq!(rig.host.pending_closure_notices()?, 0);
            assert!(fixture.recorded_status()?["replyWaitDeadlineMs"].is_u64());
            continue;
        }
        assert_eq!(rig.host.status()?.state, BrowserState::Idle, "{request:?}");
        assert_eq!(rig.stopped(), 1, "{request:?}");
        assert_eq!(
            rig.host.closure_notice(OWNER).map(|notice| notice.error),
            Some(BrowserError::Closed),
            "{request:?}"
        );
        let recorded = fixture.recorded_status()?;
        assert!(recorded["replyWaitDeadlineMs"].is_null(), "{recorded}");
        assert_eq!(recorded["replyWaitMinutes"], 30);
        assert_eq!(
            refusal(prepare(&rig.host, OWNER, &json!({"action":"look"}))),
            Some(BrowserError::Closed),
            "{request:?}"
        );
        assert_eq!(rig.started(), 1, "{request:?}");
    }
    Ok(())
}

#[test]
fn close_request_survives_action_that_fails_after_it() -> Result<(), Box<dyn Error>> {
    #[derive(Clone, Copy, Debug)]
    enum Failure {
        PageTimeout,
        DocumentChanged,
    }
    for (request, failure) in [
        (CloseRequest::ByUser, Failure::PageTimeout),
        (CloseRequest::ByUser, Failure::DocumentChanged),
        (CloseRequest::OwnSession, Failure::PageTimeout),
        (CloseRequest::OwnSession, Failure::DocumentChanged),
        (CloseRequest::OtherSession, Failure::PageTimeout),
    ] {
        let case = format!("{request:?} {failure:?}");
        let fixture = Fixture::new()?;
        let owner = fixture.call(OWNER);
        let marker = fixture.marker(OWNER);
        let page = Page::shared();
        let rig = fixture.rig(true, Some(&page));
        let session = acquire(&rig.host, &owner)?;
        wait_for_reply(&fixture, &rig.host, OWNER)?;

        let scheduled = schedule_close_during_read(&rig, &page, request, {
            let (clock, page) = (Arc::clone(&fixture.clock), Arc::clone(&page));
            move || match failure {
                Failure::PageTimeout => {
                    clock.0.fetch_add(PAGE_TIMEOUT_MS, Ordering::SeqCst);
                }
                Failure::DocumentChanged => page.lock().expect("page").navigate(),
            }
        });
        let expected = match failure {
            Failure::PageTimeout => BrowserError::PageTimeout,
            Failure::DocumentChanged => BrowserError::DocumentChanged,
        };

        assert_eq!(
            refusal(read(session.as_ref(), &owner)),
            Some(expected),
            "{case}"
        );

        assert_eq!(
            scheduled.lock().expect("scheduled").take(),
            Some((true, Some(OWNER.to_owned()))),
            "{case}"
        );
        let status = rig.host.status()?;
        assert_eq!(status.state, BrowserState::Failed, "{case}");
        assert_eq!(status.reason, Some(expected.to_string()), "{case}");
        assert_eq!(rig.stopped(), 1, "{case}");
        if matches!(request, CloseRequest::OtherSession) {
            assert!(rig.host.closure_notice(OWNER).is_none(), "{case}");
            assert!(rig.host.closure_notice(OTHER).is_none(), "{case}");
            assert_eq!(rig.host.pending_closure_notices()?, 0, "{case}");
            assert!(!marker.exists(), "{case}");
            continue;
        }
        assert_eq!(
            rig.host.closure_notice(OWNER).map(|notice| notice.error),
            Some(BrowserError::Closed),
            "{case}"
        );
        let written = fs::read_to_string(&marker)?;
        assert!(written.contains(r#""state":"closed""#), "{case}: {written}");
        for _ in 0..2 {
            assert_eq!(
                refusal(prepare(&rig.host, OWNER, &json!({"action":"look"}))),
                Some(BrowserError::Closed),
                "{case}"
            );
            assert_eq!(
                refusal(acquire(&rig.host, &owner)),
                Some(BrowserError::Closed),
                "{case}"
            );
        }
        assert_eq!(rig.started(), 1, "{case}");
        // A later browser of another session is not closed by the request
        // that this generation already answered.
        let other = fixture.call(OTHER);
        drop(acquire(&rig.host, &other)?);
        let status = settled(&rig.host, |status| status.state == BrowserState::Running)?;
        assert_eq!(status.state, BrowserState::Running, "{case}");
        assert!(rig.host.closure_notice(OTHER).is_none(), "{case}");
        close_by_assistant(&rig.host, &other)?;
        drop(session);
        drop(rig);

        let restarted = fixture.rig(true, None);
        eventually(|| restarted.host.closure_notice(OWNER).is_some())?;
        assert_eq!(
            refusal(prepare(&restarted.host, OWNER, &open())),
            Some(BrowserError::Closed),
            "{case}"
        );
        assert_eq!(
            refusal(acquire(&restarted.host, &owner)),
            Some(BrowserError::Closed),
            "{case}"
        );
        assert_eq!(restarted.started(), 0, "{case}");
        let notice = restarted
            .host
            .closure_notice(OWNER)
            .ok_or("closure notice")?;
        assert_eq!(notice.error, BrowserError::Closed, "{case}");
        restarted
            .host
            .acknowledge_closure_notice(OWNER, &notice.token);
        assert!(!marker.exists(), "{case}");
        assert!(prepare(&restarted.host, OWNER, &open()).is_ok(), "{case}");
    }
    Ok(())
}

/// The window the close used to be lost in: the manager has already looked
/// for a waiting close and is taking the generation apart, still holding the
/// browser.
#[test]
fn close_request_during_teardown_finds_nothing_open() -> Result<(), Box<dyn Error>> {
    for request in [CloseRequest::ByUser, CloseRequest::OwnSession] {
        let fixture = Fixture::new()?;
        let owner = fixture.call(OWNER);
        let marker = fixture.marker(OWNER);
        let page = Page::shared();
        let rig = fixture.rig(true, Some(&page));
        let session = acquire(&rig.host, &owner)?;
        wait_for_reply(&fixture, &rig.host, OWNER)?;
        assert!(marker.exists(), "{request:?}");

        let answered = Arc::new(Mutex::new(None));
        {
            let mut page = page.lock().expect("page");
            let clock = Arc::clone(&fixture.clock);
            page.during_read = Some(Box::new(move || {
                clock.0.fetch_add(PAGE_TIMEOUT_MS, Ordering::SeqCst);
            }));
            let (host, answered) = (rig.host.clone(), Arc::clone(&answered));
            page.during_teardown = Some(Box::new(move || {
                *answered.lock().expect("answered") = Some(ask(&host, request));
            }));
        }

        assert_eq!(
            refusal(read(session.as_ref(), &owner)),
            Some(BrowserError::PageTimeout),
            "{request:?}"
        );

        assert_eq!(
            answered.lock().expect("answered").take(),
            Some(BrowserCloseOutcome::NothingOpen),
            "{request:?}"
        );
        let status = rig.host.status()?;
        assert_eq!(status.state, BrowserState::Failed, "{request:?}");
        assert_eq!(rig.stopped(), 1, "{request:?}");
        assert!(rig.host.closure_notice(OWNER).is_none(), "{request:?}");
        assert_eq!(rig.host.pending_closure_notices()?, 0, "{request:?}");
        assert!(!marker.exists(), "{request:?}");
        // Nothing was left behind to close the session's next browser.
        drop(acquire(&rig.host, &owner)?);
        let status = settled(&rig.host, |status| status.state == BrowserState::Running)?;
        assert_eq!(status.state, BrowserState::Running, "{request:?}");
        assert_eq!(rig.started(), 2, "{request:?}");
        assert_eq!(rig.stopped(), 1, "{request:?}");
        assert!(rig.host.closure_notice(OWNER).is_none(), "{request:?}");
    }
    Ok(())
}

#[test]
fn close_request_during_start_is_told_however_start_ends() -> Result<(), Box<dyn Error>> {
    for (request, fails) in [
        (CloseRequest::ByUser, false),
        (CloseRequest::ByUser, true),
        (CloseRequest::OwnSession, false),
        (CloseRequest::OwnSession, true),
    ] {
        let case = format!("{request:?} fails={fails}");
        let fixture = Fixture::new()?;
        let owner = fixture.call(OWNER);
        let marker = fixture.marker(OWNER);
        let page = Page::shared();
        let rig = fixture.rig(true, Some(&page));
        let answered = Arc::new(Mutex::new(None));
        page.lock().expect("page").during_start = Some(Box::new({
            let (host, answered) = (rig.host.clone(), Arc::clone(&answered));
            let clock = Arc::clone(&fixture.clock);
            let start_limit_ms = fixture.bundle.config.tools.browser.startup_timeout_seconds * 1000;
            move || {
                let other = ask(&host, CloseRequest::OtherSession);
                *answered.lock().expect("answered") = Some((ask(&host, request), other));
                if fails {
                    clock.0.fetch_add(start_limit_ms, Ordering::SeqCst);
                }
            }
        }));

        let started = acquire(&rig.host, &owner);

        assert_eq!(
            answered.lock().expect("answered").take(),
            Some((
                BrowserCloseOutcome::Scheduled,
                BrowserCloseOutcome::NothingOpen
            )),
            "{case}"
        );
        if fails {
            assert_eq!(
                refusal(started),
                Some(BrowserError::StartupTimeout),
                "{case}"
            );
            assert_eq!(rig.host.status()?.state, BrowserState::Failed, "{case}");
        } else {
            assert!(started.is_ok(), "{case}: {:?}", started.err());
            let status = settled(&rig.host, |status| status.state == BrowserState::Idle)?;
            assert_eq!(status.state, BrowserState::Idle, "{case}");
        }
        assert_eq!(rig.stopped(), 1, "{case}");
        assert_eq!(
            rig.host.closure_notice(OWNER).map(|notice| notice.error),
            Some(BrowserError::Closed),
            "{case}"
        );
        assert!(rig.host.closure_notice(OTHER).is_none(), "{case}");
        let written = fs::read_to_string(&marker)?;
        assert!(written.contains(r#""state":"closed""#), "{case}: {written}");
        assert_eq!(
            refusal(acquire(&rig.host, &owner)),
            Some(BrowserError::Closed),
            "{case}"
        );
        assert_eq!(rig.started(), 1, "{case}");
    }
    Ok(())
}

#[test]
fn restart_reports_browser_closed_once() -> Result<(), Box<dyn Error>> {
    #[derive(Clone, Copy, Debug)]
    enum Ending {
        Shutdown,
        Abandoned,
        ReplyWait,
    }
    for ending in [Ending::Shutdown, Ending::Abandoned, Ending::ReplyWait] {
        let fixture = Fixture::new()?;
        let owner = fixture.call(OWNER);
        let marker = fixture.marker(OWNER);
        let first = fixture.rig(true, None);
        drop(acquire(&first.host, &owner)?);
        let written = fs::read_to_string(&marker)?;
        assert!(written.contains(r#""state":"open""#), "{written}");
        assert!(written.len() < 100, "{written}");
        assert!(
            !written.contains("chat-owner") && !written.contains("example.com"),
            "{written}"
        );
        assert_eq!(
            fs::metadata(&marker)?.permissions().mode() & 0o777,
            0o600,
            "{ending:?}"
        );
        assert_eq!(
            fs::metadata(fixture.markers())?.permissions().mode() & 0o777,
            0o700,
            "{ending:?}"
        );

        let beside = fixture.rig(true, None);
        assert!(beside.host.closure_notice(OWNER).is_none(), "{ending:?}");
        assert!(prepare(&beside.host, OWNER, &open()).is_ok(), "{ending:?}");
        assert_eq!(fs::read_to_string(&marker)?, written, "{ending:?}");
        drop(beside);

        let (expected, left) = match ending {
            Ending::Shutdown => {
                first.host.shutdown()?;
                (BrowserError::Closed, "closed")
            }
            Ending::Abandoned => (BrowserError::Closed, "open"),
            Ending::ReplyWait => {
                wait_for_reply(&fixture, &first.host, OWNER)?;
                let_reply_wait_pass(&fixture, &first.host)?;
                (BrowserError::ReplyWaitTimeout, "reply_wait_timeout")
            }
        };
        drop(first);
        let written = fs::read_to_string(&marker)?;
        assert!(
            written.contains(&format!(r#""state":"{left}""#)),
            "{ending:?}: {written}"
        );

        let second = fixture.rig(true, None);
        // The abandoned manager's lock ends with its last reference, which
        // its own upkeep thread may hold for a moment longer.
        eventually(|| second.host.closure_notice(OWNER).is_some())?;
        for _ in 0..2 {
            assert_eq!(
                refusal(prepare(&second.host, OWNER, &open())),
                Some(expected),
                "{ending:?}"
            );
            assert_eq!(
                refusal(acquire(&second.host, &owner)),
                Some(expected),
                "{ending:?}"
            );
        }
        assert_eq!(second.started(), 0, "{ending:?}");
        assert!(second.host.closure_notice(OTHER).is_none(), "{ending:?}");
        assert!(prepare(&second.host, OTHER, &open()).is_ok(), "{ending:?}");
        assert_eq!(fs::read_to_string(&marker)?, written, "{ending:?}");

        let notice = second.host.closure_notice(OWNER).ok_or("closure notice")?;
        assert_eq!(notice.error, expected, "{ending:?}");
        second.host.acknowledge_closure_notice(OWNER, &notice.token);
        assert!(!marker.exists(), "{ending:?}");
        assert!(prepare(&second.host, OWNER, &open()).is_ok(), "{ending:?}");
        drop(acquire(&second.host, &owner)?);
        assert_eq!(second.started(), 1, "{ending:?}");
        assert!(fs::read_to_string(&marker)?.contains(r#""state":"open""#));
    }

    let fixture = Fixture::new()?;
    let owner = fixture.call(OWNER);
    let marker = fixture.marker(OWNER);
    let rig = fixture.rig(true, None);
    drop(acquire(&rig.host, &owner)?);
    assert!(marker.is_file());
    close_by_assistant(&rig.host, &owner)?;
    assert!(!marker.exists());
    drop(acquire(&rig.host, &owner)?);
    assert!(marker.is_file());
    fixture.clock.0.fetch_add(IDLE_MS, Ordering::SeqCst);
    let status = settled(&rig.host, |status| status.state == BrowserState::Failed)?;
    assert_eq!(status.reason, Some(BrowserError::IdleTimeout.to_string()));
    assert!(!marker.exists());
    assert!(rig.host.closure_notice(OWNER).is_none());
    assert_eq!(rig.host.pending_closure_notices()?, 0);

    fs::remove_dir(fixture.markers())?;
    fs::write(fixture.markers(), b"")?;
    assert_eq!(
        refusal(acquire(&rig.host, &owner)),
        Some(BrowserError::StateUnavailable)
    );
    assert_eq!(rig.started(), 2);
    Ok(())
}

fn leave_twenty_notices(
    fixture: &Fixture,
    rig: &Rig,
) -> Result<Vec<(String, BrowserError)>, Box<dyn Error>> {
    let mut left = Vec::new();
    for index in 0..20 {
        let session = format!("telegram:chat-{index}");
        drop(acquire(&rig.host, &fixture.call(&session))?);
        let error = if index % 2 == 0 {
            wait_for_reply(fixture, &rig.host, &session)?;
            let_reply_wait_pass(fixture, &rig.host)?;
            BrowserError::ReplyWaitTimeout
        } else {
            close_by_user(&rig.host)?;
            BrowserError::Closed
        };
        left.push((session, error));
    }
    assert_eq!(rig.started(), 20);
    assert_eq!(rig.host.pending_closure_notices()?, 20);
    assert_eq!(fixture.marker_count()?, 20);
    Ok(left)
}

fn receive_each_once(
    fixture: &Fixture,
    rig: &Rig,
    left: &[(String, BrowserError)],
) -> Result<(), Box<dyn Error>> {
    for (index, (session, error)) in left.iter().enumerate() {
        let context = fixture.call(session);
        assert_eq!(
            refusal(prepare(&rig.host, session, &open())),
            Some(*error),
            "{session}"
        );
        assert_eq!(
            refusal(acquire(&rig.host, &context)),
            Some(*error),
            "{session}"
        );
        let notice = rig.host.closure_notice(session).ok_or("closure notice")?;
        rig.host.acknowledge_closure_notice(session, &notice.token);
        assert!(prepare(&rig.host, session, &open()).is_ok(), "{session}");
        assert!(rig.host.closure_notice(session).is_none(), "{session}");
        assert!(!fixture.marker(session).exists(), "{session}");
        assert_eq!(fixture.marker_count()?, left.len() - index - 1, "{session}");
        for (later, _) in &left[index + 1..] {
            assert!(fixture.marker(later).is_file(), "{session}: {later}");
        }
    }
    Ok(())
}

#[test]
fn undelivered_closure_notices_are_kept_past_sixteen_sessions() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let first = fixture.rig(true, None);
    let left = leave_twenty_notices(&fixture, &first)?;
    drop(first);
    let restarted = fixture.rig(true, None);
    assert_eq!(restarted.host.pending_closure_notices()?, 0);
    receive_each_once(&fixture, &restarted, &left)?;
    assert_eq!(restarted.started(), 0);
    assert_eq!(restarted.host.pending_closure_notices()?, 0);

    let fixture = Fixture::new()?;
    let same = fixture.rig(true, None);
    let left = leave_twenty_notices(&fixture, &same)?;
    receive_each_once(&fixture, &same, &left)?;
    assert_eq!(same.started(), 20);
    assert_eq!(same.host.pending_closure_notices()?, 0);
    Ok(())
}
