use super::spec049_browser_runtime::{self as existing, Clock, Facts, Runner};
use fs2::FileExt;
use serde_json::{json, Value};
use shacs_cli::browser::{
    self, BrowserCancellation, BrowserChild, BrowserChildKind, BrowserChildSpec, BrowserClock,
    BrowserControl, BrowserFolder, BrowserProcessRunner, BrowserSnapshotDestination,
    BrowserStartup, CdpReply, DevToolsPeer, ManagedBrowserHost, PausedTargetAdmission,
    ViewerListenAddress, ViewerListener,
};
use shacs_config::{BrowserConfig, ConfigBundle, PermissionMode};
use shacs_core::runtime::{
    BrowserCallContext, BrowserCloseOutcome, BrowserError, BrowserHost, BrowserTakeoverError,
    BrowserTakeoverRequest, BrowserUnavailableReason, NetworkGuard,
    ProcessContainmentProofCandidate, ProcessGateInput, RenderedWebReader, RenderedWebRequest,
    WebCheckFailure, WebFetchLimits,
};
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;
const URL: &str = "https://example.com/item";

struct AdvancingClock {
    clock: Arc<Clock>,
    pause: Arc<Mutex<Option<StopPoint>>>,
}
impl BrowserClock for AdvancingClock {
    fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }
    fn pause(&self, ms: u64) {
        let point = self.pause.lock().expect("clock pause lock poisoned").take();
        if let Some(point) = point {
            point.wait(BrowserCancellation::default());
        }
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Default)]
pub(super) enum Fault {
    #[default]
    None,
    OldLoad,
    WrongResponse,
    WrongRequest,
    Pending,
    Loader,
    Scheme,
    Epoch,
    Panic,
}

pub(super) struct StopPoint {
    entered: mpsc::Sender<BrowserCancellation>,
    release: mpsc::Receiver<()>,
}

impl StopPoint {
    fn wait(self, cancellation: BrowserCancellation) {
        self.entered
            .send(cancellation)
            .expect("entry receiver gone");
        self.release
            .recv_timeout(Duration::from_secs(5))
            .expect("release deadline");
    }
}

pub(super) fn pause_at(
    slot: &Mutex<Option<StopPoint>>,
) -> (mpsc::Receiver<BrowserCancellation>, mpsc::Sender<()>) {
    let (entered, receive) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    *slot.lock().expect("pause slot lock poisoned") = Some(StopPoint {
        entered,
        release: wait,
    });
    (receive, release)
}

struct ProcessRunner {
    inner: Runner,
    admitting: Arc<Mutex<Option<StopPoint>>>,
    starting: Arc<Mutex<Option<StopPoint>>>,
    stopping: Arc<Mutex<Option<StopPoint>>>,
}

impl BrowserProcessRunner for ProcessRunner {
    fn hostname(&self) -> Option<String> {
        Some("spec051-host".to_owned())
    }
    fn chromium_running(&self, pid: u32) -> bool {
        pid == 123
    }
    fn available(&self) -> bool {
        let point = self
            .admitting
            .lock()
            .expect("admission pause lock poisoned")
            .take();
        if let Some(point) = point {
            point.wait(BrowserCancellation::default());
        }
        self.inner.available()
    }
    fn viewer_available(&self) -> bool {
        self.inner.viewer_available()
    }
    fn viewer_listener(
        &self,
        address: ViewerListenAddress,
        port: u16,
        owner: Option<u32>,
    ) -> ViewerListener {
        self.inner.viewer_listener(address, port, owner)
    }
    fn display_ready(
        &self,
        display: u16,
        deadline: Instant,
        cancellation: &BrowserCancellation,
    ) -> Result<(), BrowserError> {
        self.inner.display_ready(display, deadline, cancellation)
    }
    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        let child = self.inner.spawn(spec, deadline)?;
        if spec.kind != BrowserChildKind::Chromium {
            return Ok(child);
        }
        let point = self
            .starting
            .lock()
            .expect("start pause lock poisoned")
            .take();
        if let Some(point) = point {
            point.wait(BrowserCancellation::default());
        }
        Ok(Box::new(ProcessChild {
            inner: child,
            stopping: self.stopping.clone(),
            stopped: false,
        }))
    }
}

struct ProcessChild {
    inner: Box<dyn BrowserChild>,
    stopping: Arc<Mutex<Option<StopPoint>>>,
    stopped: bool,
}

impl BrowserChild for ProcessChild {
    fn exited(&mut self) -> Result<bool, BrowserError> {
        self.inner.exited()
    }
    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        self.inner.succeeded()
    }
    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        self.inner.take_devtools()
    }
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.inner.stop();
        let point = self
            .stopping
            .lock()
            .expect("stop pause lock poisoned")
            .take();
        if let Some(point) = point {
            point.wait(BrowserCancellation::default());
        }
    }
}

pub(super) struct Page {
    pub(super) text: String,
    pub(super) fault: Fault,
    pub(super) status: u16,
    pub(super) commands: Vec<(String, Value, Option<String>)>,
    evaluated_at: Vec<u64>,
    late_request: bool,
    popups: bool,
    block: Option<StopPoint>,
    block_method: &'static str,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            text: "Price: 12900 USD".to_owned(),
            fault: Fault::None,
            status: 200,
            commands: Vec::new(),
            evaluated_at: Vec::new(),
            late_request: false,
            popups: false,
            block: None,
            block_method: "Runtime.evaluate",
        }
    }
}

struct Peer {
    page: Arc<Mutex<Page>>,
    clock: Arc<Clock>,
    cancellation: BrowserCancellation,
    sequence: u64,
    epoch: u64,
    navigated: bool,
    evaluated: bool,
    late_sent: bool,
    late_finished: bool,
    popup_sent: bool,
}

impl Peer {
    fn new(page: Arc<Mutex<Page>>, clock: Arc<Clock>) -> Self {
        Self {
            page,
            clock,
            cancellation: BrowserCancellation::default(),
            sequence: 0,
            epoch: 0,
            navigated: false,
            evaluated: false,
            late_sent: false,
            late_finished: false,
            popup_sent: false,
        }
    }

    fn event(method: &str, params: Value) -> Value {
        json!({"method":method,"sessionId":"session-1","params":params})
    }

    fn popup(paused: bool, id: &str) -> Value {
        json!({"method":"Target.attachedToTarget","params":{
            "sessionId":format!("{id}-session"),"waitingForDebugger":paused,
            "targetInfo":{"targetId":id,"type":"page","url":"https://example.com/popup"}
        }})
    }
}

impl DevToolsPeer for Peer {
    fn set_cancellation(&mut self, cancellation: BrowserCancellation) {
        self.cancellation = cancellation;
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        deadline: Instant,
        admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        self.cancellation.check()?;
        let point = {
            let mut page = self.page.lock().expect("page lock poisoned");
            page.commands.push((
                method.to_owned(),
                params.clone(),
                session.map(str::to_owned),
            ));
            if method == page.block_method {
                page.block.take()
            } else {
                None
            }
        };
        if let Some(point) = point {
            point.wait(self.cancellation.clone());
            self.cancellation.check()?;
        }
        let panic = method == "Runtime.evaluate"
            && matches!(
                self.page.lock().expect("page lock poisoned").fault,
                Fault::Panic
            );
        assert!(!panic, "fixture renderer panic");
        let mut events = Vec::new();
        let popups = self.page.lock().expect("page lock poisoned").popups;
        if method == "Runtime.evaluate" && popups {
            self.epoch += 1;
            let mut popup = Self::popup(true, "paused-popup");
            let nested = admit(self, &mut popup, deadline)?;
            browser::mark_admitted(&mut popup);
            assert_eq!(
                popup["params"]["rejected"], true,
                "watch must close before resume"
            );
            events.push(popup);
            events.extend(nested);
        }
        self.sequence += 1;
        let mut page = self.page.lock().expect("page lock poisoned");
        let result = match method {
            "Target.getTargets" => json!({"targetInfos":[{"targetId":"target-1","type":"page"}]}),
            "Target.attachToTarget" => json!({"sessionId":"session-1"}),
            "Page.navigate" => {
                self.navigated = true;
                self.epoch += 1;
                events.push(Self::event(
                    "Page.frameNavigated",
                    json!({"frame":{
                        "id":"frame-1","loaderId":"loader-1","url":URL
                    }}),
                ));
                events.push(Self::event(
                    "Network.requestWillBeSent",
                    json!({
                        "requestId":"main","frameId":"frame-1","loaderId":"loader-1",
                        "type":"Document","request":{"url":URL}
                    }),
                ));
                // Reused redirect ids count once, and old-loader/subframe HTTP errors do not win.
                events.push(Self::event(
                    "Network.requestWillBeSent",
                    json!({
                        "requestId":"main","frameId":"frame-1","loaderId":"loader-1",
                        "type":"Document","request":{"url":URL},"redirectResponse":{"status":302}
                    }),
                ));
                for (frame, loader, status) in [
                    ("frame-1", "old-loader", 429),
                    ("child-frame", "loader-1", 429),
                    (
                        "frame-1",
                        if matches!(page.fault, Fault::WrongResponse) {
                            "wrong-loader"
                        } else {
                            "loader-1"
                        },
                        page.status,
                    ),
                ] {
                    events.push(Self::event("Network.responseReceived", json!({
                        "requestId":if matches!(page.fault, Fault::WrongRequest) { "unknown-request" } else { "main" },
                        "frameId":frame,"loaderId":loader,"type":"Document",
                        "response":{"url":URL,"status":f64::from(status),"headers":{"Retry-After":"3600","Set-Cookie":"secret-cookie"}}
                    })));
                }
                events.push(Self::event(
                    "Network.loadingFinished",
                    json!({"requestId":"main"}),
                ));
                events.push(Self::event(
                    "Page.lifecycleEvent",
                    json!({
                        "frameId":"frame-1","loaderId":"old-loader","name":"load"
                    }),
                ));
                if !matches!(page.fault, Fault::OldLoad) {
                    events.push(Self::event(
                        "Page.lifecycleEvent",
                        json!({
                            "frameId":"frame-1","loaderId":"loader-1","name":"load"
                        }),
                    ));
                }
                if matches!(page.fault, Fault::Pending) {
                    events.push(Self::event(
                        "Network.requestWillBeSent",
                        json!({
                            "requestId":"never","frameId":"frame-1","loaderId":"loader-1",
                            "type":"Fetch","request":{"url":"https://example.com/never"}
                        }),
                    ));
                }
                json!({"frameId":"frame-1","loaderId":"loader-1"})
            }
            "Page.getFrameTree" => {
                let url = if !self.navigated {
                    "about:blank"
                } else if self.evaluated && matches!(page.fault, Fault::Scheme) {
                    "file:///secret"
                } else {
                    URL
                };
                let loader = if self.evaluated && matches!(page.fault, Fault::Loader) {
                    "new-loader"
                } else {
                    "loader-1"
                };
                json!({"frameTree":{"frame":{"id":"frame-1","loaderId":loader,"url":url}}})
            }
            "Runtime.evaluate" => {
                self.evaluated = true;
                page.evaluated_at.push(self.clock.now_ms());
                if matches!(page.fault, Fault::Epoch) {
                    self.epoch += 1;
                }
                // The expression itself is executed with Boa by the core test;
                // this protocol fake returns the same bounded wire representation.
                let expression = params["expression"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?;
                let limit: usize = expression
                    .split("const limit=")
                    .nth(1)
                    .and_then(|tail| tail.split(';').next())
                    .and_then(|bound| bound.parse().ok())
                    .ok_or(BrowserError::Protocol)?;
                let mut end = limit.min(page.text.len());
                while !page.text.is_char_boundary(end) {
                    end -= 1;
                }
                json!({"result":{"value":{"text":page.text[..end],"truncated":end<page.text.len()}}})
            }
            _ => json!({}),
        };
        Ok(CdpReply {
            result,
            sequence: self.sequence,
            document_epoch: self.epoch,
            events,
        })
    }

    fn drain_events(&mut self) -> Result<Vec<Value>, BrowserError> {
        self.cancellation.check()?;
        let page = self.page.lock().expect("page lock poisoned");
        let mut events = Vec::new();
        if self.navigated && page.late_request && !self.late_sent && self.clock.now_ms() >= 10 {
            self.late_sent = true;
            events.push(Self::event(
                "Network.requestWillBeSent",
                json!({
                    "requestId":"late","frameId":"frame-1","loaderId":"loader-1",
                    "type":"Fetch","request":{"url":"https://example.com/late"}
                }),
            ));
        }
        if self.late_sent && !self.late_finished && self.clock.now_ms() >= 100 {
            self.late_finished = true;
            events.push(Self::event(
                "Network.loadingFinished",
                json!({"requestId":"late"}),
            ));
        }
        if self.navigated && page.popups && !self.popup_sent {
            self.popup_sent = true;
            self.epoch += 1;
            events.push(Self::popup(false, "async-popup"));
        }
        Ok(events)
    }
}

pub(super) struct Rig {
    pub(super) root: tempfile::TempDir,
    pub(super) host: ManagedBrowserHost,
    pub(super) facts: Arc<Mutex<Facts>>,
    pub(super) page: Arc<Mutex<Page>>,
    pub(super) clock: Arc<Clock>,
    pub(super) context: BrowserCallContext,
    pub(super) bundle: ConfigBundle,
    invalid_containment: Arc<AtomicBool>,
    admitting: Arc<Mutex<Option<StopPoint>>>,
    pub(super) starting: Arc<Mutex<Option<StopPoint>>>,
    pub(super) stopping: Arc<Mutex<Option<StopPoint>>>,
    pub(super) pause: Arc<Mutex<Option<StopPoint>>>,
}

struct Startup {
    bundle: ConfigBundle,
    invalid_containment: Arc<AtomicBool>,
}

impl BrowserStartup for Startup {
    fn config(&self) -> BrowserConfig {
        self.bundle.config.tools.browser.clone()
    }
    fn gate(&self, _generation: u64) -> Result<ProcessGateInput, BrowserError> {
        let mut input = existing::input(&self.bundle)?;
        if self.invalid_containment.load(Ordering::SeqCst) {
            input.containment_proof = ProcessContainmentProofCandidate::Missing;
        }
        Ok(input)
    }
    fn network_guard(&self) -> NetworkGuard {
        NetworkGuard::with_ssrf_whitelist(&self.bundle.config.tools.ssrf_whitelist)
    }
}

impl Rig {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        Self::with_profile(browser::ProfileUse::Saved)
    }

    pub(super) fn with_profile(profile: browser::ProfileUse) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let mut bundle = existing::bundle(root.path(), PermissionMode::Default)?;
        bundle.config.tools.browser.persist_login = profile == browser::ProfileUse::Saved;
        bundle.config.tools.ssrf_whitelist = vec!["127.0.0.1/32".to_owned()];
        let input = existing::input(&bundle)?;
        let context = BrowserCallContext {
            effective_session_id: "cli:conversation".to_owned(),
            permission_mode_snapshot: input.envelope.action.permission_mode_snapshot.clone(),
            permission_interactive: true,
            browser_screenshots: Some(false),
            native_image_input_supported: false,
            media_root: None,
        };
        let facts = Arc::new(Mutex::new(Facts::default()));
        let page = Arc::new(Mutex::new(Page::default()));
        let clock = Arc::new(Clock::default());
        let invalid_containment = Arc::new(AtomicBool::new(false));
        let admitting = Arc::new(Mutex::new(None));
        let starting = Arc::new(Mutex::new(None));
        let stopping = Arc::new(Mutex::new(None));
        let pause = Arc::new(Mutex::new(None));
        facts.lock().expect("process facts lock poisoned").devtools =
            Some(Box::new(Peer::new(page.clone(), clock.clone())));
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                bundle: bundle.clone(),
                invalid_containment: invalid_containment.clone(),
            }),
            Box::new(ProcessRunner {
                admitting: admitting.clone(),
                inner: Runner {
                    facts: facts.clone(),
                    clock: clock.clone(),
                    chromium_dead: Arc::new(AtomicBool::new(false)),
                    xvfb_dead: Arc::new(AtomicBool::new(false)),
                    advance_start: 0,
                    advance_read: 0,
                },
                starting: starting.clone(),
                stopping: stopping.clone(),
            }),
            Arc::new(AdvancingClock {
                clock: clock.clone(),
                pause: pause.clone(),
            }),
            Some(BrowserSnapshotDestination {
                data_dir: bundle.context.data_dir.clone(),
                owner: None,
                config_path: bundle.context.config_path.clone(),
                workspace: bundle.context.workspace.clone(),
            }),
            None,
        );
        Ok(Self {
            root,
            host,
            facts,
            page,
            clock,
            context,
            bundle,
            invalid_containment,
            admitting,
            starting,
            stopping,
            pause,
        })
    }

    pub(super) fn next_peer(&self) {
        self.facts
            .lock()
            .expect("process facts lock poisoned")
            .devtools = Some(Box::new(Peer::new(self.page.clone(), self.clock.clone())));
    }

    pub(super) fn folder(&self) -> BrowserFolder {
        BrowserFolder::new(self.root.path())
    }

    pub(super) fn block(&self) -> (mpsc::Receiver<BrowserCancellation>, mpsc::Sender<()>) {
        let (entered, receive) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        self.page.lock().expect("page lock poisoned").block = Some(StopPoint {
            entered,
            release: wait,
        });
        (receive, release)
    }

    pub(super) fn cleaned(&self) {
        let facts = self.facts.lock().expect("process facts lock poisoned");
        assert!(
            facts.roots.iter().all(|root| !root.exists()),
            "temporary profiles survived"
        );
        assert_eq!(
            facts
                .stops
                .iter()
                .filter(|kind| **kind == BrowserChildKind::Chromium)
                .count(),
            facts
                .spawns
                .iter()
                .filter(|spec| spec.kind == BrowserChildKind::Chromium)
                .count()
        );
    }
}

#[cfg(feature = "spec031-test-fixture")]
pub(super) fn isolated(test_name: &str) -> Result<bool, Box<dyn Error>> {
    if std::env::var("SHACS_SPEC051_CASE").as_deref() == Ok(test_name) {
        return Ok(false);
    }
    let result = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", test_name, "--nocapture"])
        .env("SHACS_SPEC051_CASE", test_name)
        .output()?;
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(true)
}

#[test]
fn watch_start_uses_real_process_gate_without_provider() -> TestResult {
    for case in ["allow", "disabled", "missing-enable", "deny", "containment"] {
        let rig = Rig::new()?;
        let mut config: Value =
            serde_json::from_slice(&fs::read(&rig.bundle.context.config_path)?)?;
        match case {
            "disabled" => config["tools"]["browser"]["enable"] = json!(false),
            "missing-enable" => {
                config["tools"]["browser"]
                    .as_object_mut()
                    .unwrap()
                    .remove("enable");
            }
            "deny" => config["permissions"]["deny"] = json!(["browser_startup"]),
            "containment" => rig.invalid_containment.store(true, Ordering::SeqCst),
            "allow" => {}
            _ => unreachable!(),
        }
        fs::write(
            &rig.bundle.context.config_path,
            serde_json::to_vec(&config)?,
        )?;
        let result = rig.host.read(&RenderedWebRequest {
            url: URL,
            limits: &WebFetchLimits::PRODUCTION,
            use_saved_login: false,
        });
        if case == "allow" {
            result?;
            assert_eq!(
                rig.facts
                    .lock()
                    .unwrap()
                    .spawns
                    .iter()
                    .filter(|spec| spec.kind == BrowserChildKind::Chromium)
                    .count(),
                1
            );
            assert_eq!(rig.host.receipt()?.ok_or("receipt")?.dispatch_count, 1);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.failure, WebCheckFailure::BrowserUnavailable, "{case}");
            assert_eq!(
                error.browser_reason,
                Some(BrowserUnavailableReason::PermissionDenied),
                "{case}"
            );
            assert!(rig.facts.lock().unwrap().spawns.is_empty(), "{case}");
        }
        rig.cleaned();
    }
    Ok(())
}

#[test]
fn rendered_read_waits_for_its_loader_and_keeps_http_facts() -> TestResult {
    let rig = Rig::new()?;
    rig.page.lock().unwrap().late_request = true;
    let page = rig.host.read(&RenderedWebRequest {
        url: URL,
        limits: &WebFetchLimits::PRODUCTION,
        use_saved_login: false,
    })?;
    assert_eq!(page.http_status, 200);
    assert_eq!(page.text, "Price: 12900 USD");
    assert_eq!(rig.page.lock().unwrap().evaluated_at, [600]);
    for status in [429, 503] {
        rig.next_peer();
        let mut state = rig.page.lock().unwrap();
        state.status = status;
        state.fault = Fault::Pending;
        state.evaluated_at.clear();
        drop(state);
        let page = rig.host.read(&RenderedWebRequest {
            url: URL,
            limits: &WebFetchLimits::PRODUCTION,
            use_saved_login: false,
        })?;
        assert_eq!(page.http_status, status);
        assert_eq!(page.retry_after.as_deref(), Some("3600"));
        assert!(page.text.is_empty());
        assert!(
            rig.page.lock().unwrap().evaluated_at.is_empty(),
            "HTTP error must not wait for idle or extract"
        );
    }
    rig.cleaned();
    Ok(())
}

#[test]
fn rendered_read_discards_changed_or_unfinished_documents() -> TestResult {
    for fault in [
        Fault::Loader,
        Fault::Scheme,
        Fault::WrongResponse,
        Fault::WrongRequest,
        Fault::OldLoad,
        Fault::Pending,
        Fault::Epoch,
    ] {
        let rig = Rig::new()?;
        rig.page.lock().unwrap().fault = fault;
        let error = rig
            .host
            .read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
            .unwrap_err();
        assert_eq!(error.failure, WebCheckFailure::NetworkError);
        assert!(error.response.is_none_or(|page| page.text.is_empty()));
        rig.cleaned();
    }
    Ok(())
}

#[test]
fn rendered_text_limit_marks_truncation_without_splitting_utf8() -> TestResult {
    let rig = Rig::new()?;
    let text = format!("{}한😀끝", "x".repeat(1024 * 1024 - 4));
    rig.page.lock().unwrap().text = text;
    let page = rig.host.read(&RenderedWebRequest {
        url: URL,
        limits: &WebFetchLimits::PRODUCTION,
        use_saved_login: false,
    })?;
    assert_eq!(page.text.len(), 1024 * 1024 - 1);
    assert!(page.text.ends_with('한'));
    assert!(page.truncated);
    rig.cleaned();
    Ok(())
}

#[test]
fn watch_uses_public_proxy_temporary_profile_and_closes_popups() -> TestResult {
    use shacs_cli::browser::{BrowserConnector, BrowserResolver};
    use std::io::{self, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};

    let rig = Rig::new()?;
    fs::create_dir_all(rig.folder().profile())?;
    fs::write(rig.folder().profile().join("saved-cookie"), b"keep-me")?;
    rig.page.lock().unwrap().popups = true;
    let (entered, release) = rig.block();
    std::thread::scope(|scope| -> TestResult {
        let read = scope.spawn(|| {
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
        });
        entered.recv_timeout(Duration::from_secs(5))?;
        let probe = (|| -> TestResult {
            let proxy = {
                let facts = rig.facts.lock().unwrap();
                facts
                    .spawns
                    .iter()
                    .flat_map(|spec| &spec.arguments)
                    .find_map(|argument| argument.strip_prefix("--proxy-server=socks5://"))
                    .ok_or("proxy address")?
                    .to_owned()
            };
            // Exercise the guard installed in this generation, not a separately
            // constructed default guard that could hide incorrect manager wiring.
            let private = TcpListener::bind("127.0.0.1:0")?;
            let mut stream = TcpStream::connect(proxy)?;
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut request = vec![5, 1, 0, 5, 1, 0, 1, 127, 0, 0, 1];
            request.extend(private.local_addr()?.port().to_be_bytes());
            // Darwin accepted sockets inherit the listener's nonblocking flag.
            // Supply this fixed request in one write; production runs on Linux.
            stream.write_all(&request)?;
            let mut greeting = [0; 2];
            stream
                .read_exact(&mut greeting)
                .map_err(|error| format!("SOCKS greeting: {error}"))?;
            assert_eq!(greeting, [5, 0]);
            let mut reply = [0; 10];
            stream
                .read_exact(&mut reply)
                .map_err(|error| format!("SOCKS connect: {error}"))?;
            assert_eq!(
                reply[1], 2,
                "user whitelist must not admit a private watch request"
            );
            Ok(())
        })();
        release.send(())?;
        let failed = read.join().unwrap().unwrap_err();
        assert_eq!(
            failed.failure,
            WebCheckFailure::NetworkError,
            "a transport document epoch changed while text was in flight"
        );
        assert!(failed.response.is_none_or(|page| page.text.is_empty()));
        probe
    })?;
    assert_eq!(
        fs::read(rig.folder().profile().join("saved-cookie"))?,
        b"keep-me"
    );
    let facts = rig.facts.lock().unwrap();
    let chromium = facts
        .spawns
        .iter()
        .find(|spec| spec.kind == BrowserChildKind::Chromium)
        .ok_or("Chromium")?;
    assert!(!chromium
        .arguments
        .iter()
        .any(|arg| arg.contains(&rig.folder().profile().to_string_lossy().to_string())));
    assert!(chromium
        .arguments
        .iter()
        .any(|arg| arg.starts_with("--user-data-dir=/tmp/shacs-browser-")));
    drop(facts);
    let state = rig.page.lock().unwrap();
    for id in ["paused-popup", "async-popup"] {
        assert!(state
            .commands
            .iter()
            .any(|(method, params, _)| method == "Target.closeTarget" && params["targetId"] == id));
    }
    assert!(!state.commands.iter().any(|(method, _, session)| {
        method.starts_with("Input.")
            || method == "Page.captureScreenshot"
            || method == "Accessibility.getFullAXTree"
            || (method == "Runtime.runIfWaitingForDebugger"
                && session.as_deref() == Some("paused-popup-session"))
    }));
    assert!(state
        .commands
        .iter()
        .filter(|(method, _, _)| method == "Runtime.evaluate")
        .all(|(_, _, session)| session.as_deref() == Some("session-1")));
    drop(state);
    rig.cleaned();

    struct Resolver(Vec<SocketAddr>);
    impl BrowserResolver for Resolver {
        fn resolve(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
            Ok(self.0.clone())
        }
    }
    #[derive(Default)]
    struct Connector(Mutex<Vec<SocketAddr>>);
    impl BrowserConnector for Connector {
        type Connection = SocketAddr;
        fn connect(&self, address: SocketAddr) -> io::Result<SocketAddr> {
            self.0.lock().unwrap().push(address);
            Ok(address)
        }
    }
    let public: SocketAddr = "93.184.216.34:443".parse()?;
    let private: SocketAddr = "127.0.0.1:443".parse()?;
    for addresses in [vec![private], vec![public, private], vec![public]] {
        let connector = Connector::default();
        let expected = if addresses.contains(&private) {
            Vec::new()
        } else {
            vec![public]
        };
        let connected = browser::guarded_connect(
            &NetworkGuard::default(),
            &Resolver(addresses),
            &connector,
            "page.example",
            443,
        );
        assert_eq!(*connector.0.lock().unwrap(), expected);
        assert_eq!(connected.is_ok(), !expected.is_empty());
    }
    Ok(())
}

#[test]
fn watch_and_conversation_share_one_slot_without_takeover_waiting() -> TestResult {
    let rig = Rig::new()?;
    let (entered, release) = rig.block();
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let (answered, received) = mpsc::channel();
        let before_takeover = rig.clock.now_ms();
        let host = rig.host.clone();
        scope.spawn(move || {
            let prepare = host.prepare("cli:other", &json!({"action":"open","url":URL}));
            let takeover = host.takeover(&BrowserTakeoverRequest {
                session_id: "cli:other".to_owned(),
                channel: "cli".to_owned(),
                chat_id: "other".to_owned(),
                reply_route: Default::default(),
                tool_call_id: None,
            });
            answered.send((prepare, takeover)).unwrap();
        });
        let (prepare, takeover) = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            rig.clock.now_ms(),
            before_takeover,
            "watch takeover must not poll or reserve"
        );
        assert!(matches!(prepare, Err(BrowserError::Busy)));
        assert!(matches!(
            takeover,
            Err(BrowserTakeoverError::Browser(BrowserError::Busy))
        ));
        assert!(matches!(
            rig.host.acquire(&rig.context),
            Err(BrowserError::Busy)
        ));
        assert_eq!(
            rig.host.close_prepared(&rig.context, "old"),
            Err(BrowserError::Busy)
        );
        assert!(matches!(
            rig.host.capture_prepared(&rig.context, "old"),
            Err(BrowserError::Busy)
        ));
        assert_eq!(
            rig.host.status().unwrap().control,
            BrowserControl::Assistant
        );
        release.send(()).unwrap();
        read.join().unwrap().unwrap();
    });
    rig.next_peer();
    let conversation = rig.host.acquire(&rig.context)?;
    let before = rig.facts.lock().unwrap().spawns.len();
    for (waiting, use_saved_login) in [(false, false), (false, true), (true, false), (true, true)] {
        if waiting {
            rig.host.turn_finished(&rig.context.effective_session_id);
        }
        assert_eq!(
            rig.host
                .read(&RenderedWebRequest {
                    url: URL,
                    limits: &WebFetchLimits::PRODUCTION,
                    use_saved_login
                })
                .unwrap_err()
                .failure,
            WebCheckFailure::BrowserBusy
        );
    }
    assert_eq!(rig.facts.lock().unwrap().spawns.len(), before);
    conversation.close(&rig.context)?;
    rig.next_peer();
    let (starting, resume) = pause_at(&rig.starting);
    std::thread::scope(|scope| {
        let conversation = scope.spawn(|| {
            let session = rig.host.acquire(&rig.context)?;
            session.close(&rig.context)
        });
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        let before = rig.facts.lock().unwrap().spawns.len();
        assert_eq!(
            rig.host
                .read(&RenderedWebRequest {
                    url: URL,
                    limits: &WebFetchLimits::PRODUCTION,
                    use_saved_login: false
                })
                .unwrap_err()
                .failure,
            WebCheckFailure::BrowserBusy,
            "a conversation that is still starting owns the slot"
        );
        assert_eq!(rig.facts.lock().unwrap().spawns.len(), before);
        resume.send(()).unwrap();
        conversation.join().unwrap().unwrap();
    });
    rig.cleaned();
    Ok(())
}

#[test]
fn watch_start_cleanup_panic_and_takeover_reservation_keep_one_occupancy() -> TestResult {
    let rig = Rig::new()?;
    let (admitting, proceed) = pause_at(&rig.admitting);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
        });
        admitting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            rig.host.close_by_user(),
            BrowserCloseOutcome::Scheduled,
            "closing must see the watch before startup availability is inspected"
        );
        proceed.send(()).unwrap();
        assert_eq!(
            read.join().unwrap().unwrap_err().browser_reason,
            Some(BrowserUnavailableReason::Closed)
        );
    });
    assert!(rig.facts.lock().unwrap().spawns.is_empty());
    rig.next_peer();
    let (stopping, finish) = pause_at(&rig.stopping);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
        });
        stopping.recv_timeout(Duration::from_secs(5)).unwrap();
        let active = File::open(rig.folder().active_lock()).unwrap();
        let directory = File::open(rig.folder().root()).unwrap();
        assert!(
            active.try_lock_exclusive().is_err(),
            "active lock escaped cleanup"
        );
        assert!(
            directory.try_lock_exclusive().is_err(),
            "directory lock escaped cleanup"
        );
        assert!(rig
            .facts
            .lock()
            .unwrap()
            .roots
            .iter()
            .any(|root| root.exists()));
        let before_takeover = rig.clock.now_ms();
        let takeover = rig.host.takeover(&BrowserTakeoverRequest {
            session_id: "cli:other".to_owned(),
            channel: "cli".to_owned(),
            chat_id: "other".to_owned(),
            reply_route: Default::default(),
            tool_call_id: None,
        });
        assert!(matches!(
            takeover,
            Err(BrowserTakeoverError::Browser(BrowserError::Busy))
        ));
        assert_eq!(
            rig.clock.now_ms(),
            before_takeover,
            "cleanup must retain watch occupancy"
        );
        assert_eq!(
            rig.host
                .read(&RenderedWebRequest {
                    url: URL,
                    limits: &WebFetchLimits::PRODUCTION,
                    use_saved_login: false
                })
                .unwrap_err()
                .failure,
            WebCheckFailure::BrowserBusy
        );
        finish.send(()).unwrap();
        read.join().unwrap().unwrap();
        active.try_lock_exclusive().unwrap();
        directory.try_lock_exclusive().unwrap();
    });
    rig.cleaned();

    rig.next_peer();
    let (starting, resume) = pause_at(&rig.starting);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
        });
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(rig.host.close_by_user(), BrowserCloseOutcome::Scheduled);
        resume.send(()).unwrap();
        let error = read.join().unwrap().unwrap_err();
        assert_eq!(error.browser_reason, Some(BrowserUnavailableReason::Closed));
    });
    rig.cleaned();
    rig.next_peer();
    rig.page.lock().unwrap().fault = Fault::Panic;
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })
        });
        assert!(read.join().is_err());
    });
    rig.cleaned();
    rig.page.lock().unwrap().fault = Fault::None;
    rig.next_peer();
    rig.host.read(&RenderedWebRequest {
        url: URL,
        limits: &WebFetchLimits::PRODUCTION,
        use_saved_login: false,
    })?;
    assert_eq!(rig.host.pending_closure_notices()?, 0);

    // A takeover that reserved first retains priority while an ordinary
    // conversation operation owns the slot.
    rig.next_peer();
    rig.page.lock().unwrap().block_method = "Page.getFrameTree";
    let conversation = rig.host.acquire(&rig.context)?;
    let (entered, release) = rig.block();
    std::thread::scope(|scope| {
        let read = scope.spawn(|| conversation.read(&rig.context));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let (reserved, proceed) = pause_at(&rig.pause);
        let takeover = scope.spawn(|| {
            rig.host.takeover(&BrowserTakeoverRequest {
                session_id: "cli:other".to_owned(),
                channel: "cli".to_owned(),
                chat_id: "other".to_owned(),
                reply_route: Default::default(),
                tool_call_id: None,
            })
        });
        reserved.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            rig.host
                .read(&RenderedWebRequest {
                    url: URL,
                    limits: &WebFetchLimits::PRODUCTION,
                    use_saved_login: false
                })
                .unwrap_err()
                .failure,
            WebCheckFailure::BrowserBusy
        );
        release.send(()).unwrap();
        assert!(read.join().unwrap().is_err());
        proceed.send(()).unwrap();
        assert!(matches!(
            takeover.join().unwrap(),
            Err(BrowserTakeoverError::ViewUnavailable(_))
        ));
    });
    rig.cleaned();
    Ok(())
}

#[test]
fn forget_waits_for_watch_cleanup_with_one_shared_fifteen_second_budget() -> TestResult {
    use shacs_cli::browser::{ForgetConfirmation, ForgetRequest, ForgetStage};
    use shacs_cli::ScriptedPrompt;

    struct ForgetClock<'a> {
        rig: &'a Rig,
        release: Mutex<Option<mpsc::Sender<()>>>,
        finished: Mutex<mpsc::Receiver<()>>,
        first_advance: u64,
    }
    impl BrowserClock for ForgetClock<'_> {
        fn now_ms(&self) -> u64 {
            self.rig.clock.now_ms()
        }
        fn pause(&self, ms: u64) {
            let release = self.release.lock().unwrap().take();
            if let Some(release) = release {
                self.rig
                    .clock
                    .0
                    .fetch_add(self.first_advance, Ordering::SeqCst);
                self.rig.host.maintenance().unwrap();
                release.send(()).unwrap();
                self.finished
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                self.rig.cleaned();
                // Drive the next virtual-time phase only after the real file
                // locks are available. A clock tick is not an OS descriptor
                // release barrier, especially beside process-spawning tests.
                for path in [
                    self.rig.folder().active_lock(),
                    self.rig.folder().root().to_path_buf(),
                ] {
                    let lock = File::open(path).unwrap();
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        match lock.try_lock_exclusive() {
                            Ok(()) => {
                                FileExt::unlock(&lock).unwrap();
                                break;
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(Instant::now() < deadline, "cleanup retained a file lock");
                                std::thread::yield_now();
                            }
                            Err(error) => panic!("cleanup lock probe failed: {error}"),
                        }
                    }
                }
            } else {
                self.rig.clock.0.fetch_add(ms, Ordering::SeqCst);
            }
        }
    }
    for held_profile in [false, true] {
        let rig = Rig::new()?;
        fs::create_dir_all(rig.folder().profile())?;
        fs::write(
            rig.folder().profile().join("saved-cookie"),
            b"keep until closed",
        )?;
        let (entered, release) = rig.block();
        let (finished, receive) = mpsc::channel();
        std::thread::scope(|scope| {
            let read = scope.spawn(|| {
                let result = rig.host.read(&RenderedWebRequest {
                    url: URL,
                    limits: &WebFetchLimits::PRODUCTION,
                    use_saved_login: false,
                });
                finished.send(()).unwrap();
                result
            });
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
            let profile = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(rig.folder().profile_lock())
                .unwrap();
            if held_profile {
                profile.try_lock_exclusive().unwrap();
            }
            let clock = ForgetClock {
                rig: &rig,
                release: Mutex::new(Some(release)),
                finished: Mutex::new(receive),
                first_advance: if held_profile { 14_900 } else { 1_000 },
            };
            let started = clock.now_ms();
            let runner = Runner {
                facts: rig.facts.clone(),
                clock: rig.clock.clone(),
                chromium_dead: Arc::new(AtomicBool::new(false)),
                xvfb_dead: Arc::new(AtomicBool::new(false)),
                advance_start: 0,
                advance_read: 0,
            };
            let report = browser::forget(
                &ForgetRequest {
                    data_dir: rig.root.path(),
                    confirmation: ForgetConfirmation::Skip,
                    runner: &runner,
                    clock: &clock,
                },
                &mut ScriptedPrompt::answering(None),
            );
            assert_eq!(
                read.join().unwrap().unwrap_err().browser_reason,
                Some(BrowserUnavailableReason::Closed)
            );
            rig.cleaned();
            if held_profile {
                assert_eq!(report.stage, Some(ForgetStage::ProfileLockBusy));
                assert_eq!(clock.now_ms() - started, 15_000);
                assert!(rig.folder().profile().join("saved-cookie").exists());
            } else {
                assert_eq!(report.stage, Some(ForgetStage::Removed));
                assert_eq!(clock.now_ms() - started, 1_000);
                assert!(!rig.folder().profile().exists());
            }
        });
    }
    Ok(())
}

#[test]
fn forget_and_lock_maintenance_interrupt_an_active_watch() -> TestResult {
    for way in ["forget", "replace", "close", "shutdown"] {
        let rig = Rig::new()?;
        let (entered, release) = rig.block();
        std::thread::scope(|scope| {
            let read = scope.spawn(|| {
                rig.host.read(&RenderedWebRequest {
                    url: URL,
                    limits: &WebFetchLimits::PRODUCTION,
                    use_saved_login: false,
                })
            });
            let cancellation = entered.recv_timeout(Duration::from_secs(5)).unwrap();
            let held = if way == "forget" {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(rig.folder().forget_lock())
                    .unwrap();
                file.try_lock_exclusive().unwrap();
                Some(file)
            } else {
                None
            };
            if way == "replace" {
                fs::rename(
                    rig.folder().active_lock(),
                    rig.folder().root().join("old-active.lock"),
                )
                .unwrap();
                File::create(rig.folder().active_lock()).unwrap();
            }
            let shutdown = if way == "shutdown" {
                Some(scope.spawn(|| rig.host.shutdown()))
            } else {
                None
            };
            if shutdown.is_some() {
                let deadline = Instant::now() + Duration::from_secs(2);
                // Observe shutdown's own signal before allowing the fake reply
                // to return. The deadline only bounds failure; no sleep orders it.
                while cancellation.check().is_ok() {
                    assert!(
                        Instant::now() < deadline,
                        "shutdown did not cancel the watch"
                    );
                    std::thread::yield_now();
                }
            }
            if way == "close" {
                assert_eq!(rig.host.close_by_user(), BrowserCloseOutcome::Scheduled);
            } else if shutdown.is_none() {
                rig.clock.0.fetch_add(1000, Ordering::SeqCst);
                rig.host.maintenance().unwrap();
            }
            assert_eq!(cancellation.check(), Err(BrowserError::Cancelled));
            let active_path = if way == "replace" {
                rig.folder().root().join("old-active.lock")
            } else {
                rig.folder().active_lock()
            };
            let active = File::open(active_path).unwrap();
            let directory = File::open(rig.folder().root()).unwrap();
            assert!(
                active.try_lock_exclusive().is_err(),
                "active lock released during read"
            );
            assert!(
                directory.try_lock_exclusive().is_err(),
                "directory lock released during read"
            );
            release.send(()).unwrap();
            let error = read.join().unwrap().unwrap_err();
            assert_eq!(error.failure, WebCheckFailure::BrowserUnavailable);
            assert_eq!(error.browser_reason, Some(BrowserUnavailableReason::Closed));
            if let Some(shutdown) = shutdown {
                shutdown.join().unwrap().unwrap();
            }
            rig.cleaned();
            active.try_lock_exclusive().unwrap();
            directory.try_lock_exclusive().unwrap();
            FileExt::unlock(&active).unwrap();
            FileExt::unlock(&directory).unwrap();
            drop(held);
        });
        if way != "shutdown" {
            rig.next_peer();
            rig.host.read(&RenderedWebRequest {
                url: URL,
                limits: &WebFetchLimits::PRODUCTION,
                use_saved_login: false,
            })?;
            assert_eq!(rig.host.pending_closure_notices()?, 0);
        }
    }
    Ok(())
}

#[cfg(feature = "spec031-test-fixture")]
#[test]
fn gateway_worker_uses_the_production_rendered_reader() -> TestResult {
    if isolated("spec051_rendered_browser::gateway_worker_uses_the_production_rendered_reader")? {
        return Ok(());
    }
    gateway_run(false)
}

#[cfg(feature = "spec031-test-fixture")]
pub(super) fn gateway_run(saved: bool) -> TestResult {
    use shacs_cli::spec051_gateway_fixture::Gateway;
    use shacs_core::runtime::{
        self, ConfirmWebWatchRequest, EntrustedWorkConditions, EntrustedWorkConsentKind,
        EntrustedWorkConsentProof, EntrustedWorkCreateRequest, EntrustedWorkReadRequest,
        ObservationNextWake, WebCheckResult, WebCondition, WebPreviewRequest, WebPreviewTarget,
        WebReadingMode, WEB_WATCH_WORK_KIND,
    };
    use shacs_session::durable_work::ReplayWorkState;
    use shacs_session::{durable_replay, Session, SessionManager};
    let rig = Rig::new()?;
    browser::install_process_browser_fixture(rig.host.clone())?;
    let workspace = &rig.bundle.context.workspace;
    let data = &rig.bundle.context.data_dir;
    SessionManager::new(workspace)?.save(&Session::new("cli:owner"))?;
    let created = runtime::create_entrusted_work(
        workspace,
        EntrustedWorkCreateRequest {
            owner_session_id: "cli:owner".to_owned(),
            goal_text: "watch price".to_owned(),
            turn_budget: 5,
            consent: EntrustedWorkConsentProof {
                kind: EntrustedWorkConsentKind::ExplicitRequest,
                evidence_ref: "user:watch".to_owned(),
                recorded_at: "100".to_owned(),
            },
            conditions: EntrustedWorkConditions {
                instructions: "tell me".to_owned(),
                budget: None,
                prior_decisions: Vec::new(),
                remaining_questions: Vec::new(),
            },
            linked_targets: Vec::new(),
            observed_at: "100".to_owned(),
        },
    )?;
    let work = EntrustedWorkReadRequest {
        owner_session_id: "cli:owner".to_owned(),
        requester_target: "cli:owner".to_owned(),
        work_id: created.work.work_id,
    };
    let guard = NetworkGuard::default();
    let url = "https://93.184.216.34/item";
    let preview = runtime::preview_web_watch(
        workspace,
        &WebPreviewRequest {
            work: work.clone(),
            url: url.to_owned(),
            reading_mode: WebReadingMode::Browser,
            use_saved_login: saved,
            target: WebPreviewTarget::Number {
                label: "Price:".to_owned(),
                decimal_comma: false,
            },
        },
        &guard,
        &WebFetchLimits::PRODUCTION,
        Some(&rig.host),
    )?;
    rig.next_peer();
    runtime::confirm_web_watch(
        workspace,
        ConfirmWebWatchRequest {
            work: work.clone(),
            expected_revision: 1,
            url: url.to_owned(),
            reading_mode: WebReadingMode::Browser,
            use_saved_login: saved,
            candidate_token: preview.candidates[0].token.clone(),
            condition: WebCondition::AtMost {
                value: "11000".to_owned(),
            },
            cadence_seconds: 600,
            now_ms: 1_000,
        },
        &guard,
        &WebFetchLimits::PRODUCTION,
        Some(&rig.host),
    )?;
    rig.next_peer();
    let (entered, release) = rig.block();
    let mut gateway = Gateway::start(rig.bundle.clone())?;
    entered.recv_timeout(Duration::from_secs(5))?;
    if saved {
        assert!(File::open(rig.folder().profile())?
            .try_lock_exclusive()
            .is_err());
        let facts = rig.facts.lock().expect("process facts lock poisoned");
        let latest = facts
            .spawns
            .iter()
            .rev()
            .find(|spec| spec.kind == BrowserChildKind::Chromium)
            .expect("worker launched Chromium");
        assert!(latest.arguments.contains(&format!(
            "--user-data-dir={}",
            rig.folder().profile().display()
        )));
    }
    assert!(matches!(
        gateway
            .host()
            .ok_or("production host")?
            .prepare("cli:conversation", &json!({"action":"open","url":URL})),
        Err(BrowserError::Busy)
    ));
    let replay = durable_replay::evaluate_durable_recovery(
        data.join("runtime/durable-events"),
        data.join("checkpoints"),
    )
    .state
    .ok_or("durable replay")?;
    let leased = replay
        .work
        .items
        .values()
        .find(|item| item.work_kind == WEB_WATCH_WORK_KIND && item.state == ReplayWorkState::Leased)
        .ok_or("leased watch")?;
    assert_eq!(leased.lease_owner_ref.as_deref(), Some(gateway.owner_id()));
    let takeover = gateway.external_takeover()?;
    assert!(
        takeover.contains("busy") || takeover.contains("쓰는 중"),
        "{takeover}"
    );
    release.send(())?;
    gateway.stop()?;
    let status = runtime::read_web_watch(workspace, data, &work)?;
    assert_eq!(status.history.len(), 1);
    assert_eq!(status.plan.as_ref().ok_or("plan")?.use_saved_login, saved);
    assert_eq!(status.history[0].text_version, "rendered_text_v1");
    assert!(matches!(
        status.history[0].result,
        WebCheckResult::Observed { .. }
    ));
    assert!(matches!(
        status.next_wake,
        ObservationNextWake::Queued { .. }
    ));
    assert_eq!(gateway.provider_calls(), 0);
    rig.cleaned();
    Ok(())
}
