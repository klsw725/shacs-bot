//! Spec 050 PRD 010: takeover and return, on the real manager with a fake
//! process runner and a fake DevTools peer. The xauth/Xvfb/Chromium children
//! come from the Spec 049 fakes; this module fakes x11vnc, websockify and
//! websockify's connection processes, and writes down what the manager shows
//! at every step those fakes see.

use super::spec049_browser_runtime::{bundle, input as gate_input, Clock, Facts, Runner, Startup};
use serde_json::{json, Map, Value};
use shacs_cli::browser::{
    self, BrowserCancellation, BrowserChild, BrowserChildKind, BrowserChildSpec, BrowserClock,
    BrowserControl, BrowserEpisodeRecord, BrowserOwnerBinding, BrowserProcessRunner,
    BrowserSnapshotDestination, BrowserState, BrowserStatus, BrowserViewState, BrowserViewer,
    BrowserViewerFactory, CdpReply, DevToolsPeer, ManagedBrowserHost, PausedTargetAdmission,
    ViewerListenAddress, ViewerListener, ViewerRoutes,
};
use shacs_cli::{
    RuntimeOwnerProcessEvidence, RuntimeOwnershipMarker, RuntimeOwnershipState,
    RuntimeOwnershipStatus,
};
use shacs_config::{BrowserViewConfig, ConfigBundle, PermissionMode};
use shacs_core::runtime::{
    BrowserCallContext, BrowserCloseOutcome, BrowserDescriptionState, BrowserError, BrowserHost,
    BrowserPendingResume, BrowserPreparedAction, BrowserRead, BrowserReturnOutcome,
    BrowserReturnTarget, BrowserTakeover, BrowserTakeoverError, BrowserTakeoverRequest,
    BrowserViewDescription, ProcessGateInput,
};
use shacs_core::tools::{
    BrowserHandoffOrigin, BrowserTool, Tool, ToolCallExecutionContext, ToolResult,
};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// The view password of these tests. No test prints it.
pub(super) const SECRET: &str = "view-secret-marker-A1b2";
const PNG_BASE64: &str = "iVBORw0KGgo=";
pub(super) const OWNER: &str = "telegram:chat-owner";
const OTHER: &str = "telegram:chat-other";
const IDLE_MS: u64 = 300_000;
const REPLY_WAIT_MS: u64 = 30 * 60_000;
const PAGE_TIMEOUT_MS: u64 = 30_000;
pub(super) const TAKEOVER_MS: u64 = 15 * 60_000;
/// What a fake adds to the test clock each time the manager asks about a
/// viewer process that will not go away: four questions use up the two
/// seconds the manager allows.
const STOP_STALL_MS: u64 = 500;
/// The same for a viewer process that will not come up within ten seconds.
const START_STALL_MS: u64 = 2_500;

pub(super) const VIEW_ROUTE: &str = "view: 127.0.0.1:5900\n";

pub(super) struct Page {
    url: String,
    loader: String,
    navigations: u64,
    pub(super) methods: Vec<String>,
    pub(super) inserted: Vec<String>,
    pending_events: Vec<Value>,
    unreadable: bool,
    /// The one element of the page is a password field.
    pub(super) password_focus: bool,
    during_read: Option<Box<dyn FnOnce() + Send>>,
}

impl Page {
    fn shared(url: &str) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            url: url.to_owned(),
            loader: "loader-1".to_owned(),
            navigations: 0,
            methods: Vec::new(),
            inserted: Vec::new(),
            pending_events: Vec::new(),
            unreadable: false,
            password_focus: false,
            during_read: None,
        }))
    }

    fn moved(&mut self, url: &str) {
        self.navigations += 1;
        self.url = url.to_owned();
        self.loader = format!("loader-moved-{}", self.navigations);
        self.pending_events.push(
            json!({"method":"Page.frameNavigated","sessionId":"session-1","params":{"frame":{
            "id":"frame-1","loaderId":self.loader,"url":self.url}}}),
        );
    }
}

struct Tab {
    page: Arc<Mutex<Page>>,
    sequence: u64,
}

impl DevToolsPeer for Tab {
    fn set_cancellation(&mut self, _cancellation: BrowserCancellation) {}

    fn request(
        &mut self,
        method: &str,
        params: Value,
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
            "Target.getTargets" => json!({"targetInfos":[{"targetId":"target-1","type":"page"}]}),
            "Target.attachToTarget" => json!({"sessionId":"session-1"}),
            "Page.getFrameTree" => {
                if page.unreadable {
                    return Err(BrowserError::Protocol);
                }
                json!({"frameTree":{"frame":{"id":"frame-1","loaderId":page.loader,"url":page.url}}})
            }
            "Page.navigate" => {
                page.navigations += 1;
                page.url = params["url"].as_str().unwrap_or("").to_owned();
                page.loader = format!("loader-opened-{}", page.navigations);
                json!({"frameId":"frame-1","loaderId":page.loader})
            }
            "Accessibility.getFullAXTree" => {
                let during_read = page.during_read.take();
                drop(page);
                if let Some(during_read) = during_read {
                    during_read();
                }
                json!({"nodes":[{"backendDOMNodeId":1,"role":{"value":"button"},
                    "name":{"value":"결제하기"}}]})
            }
            "Accessibility.getPartialAXTree" => {
                json!({"nodes":[{"backendDOMNodeId":1,"role":{"value":"button"},
                    "name":{"value":"결제하기"}}]})
            }
            "DOM.getDocument" => {
                json!({"root":{"backendNodeId":10,"children":[{"backendNodeId":1}]}})
            }
            "DOM.resolveNode" => json!({"object":{"objectId":"object-1"}}),
            "Runtime.callFunctionOn" if params["returnByValue"] == true => {
                json!({"result":{"value":{"connected":true,"visible":true,"kind":"button",
                    "password":false,"editable":false,"disabled":false,"value":""}}})
            }
            "Runtime.callFunctionOn" => json!({"result":{"subtype":"null"}}),
            "DOM.describeNode" if page.password_focus => json!({"node":{"localName":"input",
                "backendNodeId":1,"attributes":["type","password"]}}),
            "DOM.describeNode" => json!({"node":{"localName":"button","backendNodeId":1}}),
            "DOM.getBoxModel" => json!({"model":{"content":[0,0,100,0,100,30,0,30],
                "border":[0,0,100,0,100,30,0,30]}}),
            "Page.getLayoutMetrics" => {
                json!({"cssVisualViewport":{"clientWidth":1280,"clientHeight":720}})
            }
            "Runtime.evaluate" => {
                json!({"result":{"value":{"text":format!("본문 {}", page.url),"title":"Cart"}}})
            }
            "Page.captureScreenshot" => json!({"data":PNG_BASE64}),
            "Input.insertText" => {
                page.inserted
                    .push(params["text"].as_str().unwrap_or("").to_owned());
                json!({})
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    ViewOnly,
    Input,
    Proxy,
}

struct Proc {
    kind: Kind,
    pid: u32,
    stopped: bool,
    exited: bool,
    listens: bool,
    foreign: bool,
    keeps_file: bool,
    unreapable: bool,
    listener_lingers: bool,
}

impl Proc {
    fn gone(&self) -> bool {
        self.exited || (self.stopped && !self.unreapable)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Route {
    View,
    Paste,
}

/// A connection process of websockify that looked its token up and stopped
/// right before connecting.
struct Worker {
    group: u32,
    route: Route,
    alive: bool,
    immortal: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Accepted {
    listener: Option<u32>,
    through: u32,
    route: Route,
}

#[derive(Default, Clone)]
pub(super) struct Knobs {
    input_foreign_listener: bool,
    input_keeps_password_file: bool,
    input_unreapable: bool,
    input_listener_lingers: bool,
    paste_socket_blocked: bool,
    routes_blocked: bool,
    view_only_unreapable: bool,
    proxy_never_listens: Option<usize>,
    stranger_takes_vnc: bool,
    spawn_advance_ms: u64,
}

#[derive(Clone, PartialEq, Debug)]
enum What {
    Spawned(Kind, u32),
    Stopped(Kind, u32),
    /// The answer to "did it exit" after it was told to stop.
    Reaped(Kind, u32, bool),
    /// The answer to "does its process group still live" after the stop.
    Group(u32, bool),
    Listener(ViewerListenAddress, u16, Option<u32>, ViewerListener),
}

#[derive(Clone, Debug)]
struct Seen {
    what: What,
    user: bool,
    stage: Option<&'static str>,
    input_spawned: bool,
    control: Option<BrowserControl>,
    revision: u64,
    routes: Option<String>,
    paste_socket: bool,
    view_only: bool,
    input: bool,
    proxy: bool,
    proxy_listening: bool,
    workers: usize,
    chromium_stops: usize,
    clock: u64,
}

#[derive(Default)]
struct World {
    knobs: Knobs,
    procs: Vec<Proc>,
    workers: Vec<Worker>,
    accepted: Vec<Accepted>,
    proxies: usize,
    stranger: bool,
    trace: Vec<Seen>,
    root: Option<PathBuf>,
    specs: Vec<(Kind, BrowserChildSpec)>,
    read_files: Vec<(PathBuf, String)>,
    asked: Vec<(ViewerListenAddress, u16)>,
    core_stops: Vec<(BrowserChildKind, u64)>,
    /// Runs once, inside the next stop of websockify.
    when_proxy_stops: Option<Box<dyn FnOnce() + Send>>,
}

impl World {
    fn live(&self, kind: Kind) -> Option<&Proc> {
        self.procs
            .iter()
            .find(|proc| proc.kind == kind && !proc.gone())
    }

    fn end_group(&mut self, group: u32) {
        for worker in &mut self.workers {
            if worker.group == group && !worker.immortal {
                worker.alive = false;
            }
        }
    }

    fn group_alive(&self, group: u32) -> bool {
        self.workers
            .iter()
            .any(|worker| worker.group == group && worker.alive)
    }
}

/// What the fakes share: their own facts, the clock, and the host they
/// watch. The host is asked only after the facts are unlocked.
#[derive(Clone)]
struct Probe {
    world: Arc<Mutex<World>>,
    core: Arc<Mutex<Facts>>,
    host: Arc<Mutex<Option<ManagedBrowserHost>>>,
    clock: Arc<Clock>,
}

impl Probe {
    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().expect("viewer facts")
    }

    fn note(&self, what: What) {
        let host = self.host.lock().ok().and_then(|host| host.clone());
        let records = host.as_ref().map(ManagedBrowserHost::episode_records);
        let open = records.and_then(|records| records.open);
        let control = host
            .as_ref()
            .and_then(|host| host.status().ok())
            .map(|status| status.control);
        let revision = host
            .as_ref()
            .map_or(0, ManagedBrowserHost::control_revision);
        drop(host);
        let chromium_stops = self.core.lock().map_or(0, |core| {
            core.stops
                .iter()
                .filter(|kind| **kind == BrowserChildKind::Chromium)
                .count()
        });
        let mut world = self.world();
        let root = world.root.clone();
        let seen = Seen {
            what,
            user: open.is_some(),
            stage: open.as_ref().map(|open| open.stage),
            input_spawned: open.as_ref().is_some_and(|open| open.input_spawned),
            control,
            revision,
            routes: root
                .as_ref()
                .and_then(|root| fs::read_to_string(root.join(browser::VIEW_ROUTES)).ok()),
            paste_socket: root.as_ref().is_some_and(|root| {
                fs::symlink_metadata(root.join(browser::PASTE_SOCKET))
                    .is_ok_and(|metadata| !metadata.is_dir())
            }),
            view_only: world.live(Kind::ViewOnly).is_some(),
            input: world.live(Kind::Input).is_some(),
            proxy: world.live(Kind::Proxy).is_some(),
            proxy_listening: world.live(Kind::Proxy).is_some_and(|proxy| proxy.listens),
            workers: world.workers.iter().filter(|worker| worker.alive).count(),
            chromium_stops,
            clock: self.clock.0.load(Ordering::SeqCst),
        };
        world.trace.push(seen);
    }
}

struct ViewerProc {
    probe: Probe,
    index: usize,
}

impl BrowserChild for ViewerProc {
    fn process_id(&self) -> Option<u32> {
        Some(self.probe.world().procs[self.index].pid)
    }

    fn exited(&mut self) -> Result<bool, BrowserError> {
        let (kind, pid, stopped, gone) = {
            let world = self.probe.world();
            let proc = &world.procs[self.index];
            (proc.kind, proc.pid, proc.stopped, proc.gone())
        };
        if stopped {
            if !gone {
                self.probe
                    .clock
                    .0
                    .fetch_add(STOP_STALL_MS, Ordering::SeqCst);
            }
            self.probe.note(What::Reaped(kind, pid, gone));
        }
        Ok(gone)
    }

    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        Ok(false)
    }

    fn stop(&mut self) {
        let stopped = {
            let mut world = self.probe.world();
            let root = world.root.clone();
            let proc = &mut world.procs[self.index];
            if proc.stopped {
                None
            } else {
                proc.stopped = true;
                let (kind, pid) = (proc.kind, proc.pid);
                if kind == Kind::Proxy {
                    world.end_group(pid);
                }
                if kind == Kind::Input {
                    if let Some(root) = root {
                        for name in [browser::PASTE_SOCKET, browser::VIEW_ROUTES] {
                            if root.join(name).is_dir() {
                                let _ = fs::remove_dir_all(root.join(name));
                            }
                        }
                    }
                }
                let then = (kind == Kind::Proxy)
                    .then(|| world.when_proxy_stops.take())
                    .flatten();
                Some((kind, pid, then))
            }
        };
        if let Some((kind, pid, then)) = stopped {
            if let Some(then) = then {
                then();
            }
            self.probe.note(What::Stopped(kind, pid));
        }
    }

    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        Err(BrowserError::Protocol)
    }

    fn group_alive(&mut self) -> bool {
        let (pid, stopped, alive) = {
            let world = self.probe.world();
            let proc = &world.procs[self.index];
            (
                proc.pid,
                proc.stopped,
                !proc.gone() || world.group_alive(proc.pid),
            )
        };
        if stopped {
            if alive {
                self.probe
                    .clock
                    .0
                    .fetch_add(STOP_STALL_MS, Ordering::SeqCst);
            }
            self.probe.note(What::Group(pid, alive));
        }
        alive
    }
}

impl Drop for ViewerProc {
    fn drop(&mut self) {
        if let Ok(mut world) = self.probe.world.lock() {
            let proc = &mut world.procs[self.index];
            proc.exited = true;
            let (kind, pid) = (proc.kind, proc.pid);
            if kind == Kind::Proxy {
                world.end_group(pid);
            }
        }
    }
}

/// A Spec 049 fake child that also writes down when it was stopped.
struct CoreProc {
    inner: Box<dyn BrowserChild>,
    kind: BrowserChildKind,
    probe: Probe,
}

impl BrowserChild for CoreProc {
    fn process_id(&self) -> Option<u32> {
        self.inner.process_id()
    }

    fn exited(&mut self) -> Result<bool, BrowserError> {
        self.inner.exited()
    }

    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        self.inner.succeeded()
    }

    fn stop(&mut self) {
        let now = self.probe.clock.0.load(Ordering::SeqCst);
        self.probe.world().core_stops.push((self.kind, now));
        self.inner.stop();
    }

    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        self.inner.take_devtools()
    }

    fn group_alive(&mut self) -> bool {
        self.inner.group_alive()
    }
}

struct StageRunner {
    inner: Runner,
    probe: Probe,
    page: Arc<Mutex<Page>>,
    installed: Arc<AtomicBool>,
    /// The manager's own runner writes down which (address, port) it was
    /// asked about; a second runner a test asks directly does not.
    recorded: bool,
}

impl BrowserProcessRunner for StageRunner {
    fn available(&self) -> bool {
        true
    }

    // Called while the status is being written: it asks the host nothing.
    fn viewer_available(&self) -> bool {
        self.installed.load(Ordering::SeqCst)
    }

    fn display_ready(&self, display: u16, deadline: Instant) -> Result<(), BrowserError> {
        self.inner.display_ready(display, deadline)
    }

    fn viewer_listener(
        &self,
        address: ViewerListenAddress,
        port: u16,
        owner: Option<u32>,
    ) -> ViewerListener {
        let (answer, stall) = {
            let mut world = self.probe.world();
            if self.recorded {
                world.asked.push((address, port));
            }
            if (address, port) == (ViewerListenAddress::Loopback, browser::VNC_PORT) {
                let holder = world.procs.iter().find(|proc| {
                    proc.kind != Kind::Proxy
                        && proc.listens
                        && (!proc.gone() || proc.listener_lingers)
                });
                match holder {
                    Some(proc) if proc.gone() => (ViewerListener::Foreign, STOP_STALL_MS),
                    Some(proc) if proc.foreign || owner != Some(proc.pid) => {
                        (ViewerListener::Foreign, 0)
                    }
                    Some(proc) if proc.keeps_file => (ViewerListener::OwnedByChild, START_STALL_MS),
                    Some(_) => (ViewerListener::OwnedByChild, 0),
                    None if world.stranger => (ViewerListener::Foreign, STOP_STALL_MS),
                    None => {
                        if world.knobs.stranger_takes_vnc
                            && world.procs.iter().any(|proc| proc.kind == Kind::ViewOnly)
                        {
                            // The port was just seen free; the next look
                            // finds the stranger on it.
                            world.stranger = true;
                        }
                        (ViewerListener::Absent, 0)
                    }
                }
            } else if (address, port) == (ViewerListenAddress::Any, browser::VIEWER_PORT) {
                match world.live(Kind::Proxy).filter(|proxy| proxy.listens) {
                    Some(proxy) if owner == Some(proxy.pid) => (ViewerListener::OwnedByChild, 0),
                    Some(_) => (ViewerListener::Foreign, 0),
                    None => {
                        let waiting = owner.is_some_and(|pid| {
                            world
                                .procs
                                .iter()
                                .any(|proc| proc.pid == pid && !proc.gone())
                        });
                        (
                            ViewerListener::Absent,
                            if waiting { START_STALL_MS } else { 0 },
                        )
                    }
                }
            } else {
                (ViewerListener::Absent, 0)
            }
        };
        self.probe.clock.0.fetch_add(stall, Ordering::SeqCst);
        self.probe
            .note(What::Listener(address, port, owner, answer));
        answer
    }

    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        if spec.program != browser::X11VNC && spec.program != browser::WEBSOCKIFY {
            if spec.kind == BrowserChildKind::Chromium {
                let mut core = self
                    .inner
                    .facts
                    .lock()
                    .map_err(|_| BrowserError::StateUnavailable)?;
                core.devtools = Some(Box::new(Tab {
                    page: Arc::clone(&self.page),
                    sequence: 0,
                }));
            }
            return Ok(Box::new(CoreProc {
                inner: self.inner.spawn(spec, deadline)?,
                kind: spec.kind,
                probe: self.probe.clone(),
            }));
        }
        let root = Path::new(&spec.environment["HOME"])
            .parent()
            .ok_or(BrowserError::Protocol)?
            .to_path_buf();
        let (kind, pid, index) = {
            let mut world = self.probe.world();
            world.root = Some(root.clone());
            let pid = 7000 + world.procs.len() as u32;
            let knobs = world.knobs.clone();
            let mut proc = Proc {
                kind: Kind::Proxy,
                pid,
                stopped: false,
                exited: false,
                listens: true,
                foreign: false,
                keeps_file: false,
                unreapable: false,
                listener_lingers: false,
            };
            if spec.program == browser::WEBSOCKIFY {
                world.proxies += 1;
                proc.listens = knobs.proxy_never_listens != Some(world.proxies);
            } else {
                let input = !spec
                    .arguments
                    .iter()
                    .any(|argument| argument == "-viewonly");
                proc.kind = if input { Kind::Input } else { Kind::ViewOnly };
                let path = spec
                    .arguments
                    .iter()
                    .position(|argument| argument == "-passwdfile")
                    .and_then(|index| spec.arguments.get(index + 1))
                    .and_then(|argument| argument.strip_prefix("rm:"))
                    .map(PathBuf::from)
                    .ok_or(BrowserError::Protocol)?;
                let content = fs::read_to_string(&path).map_err(|_| BrowserError::Protocol)?;
                world.read_files.push((path.clone(), content));
                proc.keeps_file = input && knobs.input_keeps_password_file;
                if !proc.keeps_file {
                    fs::remove_file(&path).map_err(|_| BrowserError::Protocol)?;
                }
                if input {
                    proc.foreign = knobs.input_foreign_listener;
                    proc.unreapable = knobs.input_unreapable;
                    proc.listener_lingers = knobs.input_listener_lingers;
                    for (blocked, name) in [
                        (knobs.paste_socket_blocked, browser::PASTE_SOCKET),
                        (knobs.routes_blocked, browser::VIEW_ROUTES),
                    ] {
                        if blocked {
                            // A folder that is not empty: it can be neither
                            // unlinked nor renamed over.
                            let _ = fs::remove_file(root.join(name));
                            fs::create_dir_all(root.join(name).join("held"))
                                .map_err(|_| BrowserError::Protocol)?;
                        }
                    }
                } else {
                    proc.unreapable = knobs.view_only_unreapable;
                }
            }
            let kind = proc.kind;
            world.specs.push((kind, spec.clone()));
            world.procs.push(proc);
            self.probe
                .clock
                .0
                .fetch_add(knobs.spawn_advance_ms, Ordering::SeqCst);
            (kind, pid, world.procs.len() - 1)
        };
        self.probe.note(What::Spawned(kind, pid));
        Ok(Box::new(ViewerProc {
            probe: self.probe.clone(),
            index,
        }))
    }
}

fn view(enable: bool, password: &str, url: &str) -> BrowserViewConfig {
    BrowserViewConfig {
        enable,
        password: password.to_owned(),
        url: url.to_owned(),
    }
}

pub(super) struct Setup {
    pub(super) view: BrowserViewConfig,
    pub(super) knobs: Knobs,
    pub(super) page: &'static str,
    pub(super) state_dir: bool,
    pub(super) enable: bool,
    pub(super) takeover_minutes: u64,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            view: view(true, SECRET, ""),
            knobs: Knobs::default(),
            page: "https://example.com/",
            state_dir: false,
            enable: true,
            takeover_minutes: 15,
        }
    }
}

pub(super) struct Stage {
    pub(super) host: ManagedBrowserHost,
    world: Arc<Mutex<World>>,
    core: Arc<Mutex<Facts>>,
    pub(super) page: Arc<Mutex<Page>>,
    clock: Arc<Clock>,
    handle: Arc<Mutex<Option<ManagedBrowserHost>>>,
    probe: Probe,
    installed: Arc<AtomicBool>,
    resumes: Arc<Mutex<Vec<BrowserPendingResume>>>,
    pub(super) bundle: ConfigBundle,
    owner: Option<RuntimeOwnershipMarker>,
    gate: ProcessGateInput,
    media: PathBuf,
    _root: tempfile::TempDir,
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = self.host.shutdown();
        if let Ok(mut handle) = self.handle.lock() {
            *handle = None;
        }
    }
}

impl Stage {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        Self::with(Setup::default())
    }

    pub(super) fn with(setup: Setup) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let bundle = bundle(root.path(), PermissionMode::Default)?;
        let gate = gate_input(&bundle)?;
        let media = root.path().join("media");
        fs::create_dir_all(&media)?;
        let clock = Arc::new(Clock::default());
        let core = Arc::new(Mutex::new(Facts::default()));
        let page = Page::shared(setup.page);
        let world = Arc::new(Mutex::new(World {
            knobs: setup.knobs,
            ..World::default()
        }));
        let handle = Arc::new(Mutex::new(None));
        let installed = Arc::new(AtomicBool::new(true));
        let probe = Probe {
            world: Arc::clone(&world),
            core: Arc::clone(&core),
            host: Arc::clone(&handle),
            clock: Arc::clone(&clock),
        };
        let owner = setup.state_dir.then(|| RuntimeOwnershipMarker {
            schema_version: 1,
            owner_id: "owner".to_owned(),
            pid: std::process::id(),
            started_at_ms: 0,
            updated_at_ms: 0,
            expires_at_ms: u64::MAX,
            lifecycle: "running".to_owned(),
            binary_version: "test".to_owned(),
            data_schema_version: 1,
            mode: "serve".to_owned(),
            config_path: bundle.context.config_path.to_string_lossy().into_owned(),
            workspace: bundle.context.workspace.to_string_lossy().into_owned(),
            lock_protocol: None,
            process_evidence: RuntimeOwnerProcessEvidence {
                pid: std::process::id(),
                pid_alive: true,
                process_started_after_marker: false,
            },
        });
        let binding = owner
            .as_ref()
            .map(BrowserOwnerBinding::from_marker)
            .transpose()?;
        let mut config = bundle.config.tools.browser.clone();
        config.view = setup.view.clone();
        config.enable = setup.enable;
        config.takeover_minutes = setup.takeover_minutes;
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                config,
                input: gate.clone(),
            }),
            Box::new(StageRunner {
                inner: Runner {
                    facts: Arc::clone(&core),
                    chromium_dead: Arc::new(AtomicBool::new(false)),
                    xvfb_dead: Arc::new(AtomicBool::new(false)),
                    clock: Arc::clone(&clock),
                    advance_start: 0,
                    advance_read: 0,
                },
                probe: probe.clone(),
                page: Arc::clone(&page),
                installed: Arc::clone(&installed),
                recorded: true,
            }),
            Arc::clone(&clock) as Arc<dyn BrowserClock>,
            binding.map(|binding| BrowserSnapshotDestination {
                data_dir: bundle.context.data_dir.clone(),
                owner: Some(binding),
                config_path: bundle.context.config_path.clone(),
                workspace: bundle.context.workspace.clone(),
            }),
            Some(Box::new(BrowserViewerFactory::new(
                setup.view,
                browser::GATEWAY_VIEWER_HOST_PORT,
            ))),
        );
        *handle.lock().map_err(|_| "host handle")? = Some(host.clone());
        let resumes = Arc::new(Mutex::new(Vec::new()));
        let told = Arc::clone(&resumes);
        host.set_resume_notifier(move |resume| {
            told.lock()
                .map(|mut told| told.push(resume.clone()))
                .is_ok()
        });
        Ok(Self {
            host,
            world,
            core,
            page,
            clock,
            handle,
            probe,
            installed,
            resumes,
            bundle,
            owner,
            gate,
            media,
            _root: root,
        })
    }

    fn asking_runner(&self) -> StageRunner {
        StageRunner {
            inner: Runner {
                facts: Arc::clone(&self.core),
                chromium_dead: Arc::new(AtomicBool::new(false)),
                xvfb_dead: Arc::new(AtomicBool::new(false)),
                clock: Arc::clone(&self.clock),
                advance_start: 0,
                advance_read: 0,
            },
            probe: self.probe.clone(),
            page: Arc::clone(&self.page),
            installed: Arc::clone(&self.installed),
            recorded: false,
        }
    }

    fn call(&self, session: &str) -> BrowserCallContext {
        BrowserCallContext {
            effective_session_id: session.to_owned(),
            permission_mode_snapshot: self.gate.envelope.action.permission_mode_snapshot.clone(),
            permission_interactive: true,
            browser_screenshots: Some(false),
            native_image_input_supported: false,
            media_root: Some(self.media.clone()),
        }
    }

    fn prepare(
        &self,
        session: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        calm(|| self.host.prepare(session, arguments))
    }

    fn execute(
        &self,
        session: &str,
        arguments: &Value,
        digest: &str,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        let context = self.call(session);
        let browser = calm(|| self.host.acquire(&context))?;
        calm(|| browser.execute_prepared(&context, arguments, digest))
    }

    pub(super) fn run(
        &self,
        session: &str,
        arguments: &Value,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        let prepared = self.prepare(session, arguments)?;
        self.execute(session, arguments, &prepared.digest)
    }

    pub(super) fn look(&self, session: &str) -> Result<BrowserRead, Box<dyn Error>> {
        Ok(self
            .run(session, &json!({"action":"look"}))?
            .ok_or("look reads the page")?)
    }

    pub(super) fn take(&self) -> Result<BrowserTakeover, BrowserTakeoverError> {
        self.host.takeover(&request(OWNER, None))
    }

    pub(super) fn give_back(&self, takeover: &BrowserTakeover) -> BrowserReturnOutcome {
        self.host
            .return_takeover(&BrowserReturnTarget::Label(takeover.label.clone()))
    }

    pub(super) fn now(&self) -> u64 {
        self.clock.0.load(Ordering::SeqCst)
    }

    fn mark(&self) -> usize {
        self.world().trace.len()
    }

    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().expect("viewer facts")
    }

    fn since(&self, mark: usize) -> Vec<Seen> {
        self.world().trace[mark..].to_vec()
    }

    pub(super) fn root(&self) -> Result<PathBuf, Box<dyn Error>> {
        Ok(self.world().root.clone().ok_or("no generation folder")?)
    }

    pub(super) fn routes(&self) -> Option<String> {
        let root = self.world().root.clone()?;
        fs::read_to_string(root.join(browser::VIEW_ROUTES)).ok()
    }

    fn commands(&self) -> usize {
        self.page.lock().expect("page").methods.len()
    }

    fn sent(&self, method: &str) -> usize {
        let page = self.page.lock().expect("page");
        page.methods.iter().filter(|sent| *sent == method).count()
    }

    pub(super) fn user(&self) -> bool {
        self.host.episode_records().open.is_some()
    }

    fn started(&self, kind: BrowserChildKind) -> usize {
        let core = self.core.lock().expect("facts");
        core.spawns.iter().filter(|spec| spec.kind == kind).count()
    }

    fn stopped(&self, kind: BrowserChildKind) -> usize {
        let core = self.core.lock().expect("facts");
        core.stops.iter().filter(|stop| **stop == kind).count()
    }

    fn spawned(&self, kind: Kind) -> usize {
        let world = self.world();
        world
            .specs
            .iter()
            .filter(|(spawned, _)| *spawned == kind)
            .count()
    }

    fn alive(&self, kind: Kind) -> Option<u32> {
        self.world().live(kind).map(|proc| proc.pid)
    }

    /// The live process of `kind` dies on its own.
    fn kill(&self, kind: Kind) {
        let mut world = self.world();
        let dead = world
            .procs
            .iter_mut()
            .find(|proc| proc.kind == kind && !proc.gone())
            .map(|proc| {
                proc.exited = true;
                proc.pid
            });
        if let (Kind::Proxy, Some(pid)) = (kind, dead) {
            world.end_group(pid);
        }
    }

    /// The test clock when Chromium was stopped, which is when the
    /// generation closed.
    fn closed_at(&self) -> Option<u64> {
        let world = self.world();
        world
            .core_stops
            .iter()
            .find(|(kind, _)| *kind == BrowserChildKind::Chromium)
            .map(|(_, at)| *at)
    }

    pub(super) fn records(&self) -> browser::BrowserEpisodeRecords {
        self.host.episode_records()
    }

    fn acknowledge(&self, session: &str) -> Option<BrowserError> {
        let notice = self.host.closure_notice(session)?;
        self.host.acknowledge_closure_notice(session, &notice.token);
        Some(notice.error)
    }

    /// Runs an action through the `browser` tool, as a turn does.
    fn via_tool(
        &self,
        session: &str,
        action: &Value,
        digest: &str,
    ) -> Result<String, Box<dyn Error>> {
        let tool = BrowserTool::new(Arc::new(self.host.clone()));
        let mut execution =
            ToolCallExecutionContext::default().with_browser_context(Some(self.call(session)));
        execution.browser_binding_digest = Some(digest.to_owned());
        let arguments = action.as_object().ok_or("arguments")?;
        let mut text = String::new();
        for _ in 0..20 {
            text = tool_text(tool.execute_with_context(arguments.clone(), &execution));
            if !text.starts_with("browser_busy") {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(text)
    }

    pub(super) fn two_routes(&self) -> Result<String, Box<dyn Error>> {
        Ok(browser::view_routes(&self.root()?, ViewerRoutes::ViewAndPaste).ok_or("routes")?)
    }

    fn resumes(&self) -> Vec<BrowserPendingResume> {
        self.resumes.lock().expect("resumes").clone()
    }

    fn stalled_worker(&self, route: Route, immortal: bool) -> Result<usize, Box<dyn Error>> {
        let mut world = self.world();
        let group = world
            .live(Kind::Proxy)
            .map(|proxy| proxy.pid)
            .ok_or("no websockify")?;
        world.workers.push(Worker {
            group,
            route,
            alive: true,
            immortal,
        });
        Ok(world.workers.len() - 1)
    }

    fn wake(&self, worker: usize) -> Result<Accepted, &'static str> {
        let mut world = self.world();
        let (group, route) = match world.workers.get(worker) {
            Some(worker) if worker.alive => (worker.group, worker.route),
            _ => return Err("no such process"),
        };
        let accepted = Accepted {
            listener: match route {
                Route::View => world
                    .procs
                    .iter()
                    .find(|proc| proc.kind != Kind::Proxy && proc.listens && !proc.gone())
                    .map(|proc| proc.pid),
                Route::Paste => None,
            },
            through: group,
            route,
        };
        world.accepted.push(accepted);
        Ok(accepted)
    }

    fn connect(&self, route: Route) -> Result<Accepted, Box<dyn Error>> {
        let worker = self.stalled_worker(route, false)?;
        Ok(self.wake(worker)?)
    }

    /// Lets the manager's upkeep act on the clock as it is now.
    pub(super) fn upkeep(&self) {
        for _ in 0..6 {
            let _ = self.host.maintenance();
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub(super) fn set_clock(&self, now: u64) {
        self.clock.0.store(now, Ordering::SeqCst);
    }

    /// Runs `then` inside the next stop of websockify, which the manager
    /// asks for with the browser slot held.
    pub(super) fn when_proxy_stops(&self, then: impl FnOnce() + Send + 'static) {
        self.world().when_proxy_stops = Some(Box::new(then));
    }

    fn takeover_deadline(&self) -> Result<u64, Box<dyn Error>> {
        Ok(self
            .host
            .status()?
            .takeover_deadline_ms
            .ok_or("no takeover deadline")?)
    }

    fn last_reason(&self) -> Option<String> {
        self.records()
            .settled
            .last()
            .map(|record| record.reason.clone())
    }

    /// What `runtime inspect` reads for the owner in `owner_id`.
    pub(super) fn inspected(&self, owner_id: &str) -> Result<BrowserStatus, Box<dyn Error>> {
        let mut marker = self.owner.clone().ok_or("no state directory")?;
        marker.owner_id = owner_id.to_owned();
        let ownership = RuntimeOwnershipStatus {
            state: RuntimeOwnershipState::Active,
            marker: Some(marker),
            reason: "fixture".to_owned(),
        };
        let now = self.now();
        Ok(browser::read_browser_snapshot(
            &self.bundle.context.data_dir,
            &ownership,
            &self.bundle.context.config_path,
            &self.bundle.context.workspace,
            BrowserStatus::new(true, true, now).with_takeover(15),
            now,
        ))
    }

    pub(super) fn settled(
        &self,
        done: impl Fn(&BrowserStatus) -> bool,
    ) -> Result<BrowserStatus, Box<dyn Error>> {
        let mut status = self.host.status()?;
        eventually(|| {
            let _ = self.host.maintenance();
            match self.host.status() {
                Ok(current) => {
                    status = current;
                    done(&status)
                }
                Err(_) => false,
            }
        })?;
        Ok(status)
    }
}

fn request(session: &str, tool_call: Option<&str>) -> BrowserTakeoverRequest {
    BrowserTakeoverRequest {
        session_id: session.to_owned(),
        channel: "telegram".to_owned(),
        chat_id: session.rsplit(':').next().unwrap_or(session).to_owned(),
        reply_route: Map::new(),
        tool_call_id: tool_call.map(str::to_owned),
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

pub(super) fn eventually(mut done: impl FnMut() -> bool) -> Result<(), Box<dyn Error>> {
    for _ in 0..400 {
        if done() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err("the browser manager did not settle".into())
}

pub(super) fn open() -> Value {
    json!({"action":"open","url":"https://example.com/"})
}

pub(super) fn taken(
    result: Result<BrowserTakeover, BrowserTakeoverError>,
) -> Result<BrowserTakeover, String> {
    result.map_err(|error| format!("takeover refused: {error:?}"))
}

fn tool_text(result: ToolResult) -> String {
    match result {
        ToolResult::Json(value) => value[0]["text"].as_str().unwrap_or("").to_owned(),
        other => other.into_text(),
    }
}

fn step(
    steps: &[Seen],
    from: usize,
    name: &str,
    matches: impl Fn(&What) -> bool,
) -> Result<usize, String> {
    steps
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, seen)| matches(&seen.what))
        .map(|(index, _)| index)
        .ok_or_else(|| format!("no step: {name}"))
}

fn x11vnc_step(what: &What) -> bool {
    matches!(
        what,
        What::Spawned(Kind::ViewOnly | Kind::Input, _)
            | What::Stopped(Kind::ViewOnly | Kind::Input, _)
    )
}

/// PRD 010 scope 4: at no step may the assistant hold control while an
/// x11vnc that passes input on is alive.
fn assert_input_never_outlives_user_control(steps: &[Seen]) {
    for seen in steps {
        assert!(
            seen.user || !seen.input,
            "assistant control with a live input viewer at {:?}",
            seen.what
        );
        assert!(
            seen.control != Some(BrowserControl::Assistant) || !seen.input || seen.user,
            "status says assistant with a live input viewer at {:?}",
            seen.what
        );
    }
}

fn assert_ports_asked_by_their_address(stage: &Stage) {
    let world = stage.world();
    assert!(!world.asked.is_empty());
    for (address, port) in &world.asked {
        match *port {
            browser::VIEWER_PORT => assert_eq!(*address, ViewerListenAddress::Any),
            browser::VNC_PORT => assert_eq!(*address, ViewerListenAddress::Loopback),
            other => panic!("the manager asked about port {other}"),
        }
    }
}

#[test]
fn listener_check_reads_bind_address_port_and_socket_inode() -> Result<(), Box<dyn Error>> {
    use ViewerListenAddress::{Any, Loopback};
    use ViewerListener::{Absent, Foreign, OwnedByChild};
    const HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";
    let row = |local: &str, state: &str, uid: u32, timeout: u32, inode: u32| {
        format!(
            "   0: {local} 00000000:0000 {state} 00000000:00000000 00:00000000 00000000 {uid:>5} {timeout:>8} {inode} 1 0000000000000000 100 0 0 10 0"
        )
    };
    let table = |rows: &[String]| format!("{HEADER}\n{}\n", rows.join("\n"));
    let links = |inode: u32| vec!["/dev/null".to_owned(), format!("socket:[{inode}]")];
    let check = |address, port, table: &str, links: Option<&[String]>| {
        browser::viewer_listener_from(address, port, table, links)
    };

    let every = table(&[row("00000000:17C0", "0A", 0, 0, 4242)]);
    assert_eq!(check(Any, 6080, &every, Some(&links(4242))), OwnedByChild);
    assert_eq!(check(Loopback, 6080, &every, Some(&links(4242))), Absent);

    let vnc = table(&[row("0100007F:170C", "0A", 0, 0, 5151)]);
    assert_eq!(
        check(Loopback, 5900, &vnc, Some(&links(5151))),
        OwnedByChild
    );
    assert_eq!(check(Any, 6080, &vnc, Some(&links(5151))), Absent);

    let loopback_only = table(&[row("0100007F:17C0", "0A", 0, 0, 4242)]);
    assert_eq!(
        check(Any, 6080, &loopback_only, Some(&links(4242))),
        Foreign
    );

    assert_eq!(check(Any, 6080, &every, Some(&links(4243))), Foreign);
    assert_eq!(check(Any, 6080, &every, None), Foreign);
    assert_eq!(check(Any, 6080, &every, Some(&[])), Foreign);

    let shared = table(&[
        row("00000000:17C0", "0A", 0, 0, 4242),
        row("03FA1EAC:17C0", "0A", 0, 0, 4300),
    ]);
    assert_eq!(check(Any, 6080, &shared, Some(&links(4242))), Foreign);

    let connected = table(&[row("00000000:17C0", "01", 0, 0, 4242)]);
    assert_eq!(check(Any, 6080, &connected, Some(&links(4242))), Absent);
    let other_port = table(&[row("00000000:17C1", "0A", 0, 0, 4242)]);
    assert_eq!(check(Any, 6080, &other_port, Some(&links(4242))), Absent);
    assert_eq!(
        check(Any, 6080, &format!("{HEADER}\n"), Some(&links(4242))),
        Absent
    );
    assert_eq!(
        check(Loopback, 5900, &format!("{HEADER}\n"), Some(&links(4242))),
        Absent
    );

    // The inode is the tenth column: the uid and the timeout before it may
    // hold the same number without making the socket the owner's.
    let lookalike = table(&[row("00000000:17C0", "0A", 4242, 4242, 9001)]);
    assert_eq!(check(Any, 6080, &lookalike, Some(&links(4242))), Foreign);
    assert_eq!(
        check(Any, 6080, &lookalike, Some(&links(9001))),
        OwnedByChild
    );
    let vnc_lookalike = table(&[row("0100007F:170C", "0A", 5151, 5151, 9002)]);
    assert_eq!(
        check(Loopback, 5900, &vnc_lookalike, Some(&links(5151))),
        Foreign
    );

    let stage = Stage::new()?;
    let runner = stage.asking_runner();
    assert_eq!(runner.viewer_listener(Loopback, 5900, None), Absent);
    assert_eq!(runner.viewer_listener(Any, 6080, None), Absent);
    stage.run(OWNER, &open())?;
    let screen = stage.alive(Kind::ViewOnly).ok_or("no x11vnc")?;
    let proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
    assert_eq!(
        runner.viewer_listener(Loopback, 5900, Some(screen)),
        OwnedByChild
    );
    assert_eq!(runner.viewer_listener(Any, 6080, Some(proxy)), OwnedByChild);
    assert_eq!(runner.viewer_listener(Loopback, 5900, Some(proxy)), Foreign);
    assert_eq!(runner.viewer_listener(Any, 6080, Some(screen)), Foreign);
    for owner in [None, Some(screen), Some(proxy)] {
        assert_eq!(runner.viewer_listener(Loopback, 6080, owner), Absent);
        assert_eq!(runner.viewer_listener(Any, 5900, owner), Absent);
    }
    let takeover = taken(stage.take())?;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert_ports_asked_by_their_address(&stage);
    let world = stage.world();
    for port in [browser::VIEWER_PORT, browser::VNC_PORT] {
        assert!(world.asked.iter().any(|(_, asked)| *asked == port));
    }
    Ok(())
}

#[test]
fn takeover_blocks_every_action_without_devtools_traffic() -> Result<(), Box<dyn Error>> {
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let read = stage.look(OWNER)?;
    let element = read.elements.first().ok_or("no numbered element")?.element;
    let actions = [
        json!({"action":"open","url":"https://example.com/next"}),
        json!({"action":"look"}),
        json!({"action":"capture"}),
        json!({"action":"click","element":element,"revision":read.revision}),
        json!({"action":"type","element":element,"revision":read.revision,"text":"hello"}),
        json!({"action":"press","key":"Enter"}),
        json!({"action":"scroll","direction":"down"}),
        json!({"action":"back"}),
        json!({"action":"wait","text":"본문","timeoutMs":1000}),
        json!({"action":"close"}),
    ];
    let mut prepared = Vec::new();
    for action in &actions {
        prepared.push(stage.prepare(OWNER, action)?.digest);
    }

    taken(stage.take())?;
    let tool = BrowserTool::new(Arc::new(stage.host.clone()));
    let refused = BrowserError::UserControl.to_string();
    assert!(refused.starts_with("browser_user_control"));
    let before = stage.commands();
    for (action, digest) in actions.iter().zip(&prepared) {
        for session in [OWNER, OTHER] {
            assert!(
                matches!(
                    tool.prepare_browser_action(session, action),
                    Err(BrowserError::UserControl)
                ),
                "{action}"
            );
            let mut execution =
                ToolCallExecutionContext::default().with_browser_context(Some(stage.call(session)));
            execution.browser_binding_digest = Some(digest.clone());
            let result = tool
                .execute_with_context(action.as_object().ok_or("arguments")?.clone(), &execution);
            assert_eq!(tool_text(result), refused, "{action}");
        }
    }
    for session in [OWNER, OTHER] {
        let handoff = json!({"action":"handoff","reason":"로그인이 필요합니다"});
        let execution = ToolCallExecutionContext::default()
            .with_browser_context(Some(stage.call(session)))
            .with_browser_handoff_origin(Some(BrowserHandoffOrigin {
                tool_call_id: "call-handoff".to_owned(),
                channel: "telegram".to_owned(),
                chat_id: "chat-owner".to_owned(),
                reply_route: Map::new(),
            }));
        let result =
            tool.execute_with_context(handoff.as_object().ok_or("arguments")?.clone(), &execution);
        assert_eq!(tool_text(result), refused);
        assert_eq!(
            stage.host.takeover(&request(session, Some("call-direct"))),
            Err(BrowserTakeoverError::AlreadyTaken)
        );
    }
    let context = stage.call(OWNER);
    assert!(matches!(
        stage.host.acquire(&context),
        Err(BrowserError::UserControl)
    ));
    assert!(matches!(
        stage.host.capture_prepared(&context, &prepared[2]),
        Err(BrowserError::UserControl)
    ));
    assert_eq!(
        stage.host.close_prepared(&context, &prepared[9]),
        Err(BrowserError::UserControl)
    );
    assert_eq!(stage.commands(), before);
    assert!(stage.user());
    assert_eq!(stage.stopped(BrowserChildKind::Chromium), 0);
    assert_eq!(fs::read_dir(&stage.media)?.count(), 0);

    let takeover = stage.host.episode_records().open.ok_or("episode")?;
    assert_eq!(
        stage
            .host
            .return_takeover(&BrowserReturnTarget::Label(takeover.label)),
        BrowserReturnOutcome::Returned
    );
    stage.look(OWNER)?;
    assert!(stage.commands() > before);
    Ok(())
}

#[test]
fn takeover_enters_user_state_before_input_viewer_and_opens_routes_last(
) -> Result<(), Box<dyn Error>> {
    let stage = Stage::with(Setup {
        knobs: Knobs {
            spawn_advance_ms: 250,
            ..Knobs::default()
        },
        ..Setup::default()
    })?;
    stage.run(OWNER, &open())?;
    let old_proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
    let old_screen = stage.alive(Kind::ViewOnly).ok_or("no x11vnc")?;
    let revision = stage.host.control_revision();
    assert!(!stage.user());
    assert_eq!(stage.host.status()?.control, BrowserControl::Assistant);
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));

    let mark = stage.mark();
    let asked_at = stage.now();
    let takeover = taken(stage.take())?;
    let steps = stage.since(mark);
    let two_routes = stage.two_routes()?;

    let first = steps
        .first()
        .ok_or("the takeover touched no viewer process")?;
    assert_eq!(first.what, What::Stopped(Kind::Proxy, old_proxy));
    assert!(first.user);
    assert_eq!(first.stage, Some("opening"));
    assert_eq!(first.control, Some(BrowserControl::User));
    assert_eq!(first.revision, revision + 1);
    assert_eq!(first.routes.as_deref(), Some(""));
    assert!(!first.input_spawned && !first.input && first.view_only);

    let reaped = step(&steps, 0, "websockify reaped", |what| {
        *what == What::Reaped(Kind::Proxy, old_proxy, true)
    })?;
    let group = step(&steps, 0, "websockify group empty", |what| {
        *what == What::Group(old_proxy, false)
    })?;
    let port_free = step(&steps, 0, "viewer port free", |what| {
        *what
            == What::Listener(
                ViewerListenAddress::Any,
                browser::VIEWER_PORT,
                None,
                ViewerListener::Absent,
            )
    })?;
    let confirmed = reaped.max(group).max(port_free);
    let screen_stopped = step(&steps, 0, "view-only x11vnc stopped", |what| {
        *what == What::Stopped(Kind::ViewOnly, old_screen)
    })?;
    assert!(confirmed < screen_stopped);
    assert!(!steps[..screen_stopped]
        .iter()
        .any(|seen| x11vnc_step(&seen.what)));
    for seen in &steps[..=confirmed] {
        assert!(seen.view_only && !seen.input && seen.routes.as_deref() == Some(""));
    }

    let screen_gone = step(&steps, screen_stopped, "view-only x11vnc reaped", |what| {
        *what == What::Reaped(Kind::ViewOnly, old_screen, true)
    })?;
    let vnc_free = step(&steps, screen_gone, "5900 free", |what| {
        *what
            == What::Listener(
                ViewerListenAddress::Loopback,
                browser::VNC_PORT,
                None,
                ViewerListener::Absent,
            )
    })?;
    let input_up = step(&steps, vnc_free, "input x11vnc spawned", |what| {
        matches!(what, What::Spawned(Kind::Input, _))
    })?;
    let What::Spawned(_, input) = steps[input_up].what else {
        return Err("input x11vnc".into());
    };
    let at_input = &steps[input_up];
    assert!(at_input.user && at_input.input_spawned && !at_input.view_only);
    assert_eq!(at_input.stage, Some("opening"));
    assert_eq!(at_input.routes.as_deref(), Some(""));
    assert!(!at_input.paste_socket);

    let input_ready = step(&steps, input_up, "input x11vnc listener", |what| {
        *what
            == What::Listener(
                ViewerListenAddress::Loopback,
                browser::VNC_PORT,
                Some(input),
                ViewerListener::OwnedByChild,
            )
    })?;
    assert_eq!(steps[input_ready].routes.as_deref(), Some(""));
    assert!(!steps[input_ready].paste_socket);
    let proxy_up = step(&steps, input_ready, "new websockify spawned", |what| {
        matches!(what, What::Spawned(Kind::Proxy, _))
    })?;
    let What::Spawned(_, proxy) = steps[proxy_up].what else {
        return Err("new websockify".into());
    };
    assert_ne!(proxy, old_proxy);
    for seen in &steps[input_up..proxy_up] {
        assert!(seen.input && !seen.proxy && !seen.proxy_listening);
        assert_eq!(seen.routes.as_deref(), Some(""));
    }
    let at_proxy = &steps[proxy_up];
    assert_eq!(at_proxy.routes.as_deref(), Some(two_routes.as_str()));
    assert!(at_proxy.paste_socket && at_proxy.input);
    assert_eq!(at_proxy.stage, Some("opening"));

    let proxy_ready = step(&steps, proxy_up, "new websockify listener", |what| {
        *what
            == What::Listener(
                ViewerListenAddress::Any,
                browser::VIEWER_PORT,
                Some(proxy),
                ViewerListener::OwnedByChild,
            )
    })?;
    assert_eq!(steps[proxy_ready].stage, Some("opening"));
    assert!(!steps[proxy_ready + 1..]
        .iter()
        .any(|seen| matches!(seen.what, What::Spawned(..) | What::Stopped(..))));
    for seen in &steps {
        assert!(seen.user && seen.revision == revision + 1);
        assert_eq!(seen.control, Some(BrowserControl::User));
        assert_eq!(seen.chromium_stops, 0);
    }
    assert_input_never_outlives_user_control(&steps);

    let open = stage.host.episode_records().open.ok_or("episode")?;
    assert_eq!(open.stage, "open");
    assert_eq!(open.episode, takeover.episode);
    assert_eq!(open.label, takeover.label);
    assert!(open.input_spawned && !open.paused && !open.asked);
    assert_eq!(takeover.minutes, 15);
    assert_eq!(takeover.view_url, "http://127.0.0.1:6080/shacs-view.html");
    let status = stage.host.status()?;
    assert_eq!(status.control, BrowserControl::User);
    assert_eq!(status.takeover_minutes, 15);
    // The fifteen minutes run from the end of the switch, not from the call.
    assert!(stage.now() >= asked_at + 500);
    assert_eq!(status.takeover_deadline_ms, Some(stage.now() + TAKEOVER_MS));
    assert_eq!(stage.host.control_revision(), revision + 1);
    assert_eq!(stage.routes().as_deref(), Some(two_routes.as_str()));
    Ok(())
}

#[test]
fn viewer_arguments_follow_control_state() -> Result<(), Box<dyn Error>> {
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let root = stage.root()?;
    let two_routes = stage.two_routes()?;
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));

    let watching = Arc::new(AtomicBool::new(true));
    let watcher = {
        let (watching, path) = (Arc::clone(&watching), root.join(browser::VIEW_ROUTES));
        let allowed = [String::new(), VIEW_ROUTE.to_owned(), two_routes.clone()];
        thread::spawn(move || {
            let mut reads = 0_u64;
            let mut torn = Vec::new();
            while watching.load(Ordering::SeqCst) {
                match fs::read_to_string(&path) {
                    Ok(text) if allowed.contains(&text) => reads += 1,
                    Ok(text) => torn.push(text),
                    Err(error) => torn.push(error.to_string()),
                }
            }
            (reads, torn)
        })
    };
    let mut episodes = Vec::new();
    for _ in 0..3 {
        let mark = stage.mark();
        let takeover = taken(stage.take())?;
        assert_eq!(stage.routes().as_deref(), Some(two_routes.as_str()));
        let taking = stage.since(mark);
        let mark = stage.mark();
        assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
        assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));
        episodes.push((taking, stage.since(mark)));
    }
    watching.store(false, Ordering::SeqCst);
    let (reads, torn) = watcher.join().map_err(|_| "routes watcher")?;
    assert!(reads > 0);
    assert_eq!(torn, Vec::<String>::new());

    for (taking, returning) in &episodes {
        for steps in [taking, returning] {
            let stopped = step(steps, 0, "websockify stopped", |what| {
                matches!(what, What::Stopped(Kind::Proxy, _))
            })?;
            let spawned = step(steps, stopped, "websockify spawned", |what| {
                matches!(what, What::Spawned(Kind::Proxy, _))
            })?;
            assert_eq!(steps[stopped].routes.as_deref(), Some(""));
            for seen in &steps[stopped..spawned] {
                assert!(!seen.proxy && !seen.proxy_listening);
                assert_eq!(seen.routes.as_deref(), Some(""));
            }
            assert_ne!(steps[spawned].routes.as_deref(), Some(""));
        }
    }

    {
        let world = stage.world();
        let normal = |spec: &BrowserChildSpec| -> Vec<String> {
            spec.arguments
                .iter()
                .map(|argument| match argument.strip_prefix("rm:") {
                    Some(_) => "rm:<password file>".to_owned(),
                    None => argument.clone(),
                })
                .collect()
        };
        let of = |kind: Kind| -> Vec<&BrowserChildSpec> {
            world
                .specs
                .iter()
                .filter(|(spawned, _)| *spawned == kind)
                .map(|(_, spec)| spec)
                .collect()
        };
        let (view_only, input, proxies) = (of(Kind::ViewOnly), of(Kind::Input), of(Kind::Proxy));
        assert_eq!((view_only.len(), input.len(), proxies.len()), (4, 3, 7));
        let view_arguments = normal(view_only[0]);
        let input_arguments = normal(input[0]);
        assert_eq!(
            view_arguments
                .iter()
                .filter(|argument| *argument != "-viewonly")
                .cloned()
                .collect::<Vec<_>>(),
            input_arguments
        );
        assert_eq!(view_arguments.len(), input_arguments.len() + 1);
        for spec in view_only.iter().chain(&input) {
            assert_eq!(spec.program, browser::X11VNC);
            let arguments = normal(spec);
            assert!(arguments == view_arguments || arguments == input_arguments);
            for always in ["-noremote", "-nosel", "-noclipboard", "-localhost"] {
                assert!(arguments.iter().any(|argument| argument == always));
            }
            let listen = arguments
                .iter()
                .position(|argument| argument == "-listen")
                .ok_or("-listen")?;
            assert_eq!(arguments[listen + 1], "127.0.0.1");
        }
        let expected = browser::websockify_arguments(&root.join(browser::VIEW_ROUTES));
        assert_eq!(
            expected,
            [
                "--web=/usr/share/novnc".to_owned(),
                "--token-plugin=TokenFile".to_owned(),
                format!("--token-source={}", root.join("view-routes").display()),
                "--auth-plugin=shacs_view_auth.SameOrigin".to_owned(),
                "0.0.0.0:6080".to_owned(),
            ]
        );
        for spec in &proxies {
            assert_eq!(spec.program, browser::WEBSOCKIFY);
            assert_eq!(spec.arguments, expected);
            assert!(!spec
                .arguments
                .iter()
                .any(|argument| argument.contains("127.0.0.1:5900")));
        }
        let proxy_pids: Vec<u32> = world
            .procs
            .iter()
            .filter(|proc| proc.kind == Kind::Proxy)
            .map(|proc| proc.pid)
            .collect();
        assert_eq!(proxy_pids.len(), 7);
        assert_eq!(
            proxy_pids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            7
        );

        // One password file per x11vnc, each a new path that is gone again.
        assert_eq!(world.read_files.len(), 7);
        let mut paths = std::collections::BTreeSet::new();
        for (path, content) in &world.read_files {
            assert!(paths.insert(path.clone()));
            assert_eq!(path.parent(), Some(root.as_path()));
            assert!(
                content.strip_suffix('\n') == Some(SECRET),
                "the password file does not hold the view password"
            );
            assert!(!path.exists());
        }
    }
    let mut left: Vec<String> = fs::read_dir(&root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("view-") || name.starts_with("routes-"))
        .collect();
    left.sort();
    assert_eq!(left, ["view-routes"]);
    assert!(!root.join(browser::PASTE_SOCKET).exists());

    let viewer = BrowserViewer::new(&view(true, SECRET, ""), browser::GATEWAY_VIEWER_HOST_PORT);
    assert!(
        !format!("{viewer:?}").contains(SECRET),
        "the viewer's Debug shows the view password"
    );

    // A generation folder with a `:` in its path cannot be named in the
    // token file, so no route text exists for it and the viewer fails to
    // start instead of running websockify on a broken file.
    for routes in [
        ViewerRoutes::Empty,
        ViewerRoutes::View,
        ViewerRoutes::ViewAndPaste,
    ] {
        assert_eq!(
            browser::view_routes(Path::new("/tmp/shacs:browser"), routes),
            None
        );
    }
    assert_eq!(
        browser::view_routes(
            Path::new("/tmp/shacs-browser-0"),
            ViewerRoutes::ViewAndPaste
        )
        .as_deref(),
        Some("view: 127.0.0.1:5900\npaste: unix_socket:/tmp/shacs-browser-0/paste.sock\n")
    );
    assert_eq!(
        browser::view_routes(Path::new("/tmp/shacs-browser-0"), ViewerRoutes::Empty).as_deref(),
        Some("")
    );

    // A viewer that failed offers no route at all.
    stage.kill(Kind::Proxy);
    let status = stage.settled(|status| status.view.state == BrowserViewState::Failed)?;
    assert_eq!(status.state, BrowserState::Running);
    assert_eq!(stage.routes().as_deref(), Some(""));
    assert_eq!(
        stage.take(),
        Err(BrowserTakeoverError::ViewUnavailable(
            "process_exited".to_owned()
        ))
    );

    for (configured, shown) in [
        ("", "http://127.0.0.1:6080/shacs-view.html"),
        (
            "https://view.example.com/",
            "https://view.example.com/shacs-view.html",
        ),
        (
            "http://192.168.0.7:16080",
            "http://192.168.0.7:16080/shacs-view.html",
        ),
    ] {
        let stage = Stage::with(Setup {
            view: view(true, SECRET, configured),
            ..Setup::default()
        })?;
        let takeover = taken(stage.take())?;
        assert_eq!(takeover.view_url, shown);
        assert_eq!(stage.host.status()?.view.url.as_deref(), Some(shown));
    }
    Ok(())
}

#[test]
fn late_websockify_worker_cannot_reach_swapped_listener() -> Result<(), Box<dyn Error>> {
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let old_proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
    let late = stage.stalled_worker(Route::View, false)?;
    let mark = stage.mark();
    let takeover = taken(stage.take())?;
    let steps = stage.since(mark);
    let screen_stopped = step(&steps, 0, "view-only x11vnc stopped", |what| {
        matches!(what, What::Stopped(Kind::ViewOnly, _))
    })?;
    assert_eq!(steps[0].workers, 0);
    assert_eq!(steps[screen_stopped].workers, 0);
    assert_eq!(stage.wake(late), Err("no such process"));
    let input = stage.alive(Kind::Input).ok_or("no input x11vnc")?;
    let proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
    assert_ne!(proxy, old_proxy);
    assert_eq!(
        stage.connect(Route::View)?,
        Accepted {
            listener: Some(input),
            through: proxy,
            route: Route::View,
        }
    );
    assert!(stage
        .world()
        .accepted
        .iter()
        .all(|accepted| accepted.listener != Some(input) || accepted.through == proxy));

    // The same on the way back, with a paste connection stalled next to it.
    let late_view = stage.stalled_worker(Route::View, false)?;
    let late_paste = stage.stalled_worker(Route::Paste, false)?;
    let mark = stage.mark();
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    let steps = stage.since(mark);
    let input_stopped = step(&steps, 0, "input x11vnc stopped", |what| {
        *what == What::Stopped(Kind::Input, input)
    })?;
    assert_eq!(steps[input_stopped].workers, 0);
    assert!(!steps[..input_stopped]
        .iter()
        .any(|seen| x11vnc_step(&seen.what)));
    assert_eq!(stage.wake(late_view), Err("no such process"));
    assert_eq!(stage.wake(late_paste), Err("no such process"));
    let screen = stage.alive(Kind::ViewOnly).ok_or("no x11vnc")?;
    let back_proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
    assert_eq!(stage.connect(Route::View)?.through, back_proxy);
    assert!(stage
        .world()
        .accepted
        .iter()
        .all(|accepted| accepted.listener != Some(screen) || accepted.through == back_proxy));

    // The next episode's paste listener is reached through its own
    // websockify only.
    let next = taken(stage.take())?;
    let next_proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
    assert_eq!(stage.wake(late_paste), Err("no such process"));
    assert_eq!(stage.connect(Route::Paste)?.through, next_proxy);
    assert!(stage
        .world()
        .accepted
        .iter()
        .filter(|accepted| accepted.route == Route::Paste)
        .all(|accepted| accepted.through == next_proxy));
    assert_eq!(stage.give_back(&next), BrowserReturnOutcome::Returned);
    assert_input_never_outlives_user_control(&stage.since(0));

    // A connection process that will not die: the generation closes two
    // seconds later and no x11vnc is touched before that.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let immortal = stage.stalled_worker(Route::View, true)?;
    let (mark, asked_at) = (stage.mark(), stage.now());
    assert_eq!(
        stage.take(),
        Err(BrowserTakeoverError::Failed(
            "input_shutdown_unconfirmed".to_owned()
        ))
    );
    let steps = stage.since(mark);
    assert!(steps
        .iter()
        .any(|seen| seen.clock >= asked_at + browser::VIEWER_STOP_LIMIT_MS));
    for seen in steps
        .iter()
        .filter(|seen| seen.clock < asked_at + browser::VIEWER_STOP_LIMIT_MS)
    {
        assert!(!x11vnc_step(&seen.what), "{:?}", seen.what);
        assert_eq!(seen.chromium_stops, 0);
    }
    assert!(!steps
        .iter()
        .any(|seen| matches!(seen.what, What::Spawned(..))));
    assert_eq!(stage.spawned(Kind::Input), 0);
    assert_eq!(stage.stopped(BrowserChildKind::Chromium), 1);
    assert_eq!(stage.host.status()?.state, BrowserState::Idle);
    assert!(!stage.user());
    assert!(stage.wake(immortal).is_ok());
    assert!(stage
        .world()
        .accepted
        .iter()
        .all(|accepted| accepted.listener.is_none()));

    // Another process takes 5900 right after the view-only x11vnc is gone.
    let stage = Stage::with(Setup {
        knobs: Knobs {
            stranger_takes_vnc: true,
            ..Knobs::default()
        },
        ..Setup::default()
    })?;
    stage.run(OWNER, &open())?;
    let late = stage.stalled_worker(Route::View, false)?;
    let mark = stage.mark();
    assert_eq!(
        stage.take(),
        Err(BrowserTakeoverError::Failed("port_in_use".to_owned()))
    );
    let steps = stage.since(mark);
    let taken_over = step(&steps, 0, "5900 held by a stranger", |what| {
        *what
            == What::Listener(
                ViewerListenAddress::Loopback,
                browser::VNC_PORT,
                None,
                ViewerListener::Foreign,
            )
    })?;
    assert!(!steps[taken_over].proxy && steps[taken_over].workers == 0);
    assert_eq!(stage.spawned(Kind::Input), 0);
    assert_eq!(stage.wake(late), Err("no such process"));
    assert!(stage.world().accepted.is_empty());
    assert!(!stage.user());
    assert_input_never_outlives_user_control(&steps);
    Ok(())
}

fn record(
    takeover_episode: &str,
    reason: &str,
    tool_call: Option<&str>,
    paused: bool,
) -> BrowserEpisodeRecord {
    BrowserEpisodeRecord {
        episode: takeover_episode.to_owned(),
        reason: reason.to_owned(),
        session: OWNER.to_owned(),
        channel: "telegram".to_owned(),
        chat_id: "chat-owner".to_owned(),
        reply_route: Map::new(),
        tool_call_id: tool_call.map(str::to_owned),
        paused,
    }
}

/// What goes wrong, the knob that makes it go wrong, and the reason told.
type InputFault = (&'static str, fn(&mut Knobs), &'static str);

#[test]
fn takeover_failure_after_input_spawn_turns_input_off() -> Result<(), Box<dyn Error>> {
    let faults: [InputFault; 5] = [
        (
            "the 5900 listener is another process's",
            |knobs| knobs.input_foreign_listener = true,
            "port_in_use",
        ),
        (
            "the password file is left unread",
            |knobs| knobs.input_keeps_password_file = true,
            "startup_timeout",
        ),
        (
            "the paste socket cannot be opened",
            |knobs| knobs.paste_socket_blocked = true,
            "paste_socket_unavailable",
        ),
        (
            "view-routes cannot be written",
            |knobs| knobs.routes_blocked = true,
            "view_routes_unavailable",
        ),
        (
            "the new websockify never listens",
            |knobs| knobs.proxy_never_listens = Some(2),
            "startup_timeout",
        ),
    ];
    for (name, fault, reason) in faults {
        for tool_call in [None, Some("call-handoff")] {
            let mut knobs = Knobs::default();
            fault(&mut knobs);
            let stage = Stage::with(Setup {
                knobs,
                ..Setup::default()
            })?;
            stage.run(OWNER, &open())?;
            let old_screen = stage.alive(Kind::ViewOnly).ok_or("no x11vnc")?;
            let old_proxy = stage.alive(Kind::Proxy).ok_or("no websockify")?;
            let revision = stage.host.control_revision();
            let mark = stage.mark();
            assert_eq!(
                stage.host.takeover(&request(OWNER, tool_call)),
                Err(BrowserTakeoverError::Failed(reason.to_owned())),
                "{name}"
            );
            let steps = stage.since(mark);
            assert_input_never_outlives_user_control(&steps);

            let input_up = step(&steps, 0, "input x11vnc spawned", |what| {
                matches!(what, What::Spawned(Kind::Input, _))
            })?;
            let What::Spawned(_, input) = steps[input_up].what else {
                return Err("input x11vnc".into());
            };
            let input_stopped = step(&steps, input_up, "input x11vnc stopped", |what| {
                *what == What::Stopped(Kind::Input, input)
            })?;
            // websockify and the paste socket are gone before the input
            // x11vnc is told to stop.
            assert!(!steps[input_stopped].proxy && !steps[input_stopped].paste_socket);
            let input_gone = step(&steps, input_stopped, "input x11vnc reaped", |what| {
                *what == What::Reaped(Kind::Input, input, true)
            })?;
            let vnc_free = step(&steps, input_gone, "5900 free", |what| {
                *what
                    == What::Listener(
                        ViewerListenAddress::Loopback,
                        browser::VNC_PORT,
                        None,
                        ViewerListener::Absent,
                    )
            })?;
            let screen_up = step(&steps, input_up, "view-only x11vnc spawned", |what| {
                matches!(what, What::Spawned(Kind::ViewOnly, _))
            })?;
            assert!(vnc_free < screen_up, "{name}");
            for seen in &steps[..=vnc_free] {
                assert!(seen.user, "{name}: {:?}", seen.what);
                assert_eq!(seen.control, Some(BrowserControl::User), "{name}");
            }
            for seen in &steps[input_stopped..] {
                assert!(
                    !seen
                        .routes
                        .as_deref()
                        .is_some_and(|routes| routes.contains("paste")),
                    "{name}: {:?}",
                    seen.what
                );
            }
            assert!(!steps[input_stopped..]
                .iter()
                .any(|seen| matches!(seen.what, What::Spawned(Kind::Input, _))));

            assert!(!stage.user(), "{name}");
            let status = stage.host.status()?;
            assert_eq!(status.control, BrowserControl::Assistant, "{name}");
            assert_eq!(status.state, BrowserState::Running, "{name}");
            assert_eq!(status.view.state, BrowserViewState::Open, "{name}");
            assert_eq!(status.takeover_deadline_ms, None);
            assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE), "{name}");
            let screen = stage.alive(Kind::ViewOnly).ok_or("no new x11vnc")?;
            let proxy = stage.alive(Kind::Proxy).ok_or("no new websockify")?;
            assert!(screen != old_screen && proxy != old_proxy, "{name}");
            assert_eq!(stage.alive(Kind::Input), None, "{name}");
            let root = stage.root()?;
            assert!(fs::symlink_metadata(root.join(browser::PASTE_SOCKET)).is_err());
            let mut left: Vec<String> = fs::read_dir(&root)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("view-") || name.starts_with("routes-"))
                .collect();
            left.sort();
            assert_eq!(left, ["view-routes"], "{name}");

            let records = stage.records();
            assert_eq!(records.open, None);
            assert_eq!(records.waiting, [], "{name}");
            assert_eq!(records.settled.len(), 1, "{name}");
            let ended = &records.settled[0];
            assert_eq!(
                *ended,
                record(&ended.episode, "failed", tool_call, false),
                "{name}"
            );
            assert_eq!(stage.resumes(), [], "{name}");
            assert_eq!(stage.host.pending_resume(&ended.episode), None);
            assert_eq!(stage.host.control_revision(), revision + 2);
            assert_eq!(stage.host.closure_notice(OWNER), None);
            assert_eq!(stage.stopped(BrowserChildKind::Chromium), 0);

            let read = stage.look(OWNER)?;
            assert_eq!(read.document.url, "https://example.com/", "{name}");
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    /// The input x11vnc cannot be reaped.
    Unreapable,
    /// The 5900 listener stays after the input x11vnc is gone.
    Listener,
    /// A connection process stays in websockify's process group.
    Worker,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ending {
    Return,
    Limit,
    Answer,
}

#[test]
fn unconfirmed_input_shutdown_disposes_generation() -> Result<(), Box<dyn Error>> {
    let closed = |stage: &Stage, started: u64, name: &str| -> Result<(), Box<dyn Error>> {
        stage.settled(|status| status.state == BrowserState::Idle)?;
        assert_eq!(
            stage.closed_at(),
            Some(started + browser::VIEWER_STOP_LIMIT_MS),
            "{name}"
        );
        assert_eq!(stage.stopped(BrowserChildKind::Chromium), 1, "{name}");
        assert!(!stage.user(), "{name}");
        assert_eq!(stage.host.status()?.control, BrowserControl::Assistant);
        assert_eq!(
            stage.host.closure_notice(OWNER).map(|notice| notice.error),
            Some(BrowserError::Closed),
            "{name}"
        );
        assert_eq!(stage.alive(Kind::ViewOnly), None, "{name}");
        assert_eq!(stage.alive(Kind::Proxy), None, "{name}");
        Ok(())
    };

    for ending in [Ending::Return, Ending::Limit, Ending::Answer] {
        for fault in [Fault::Unreapable, Fault::Listener, Fault::Worker] {
            for handoff in [false, true] {
                if ending == Ending::Answer && !handoff {
                    continue;
                }
                let name = format!("{ending:?} with {fault:?}, handoff {handoff}");
                let stage = Stage::with(Setup {
                    knobs: Knobs {
                        input_unreapable: fault == Fault::Unreapable,
                        input_listener_lingers: fault == Fault::Listener,
                        ..Knobs::default()
                    },
                    ..Setup::default()
                })?;
                stage.run(OWNER, &open())?;
                let tool_call = handoff.then_some("call-handoff");
                let takeover = taken(stage.host.takeover(&request(OWNER, tool_call)))?;
                assert_eq!(stage.records().open.map(|open| open.paused), Some(handoff));
                if fault == Fault::Worker {
                    stage.stalled_worker(Route::View, true)?;
                }
                let mark = stage.mark();
                let started = match ending {
                    Ending::Return => {
                        let started = stage.now();
                        assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
                        started
                    }
                    Ending::Limit => {
                        let deadline = stage
                            .host
                            .status()?
                            .takeover_deadline_ms
                            .ok_or("no takeover deadline")?;
                        stage.clock.0.store(deadline, Ordering::SeqCst);
                        deadline
                    }
                    Ending::Answer => {
                        let started = stage.now();
                        stage
                            .host
                            .finish_handoff(OWNER, "call-handoff", &takeover.episode);
                        started
                    }
                };
                closed(&stage, started, &name)?;
                let steps = stage.since(mark);
                assert_input_never_outlives_user_control(&steps);
                assert!(
                    !steps
                        .iter()
                        .any(|seen| matches!(seen.what, What::Spawned(..))),
                    "{name}"
                );
                if fault == Fault::Worker {
                    for seen in steps
                        .iter()
                        .filter(|seen| seen.clock < started + browser::VIEWER_STOP_LIMIT_MS)
                    {
                        assert!(!x11vnc_step(&seen.what), "{name}: {:?}", seen.what);
                    }
                }
                let records = stage.records();
                assert_eq!(records.open, None, "{name}");
                let ended = record(&takeover.episode, "closed", tool_call, handoff);
                if handoff && ending != Ending::Answer {
                    assert_eq!(records.waiting, [ended], "{name}");
                    assert_eq!(records.settled, [], "{name}");
                    assert_eq!(
                        stage.resumes(),
                        [BrowserPendingResume {
                            episode: takeover.episode.clone(),
                            reason: "closed".to_owned(),
                            session_id: OWNER.to_owned(),
                            tool_call_id: "call-handoff".to_owned(),
                            channel: "telegram".to_owned(),
                            chat_id: "chat-owner".to_owned(),
                            reply_route: Map::new(),
                        }],
                        "{name}"
                    );
                } else {
                    assert_eq!(records.waiting, [], "{name}");
                    assert_eq!(records.settled, [ended], "{name}");
                    assert_eq!(stage.resumes(), [], "{name}");
                }
            }
        }
    }

    // The takeover fails after the input x11vnc is up, and undoing it cannot
    // confirm that the input side is off.
    for fault in [Fault::Unreapable, Fault::Listener] {
        for tool_call in [None, Some("call-handoff")] {
            let name = format!("rollback with {fault:?}, handoff {}", tool_call.is_some());
            let stage = Stage::with(Setup {
                knobs: Knobs {
                    paste_socket_blocked: true,
                    input_unreapable: fault == Fault::Unreapable,
                    input_listener_lingers: fault == Fault::Listener,
                    ..Knobs::default()
                },
                ..Setup::default()
            })?;
            stage.run(OWNER, &open())?;
            let (mark, started) = (stage.mark(), stage.now());
            assert_eq!(
                stage.host.takeover(&request(OWNER, tool_call)),
                Err(BrowserTakeoverError::Failed(
                    "paste_socket_unavailable".to_owned()
                )),
                "{name}"
            );
            closed(&stage, started, &name)?;
            let steps = stage.since(mark);
            assert_input_never_outlives_user_control(&steps);
            assert!(
                !steps.iter().any(|seen| matches!(
                    seen.what,
                    What::Spawned(Kind::ViewOnly | Kind::Proxy, _)
                )),
                "{name}"
            );
            let records = stage.records();
            assert_eq!(
                (records.open, records.waiting),
                (None, Vec::new()),
                "{name}"
            );
            assert_eq!(records.settled.len(), 1, "{name}");
            assert_eq!(
                records.settled[0],
                record(&records.settled[0].episode, "closed", tool_call, false),
                "{name}"
            );
            assert_eq!(stage.resumes(), [], "{name}");
        }
    }

    // The view-only x11vnc cannot be confirmed gone, so no input x11vnc is
    // ever started.
    for tool_call in [None, Some("call-handoff")] {
        let name = format!("view-only unconfirmed, handoff {}", tool_call.is_some());
        let stage = Stage::with(Setup {
            knobs: Knobs {
                view_only_unreapable: true,
                ..Knobs::default()
            },
            ..Setup::default()
        })?;
        stage.run(OWNER, &open())?;
        let (mark, started) = (stage.mark(), stage.now());
        assert_eq!(
            stage.host.takeover(&request(OWNER, tool_call)),
            Err(BrowserTakeoverError::Failed(
                "input_shutdown_unconfirmed".to_owned()
            )),
            "{name}"
        );
        closed(&stage, started, &name)?;
        let steps = stage.since(mark);
        assert!(
            !steps
                .iter()
                .any(|seen| matches!(seen.what, What::Spawned(..))),
            "{name}"
        );
        assert_eq!(stage.spawned(Kind::Input), 0, "{name}");
        let records = stage.records();
        assert_eq!(
            (records.open, records.waiting),
            (None, Vec::new()),
            "{name}"
        );
        assert_eq!(records.settled.len(), 1, "{name}");
        assert_eq!(
            records.settled[0],
            record(&records.settled[0].episode, "closed", tool_call, false),
            "{name}"
        );
        assert_eq!(stage.resumes(), [], "{name}");
    }
    Ok(())
}

#[test]
fn return_invalidates_numbers_and_keeps_page_readable() -> Result<(), Box<dyn Error>> {
    let look = json!({"action":"look"});
    let stale = BrowserError::StaleRevision.to_string();
    assert!(stale.starts_with("browser_stale_revision"));

    // The page does not change at all while the user has it.
    let stage = Stage::new()?;
    let open_without_document = stage.prepare(OWNER, &open())?.digest;
    stage.run(OWNER, &open())?;
    let read = stage.look(OWNER)?;
    let element = read.elements.first().ok_or("no numbered element")?.element;
    let click = json!({"action":"click","element":element,"revision":read.revision});
    let earlier = [
        look.clone(),
        json!({"action":"capture"}),
        json!({"action":"scroll","direction":"down"}),
        json!({"action":"close"}),
        open(),
        click.clone(),
    ];
    let mut digests = Vec::new();
    for action in &earlier {
        digests.push(stage.prepare(OWNER, action)?.digest);
    }
    let page_before = {
        let page = stage.page.lock().map_err(|_| "page")?;
        (page.url.clone(), page.loader.clone(), page.navigations)
    };
    let takeover = taken(stage.take())?;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    {
        let page = stage.page.lock().map_err(|_| "page")?;
        assert_eq!(
            (page.url.clone(), page.loader.clone(), page.navigations),
            page_before
        );
    }
    let commands = stage.commands();
    for (action, digest) in earlier.iter().zip(&digests) {
        assert_eq!(stage.via_tool(OWNER, action, digest)?, stale, "{action}");
    }
    assert_eq!(
        stage.via_tool(OWNER, &open(), &open_without_document)?,
        stale
    );
    assert_eq!(
        stage.execute(OWNER, &click, &digests[5]).map(|_| ()),
        Err(BrowserError::StaleRevision)
    );
    assert!(matches!(
        stage.prepare(OWNER, &click),
        Err(BrowserError::StaleRevision)
    ));
    assert_eq!(stage.commands(), commands);
    assert_eq!(fs::read_dir(&stage.media)?.count(), 0);
    assert_eq!(stage.stopped(BrowserChildKind::Chromium), 0);

    // Prepared again, the same actions run, and the page is read as it is:
    // nothing opens it a second time.
    let navigations = stage.sent("Page.navigate");
    let fresh = stage.prepare(OWNER, &look)?;
    assert_ne!(fresh.digest, digests[0]);
    let reread = stage
        .execute(OWNER, &look, &fresh.digest)?
        .ok_or("look reads the page")?;
    assert_eq!(reread.document, read.document);
    assert_eq!(stage.sent("Page.navigate"), navigations);
    let capture = stage.prepare(OWNER, &earlier[1])?;
    assert!(stage
        .via_tool(OWNER, &earlier[1], &capture.digest)?
        .starts_with("browser_capture: "));
    assert_eq!(fs::read_dir(&stage.media)?.count(), 1);
    stage.run(OWNER, &earlier[2])?;
    let reopened = stage.prepare(OWNER, &open())?;
    assert_ne!(reopened.digest, digests[4]);
    stage.execute(OWNER, &open(), &reopened.digest)?;
    assert_eq!(stage.sent("Page.navigate"), navigations + 1);
    let close = stage.prepare(OWNER, &earlier[3])?;
    assert_eq!(
        stage.via_tool(OWNER, &earlier[3], &close.digest)?,
        "browser_closed"
    );

    // The generation closes while the user has it. A new one, with no
    // document as when `open` was prepared, still refuses that `open`.
    let stage = Stage::with(Setup {
        page: "about:blank",
        ..Setup::default()
    })?;
    let earlier_open = stage.prepare(OWNER, &open())?.digest;
    taken(stage.take())?;
    assert_ne!(stage.host.close_by_user(), BrowserCloseOutcome::NothingOpen);
    stage.settled(|status| status.state == BrowserState::Idle)?;
    assert!(!stage.user());
    stage.acknowledge(OWNER);
    assert_eq!(
        stage.execute(OWNER, &open(), &earlier_open).map(|_| ()),
        Err(BrowserError::StaleRevision)
    );
    assert_eq!(stage.started(BrowserChildKind::Chromium), 2);
    assert_eq!(stage.sent("Page.navigate"), 0);
    stage.run(OWNER, &open())?;
    assert_eq!(
        stage.execute(OWNER, &open(), &earlier_open).map(|_| ()),
        Err(BrowserError::StaleRevision)
    );
    assert_eq!(stage.sent("Page.navigate"), 1);

    // The user went to another page.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let takeover = taken(stage.take())?;
    stage
        .page
        .lock()
        .map_err(|_| "page")?
        .moved("https://example.com/after-login");
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    let navigations = stage.sent("Page.navigate");
    let read = stage.look(OWNER)?;
    assert_eq!(read.document.url, "https://example.com/after-login");
    assert!(read.visible_text.contains("after-login"));
    assert_eq!(stage.sent("Page.navigate"), navigations);

    // The user went nowhere from the blank tab the takeover started on.
    let stage = Stage::with(Setup {
        page: "about:blank",
        ..Setup::default()
    })?;
    let takeover = taken(stage.take())?;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    let commands = stage.commands();
    // Spec 049 answers a `look` with no open document this way.
    assert!(matches!(
        stage.prepare(OWNER, &look),
        Err(BrowserError::DocumentChanged)
    ));
    assert_eq!(stage.commands(), commands);
    assert_eq!(stage.host.status()?.state, BrowserState::Running);
    stage.run(OWNER, &open())?;
    assert_eq!(stage.look(OWNER)?.document.url, "https://example.com/");

    // The tab cannot be read when control comes back.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let takeover = taken(stage.take())?;
    stage.page.lock().map_err(|_| "page")?.unreadable = true;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert!(!stage.user());
    let status = stage.host.status()?;
    assert_eq!(status.control, BrowserControl::Assistant);
    assert_eq!(status.takeover_deadline_ms, None);
    assert_eq!(
        stage.records().settled,
        [record(&takeover.episode, "returned", None, false)]
    );
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));
    assert_eq!(stage.alive(Kind::Input), None);
    assert!(stage.alive(Kind::ViewOnly).is_some());
    Ok(())
}

#[test]
fn image_carries_view_page_and_origin_check() -> Result<(), Box<dyn Error>> {
    let dockerfile = include_str!("../../../../Dockerfile");
    let page = include_str!("../../../../docker/shacs-view.html");
    let plugin = include_str!("../../../../docker/shacs_view_auth.py");

    let line = |text: &str| {
        dockerfile
            .lines()
            .position(|line| line == text)
            .ok_or_else(|| format!("the Dockerfile has no line: {text}"))
    };
    let page_copy = line(
        "COPY --chown=root:root --chmod=0644 docker/shacs-view.html /usr/share/novnc/shacs-view.html",
    )?;
    let plugin_copy = line(
        "COPY --chown=root:root --chmod=0644 docker/shacs_view_auth.py /usr/lib/python3/dist-packages/shacs_view_auth.py",
    )?;
    // Both land in directories the viewer packages made, and as root.
    let packages = line("    && rm -rf /var/lib/apt/lists/* \\")?;
    let user = line("USER shacs")?;
    assert!(packages < page_copy && page_copy < user);
    assert!(packages < plugin_copy && plugin_copy < user);
    for package in ["        novnc \\", "        websockify \\"] {
        assert!(line(package)? < packages, "{package}");
    }

    // websockify is started with the page's directory and the plugin's name.
    let arguments = browser::websockify_arguments(Path::new("/generation/view-routes"));
    assert!(arguments.contains(&"--web=/usr/share/novnc".to_owned()));
    assert!(arguments.contains(&"--auth-plugin=shacs_view_auth.SameOrigin".to_owned()));

    assert!(page.contains("path=websockify%3Ftoken%3Dview"));
    assert!(page.contains("resize=scale"));
    assert!(page.contains("\"/websockify?token=paste\""));
    assert!(page.contains("type=\"password\""));
    assert_eq!(page.matches("src=").count(), 1);
    assert!(page.contains("src=\"vnc.html?"));
    for outside in [
        "http://",
        "https://",
        "href=",
        "url(",
        "@import",
        "fetch(",
        "XMLHttpRequest",
        "import(",
        "srcset",
    ] {
        assert!(!page.contains(outside), "the view page names {outside}");
    }
    // The password stays in the page's memory.
    for kept in [
        "localStorage",
        "sessionStorage",
        "indexedDB",
        "document.cookie",
        "location.hash",
        "location.search",
        "location.href",
        "history.",
    ] {
        assert!(!page.contains(kept), "the view page uses {kept}");
    }
    assert!(page.contains("text.value = \"\""));

    assert!(plugin.contains("from websockify.auth_plugins import AuthenticationError"));
    assert!(plugin.contains("class SameOrigin:"));
    assert!(plugin.contains("def authenticate(self, headers, target_host, target_port):"));
    assert_eq!(
        plugin
            .matches("AuthenticationError(response_code=403)")
            .count(),
        2
    );
    assert_eq!(plugin.matches("class ").count(), 1);
    Ok(())
}

#[test]
fn takeover_limit_returns_and_pauses_browser_timers() -> Result<(), Box<dyn Error>> {
    let running_under_user = |stage: &Stage, name: &str| -> Result<(), Box<dyn Error>> {
        stage.upkeep();
        let status = stage.host.status()?;
        assert_eq!(status.state, BrowserState::Running, "{name}");
        assert_eq!(status.control, BrowserControl::User, "{name}");
        assert_eq!(status.reply_wait_deadline_ms, None, "{name}");
        assert!(stage.user(), "{name}");
        assert_eq!(stage.stopped(BrowserChildKind::Chromium), 0, "{name}");
        Ok(())
    };
    let back_by_limit =
        |stage: &Stage, takeover: &BrowserTakeover| -> Result<BrowserStatus, Box<dyn Error>> {
            let status = stage.settled(|status| status.control == BrowserControl::Assistant)?;
            assert_eq!(status.state, BrowserState::Running);
            assert_eq!(status.view.state, BrowserViewState::Open);
            assert_eq!(status.takeover_deadline_ms, None);
            assert_eq!(stage.last_reason().as_deref(), Some("limit"));
            assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));
            assert_eq!(stage.alive(Kind::Input), None);
            assert!(stage.alive(Kind::ViewOnly).is_some());
            assert_eq!(stage.give_back(takeover), BrowserReturnOutcome::Over);
            Ok(status)
        };

    // The reply wait stands still under the user and starts over at the
    // return.
    let stage = Stage::with(Setup {
        takeover_minutes: 60,
        ..Setup::default()
    })?;
    stage.run(OWNER, &open())?;
    stage.host.turn_finished(OWNER);
    let waiting = stage.settled(|status| status.reply_wait_deadline_ms.is_some())?;
    assert_eq!(
        waiting.reply_wait_deadline_ms,
        Some(stage.now() + REPLY_WAIT_MS)
    );
    let takeover = taken(stage.take())?;
    assert_eq!(takeover.minutes, 60);
    let deadline = stage.takeover_deadline()?;
    assert_eq!(deadline, stage.now() + 60 * 60_000);
    assert_eq!(stage.host.status()?.takeover_minutes, 60);
    stage.set_clock(stage.now() + REPLY_WAIT_MS + 60_000);
    running_under_user(&stage, "past the reply wait")?;
    stage.set_clock(deadline - 1);
    running_under_user(&stage, "a millisecond before the limit")?;
    stage.set_clock(deadline);
    let status = back_by_limit(&stage, &takeover)?;
    assert_eq!(
        status.reply_wait_deadline_ms,
        Some(deadline + REPLY_WAIT_MS)
    );
    stage.set_clock(deadline + REPLY_WAIT_MS - 1);
    stage.upkeep();
    assert_eq!(stage.host.status()?.state, BrowserState::Running);
    stage.set_clock(deadline + REPLY_WAIT_MS);
    let status = stage.settled(|status| status.state == BrowserState::Failed)?;
    assert_eq!(
        status.reason,
        Some(BrowserError::ReplyWaitTimeout.to_string())
    );

    // The same for the idle timeout of a session whose turn is running.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    stage.host.turn_started(OWNER);
    let takeover = taken(stage.take())?;
    assert_eq!(takeover.minutes, 15);
    let deadline = stage.takeover_deadline()?;
    assert_eq!(deadline, stage.now() + TAKEOVER_MS);
    stage.set_clock(stage.now() + IDLE_MS + 1_000);
    running_under_user(&stage, "past the idle timeout")?;
    stage.set_clock(deadline - 1);
    running_under_user(&stage, "a millisecond before the limit")?;
    stage.set_clock(deadline);
    let status = back_by_limit(&stage, &takeover)?;
    assert_eq!(status.reply_wait_deadline_ms, None);
    stage.set_clock(deadline + IDLE_MS - 1);
    stage.upkeep();
    assert_eq!(stage.host.status()?.state, BrowserState::Running);
    stage.set_clock(deadline + IDLE_MS);
    let status = stage.settled(|status| status.state == BrowserState::Failed)?;
    assert_eq!(status.reason, Some(BrowserError::IdleTimeout.to_string()));

    // A viewer process that dies under the user ends the episode.
    for dead in [Kind::Input, Kind::Proxy] {
        let stage = Stage::new()?;
        stage.run(OWNER, &open())?;
        let revision = stage.host.control_revision();
        let takeover = taken(stage.take())?;
        // The view-only x11vnc and the websockify the takeover ended in
        // order are not a failed viewer.
        running_under_user(&stage, "after the switch")?;
        assert_eq!(stage.host.status()?.view.state, BrowserViewState::Open);
        stage.kill(dead);
        let status = stage.settled(|status| status.control == BrowserControl::Assistant)?;
        assert_eq!(status.state, BrowserState::Running, "{dead:?}");
        assert_eq!(status.view.state, BrowserViewState::Failed, "{dead:?}");
        assert_eq!(status.view.reason.as_deref(), Some("process_exited"));
        assert_eq!(status.takeover_deadline_ms, None);
        assert_eq!(
            stage.records().settled,
            [record(&takeover.episode, "view_failed", None, false)],
            "{dead:?}"
        );
        assert_eq!(stage.routes().as_deref(), Some(""), "{dead:?}");
        for kind in [Kind::Input, Kind::ViewOnly, Kind::Proxy] {
            assert_eq!(stage.alive(kind), None, "{dead:?}: {kind:?}");
        }
        assert!(fs::symlink_metadata(stage.root()?.join(browser::PASTE_SOCKET)).is_err());
        assert_eq!(stage.host.control_revision(), revision + 2);
        assert_eq!(stage.stopped(BrowserChildKind::Chromium), 0);
        assert_eq!(stage.look(OWNER)?.document.url, "https://example.com/");
        assert_eq!(
            stage.take(),
            Err(BrowserTakeoverError::ViewUnavailable(
                "process_exited".to_owned()
            ))
        );
    }

    // A return in order leaves the viewer open.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let takeover = taken(stage.take())?;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    stage.upkeep();
    let status = stage.host.status()?;
    assert_eq!(status.view.state, BrowserViewState::Open);
    assert_eq!(status.reason, None);
    assert_eq!(
        stage.records().settled,
        [record(&takeover.episode, "returned", None, false)]
    );
    Ok(())
}

#[test]
fn episode_ends_once_and_ignores_stale_endings() -> Result<(), Box<dyn Error>> {
    // An answer, a return and the limit come at the same moment.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let mut quiet = Vec::new();
    for round in 0..16 {
        let call = format!("call-{round}");
        let revision = stage.host.control_revision();
        let takeover = taken(stage.host.takeover(&request(OWNER, Some(&call))))?;
        let deadline = stage.takeover_deadline()?;
        let mark = stage.mark();
        let start = Arc::new(Barrier::new(3));
        let answer = {
            let (host, start) = (stage.host.clone(), Arc::clone(&start));
            let (call, episode) = (call.clone(), takeover.episode.clone());
            thread::spawn(move || {
                start.wait();
                host.finish_handoff(OWNER, &call, &episode)
            })
        };
        let giving = {
            let (host, start) = (stage.host.clone(), Arc::clone(&start));
            let label = takeover.label.clone();
            thread::spawn(move || {
                start.wait();
                host.return_takeover(&BrowserReturnTarget::Label(label))
            })
        };
        let limit = {
            let (host, start) = (stage.host.clone(), Arc::clone(&start));
            let clock = Arc::clone(&stage.clock);
            thread::spawn(move || {
                start.wait();
                clock.0.store(deadline, Ordering::SeqCst);
                let _ = host.maintenance();
            })
        };
        let answered = answer.join().map_err(|_| "the answer panicked")?;
        let returned =
            giving.join().map_err(|_| "the return panicked")? == BrowserReturnOutcome::Returned;
        limit.join().map_err(|_| "the limit panicked")?;
        eventually(|| !stage.user())?;

        let steps = stage.since(mark);
        assert_input_never_outlives_user_control(&steps);
        let spawned = |kind: Kind| {
            steps
                .iter()
                .filter(|seen| matches!(seen.what, What::Spawned(spawned, _) if spawned == kind))
                .count()
        };
        let stopped = |kind: Kind| {
            let mut pids: Vec<u32> = steps
                .iter()
                .filter_map(|seen| match seen.what {
                    What::Stopped(stopped, pid) if stopped == kind => Some(pid),
                    _ => None,
                })
                .collect();
            pids.sort_unstable();
            pids.dedup();
            pids.len()
        };
        assert_eq!(
            (
                stopped(Kind::Proxy),
                stopped(Kind::Input),
                stopped(Kind::ViewOnly)
            ),
            (1, 1, 0),
            "round {round}"
        );
        assert_eq!(
            (
                spawned(Kind::ViewOnly),
                spawned(Kind::Proxy),
                spawned(Kind::Input)
            ),
            (1, 1, 0),
            "round {round}"
        );
        assert_eq!(stage.host.control_revision(), revision + 2, "round {round}");

        let records = stage.records();
        let waiting: Vec<&BrowserEpisodeRecord> = records
            .waiting
            .iter()
            .filter(|record| record.episode == takeover.episode)
            .collect();
        let settled: Vec<&BrowserEpisodeRecord> = records
            .settled
            .iter()
            .filter(|record| record.episode == takeover.episode)
            .collect();
        assert_eq!(waiting.len() + settled.len(), 1, "round {round}");
        let ended = *waiting.first().or(settled.first()).ok_or("no record")?;
        let reason = ended.reason.clone();
        assert!(
            ["answered", "returned", "limit"].contains(&reason.as_str()),
            "round {round}: {reason}"
        );
        assert_eq!(
            *ended,
            record(&takeover.episode, &reason, Some(&call), true)
        );
        assert_eq!(answered, reason == "answered", "round {round}");
        assert_eq!(returned, reason == "returned", "round {round}");
        if waiting.is_empty() {
            assert_eq!(stage.host.pending_resume(&takeover.episode), None);
            quiet.push(takeover.episode.clone());
        } else {
            // The answer came after another ending had the episode, so the
            // paused call still waits and is told once.
            assert_ne!(reason, "answered", "round {round}");
            let told = || {
                stage
                    .resumes()
                    .iter()
                    .filter(|resume| resume.episode == takeover.episode)
                    .count()
            };
            eventually(|| told() == 1)?;
            stage.upkeep();
            assert_eq!(told(), 1, "round {round}");
            stage.host.resume_settled(&takeover.episode);
            assert_eq!(stage.host.pending_resume(&takeover.episode), None);
        }
        assert!(!stage.host.finish_handoff(OWNER, &call, &takeover.episode));
        assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Over);
    }
    stage.upkeep();
    let told = stage.resumes();
    assert!(quiet
        .iter()
        .all(|episode| told.iter().all(|resume| &resume.episode != episode)));
    assert_eq!(stage.records().waiting, []);
    assert_eq!(stage.stopped(BrowserChildKind::Chromium), 0);

    // A request that names an episode that is over changes nothing.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let first = taken(stage.host.takeover(&request(OWNER, Some("call-1"))))?;
    let first_deadline = stage.takeover_deadline()?;
    assert_eq!(stage.give_back(&first), BrowserReturnOutcome::Returned);
    stage.set_clock(stage.now() + 10 * 60_000);
    let second = taken(stage.host.takeover(&request(OWNER, Some("call-2"))))?;
    let second_deadline = stage.takeover_deadline()?;
    assert_eq!(second_deadline, stage.now() + TAKEOVER_MS);
    let before = (stage.records(), stage.host.control_revision());
    let mark = stage.mark();
    assert!(!stage.host.finish_handoff(OWNER, "call-1", &first.episode));
    assert!(!stage.host.finish_handoff(OWNER, "call-1", &second.episode));
    assert!(!stage.host.finish_handoff(OWNER, "call-2", &first.episode));
    assert!(!stage.host.finish_handoff(OTHER, "call-2", &second.episode));
    assert_eq!(stage.give_back(&first), BrowserReturnOutcome::Over);
    stage.host.cancel_takeover(OTHER);
    // The limit of the episode that is over passes.
    stage.set_clock(first_deadline);
    stage.upkeep();
    stage.set_clock(second_deadline - 1);
    stage.upkeep();
    assert_eq!((stage.records(), stage.host.control_revision()), before);
    assert_eq!(stage.host.status()?.control, BrowserControl::User);
    assert_eq!(
        stage.host.status()?.takeover_deadline_ms,
        Some(second_deadline)
    );
    assert!(!stage
        .since(mark)
        .iter()
        .any(|seen| matches!(seen.what, What::Spawned(..) | What::Stopped(..))));
    assert_eq!(
        before.0.open.map(|open| open.episode),
        Some(second.episode.clone())
    );
    assert_eq!(
        before.0.waiting,
        [record(&first.episode, "returned", Some("call-1"), true)]
    );
    assert!(stage.host.finish_handoff(OWNER, "call-2", &second.episode));
    assert!(!stage.host.finish_handoff(OWNER, "call-2", &second.episode));
    assert_eq!(
        stage.records().settled,
        [record(&second.episode, "answered", Some("call-2"), true)]
    );

    // The settled records are the last eight. One that waits to continue
    // is not among them until its call is closed.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let paused = taken(stage.host.takeover(&request(OWNER, Some("call-waiting"))))?;
    assert_eq!(stage.give_back(&paused), BrowserReturnOutcome::Returned);
    let waiting = record(&paused.episode, "returned", Some("call-waiting"), true);
    let mut plain = Vec::new();
    for _ in 0..10 {
        let takeover = taken(stage.take())?;
        assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
        plain.push(record(&takeover.episode, "returned", None, false));
    }
    let records = stage.records();
    assert_eq!(records.waiting, std::slice::from_ref(&waiting));
    assert_eq!(records.settled, plain[2..]);
    let resume = BrowserPendingResume {
        episode: paused.episode.clone(),
        reason: "returned".to_owned(),
        session_id: OWNER.to_owned(),
        tool_call_id: "call-waiting".to_owned(),
        channel: "telegram".to_owned(),
        chat_id: "chat-owner".to_owned(),
        reply_route: Map::new(),
    };
    assert_eq!(stage.resumes(), std::slice::from_ref(&resume));
    assert_eq!(stage.host.pending_resume(&paused.episode), Some(resume));
    stage.host.resume_settled(&plain[9].episode);
    assert_eq!(stage.records(), records);
    stage.host.resume_settled(&paused.episode);
    let records = stage.records();
    assert_eq!(records.waiting, []);
    assert_eq!(records.settled.len(), 8);
    assert_eq!(records.settled[..7], plain[3..]);
    assert_eq!(records.settled[7], waiting);
    assert_eq!(stage.host.pending_resume(&paused.episode), None);

    // `/stop` and `/new` of the session, and a call closed another way,
    // close a waiting record too.
    let limited = taken(stage.host.takeover(&request(OWNER, Some("call-limit"))))?;
    stage.set_clock(stage.takeover_deadline()?);
    stage.settled(|status| status.control == BrowserControl::Assistant)?;
    let over = taken(stage.host.takeover(&request(OWNER, Some("call-over"))))?;
    assert_eq!(stage.give_back(&over), BrowserReturnOutcome::Returned);
    let limited = record(&limited.episode, "limit", Some("call-limit"), true);
    let over = record(&over.episode, "returned", Some("call-over"), true);
    assert_eq!(stage.records().waiting, [limited.clone(), over.clone()]);
    stage.host.handoff_closed(OWNER, "call-other");
    stage.host.handoff_closed(OTHER, "call-over");
    stage.host.cancel_takeover(OTHER);
    assert_eq!(stage.records().waiting, [limited.clone(), over.clone()]);
    stage.host.handoff_closed(OWNER, "call-over");
    let records = stage.records();
    assert_eq!(records.waiting, std::slice::from_ref(&limited));
    assert_eq!(records.settled.last(), Some(&over));
    stage.host.cancel_takeover(OWNER);
    let records = stage.records();
    assert_eq!(records.waiting, []);
    assert_eq!(records.settled.last(), Some(&limited));
    assert_eq!(records.settled.len(), 8);
    assert_eq!(stage.host.pending_resume(&limited.episode), None);
    assert_eq!(stage.host.status()?.state, BrowserState::Running);

    // `/browser close` and the session's `/new` under the user.
    for handoff in [false, true] {
        let tool_call = handoff.then_some("call-handoff");
        let stage = Stage::new()?;
        stage.run(OWNER, &open())?;
        let takeover = taken(stage.host.takeover(&request(OWNER, tool_call)))?;
        assert!(stage.host.close_by_user() != BrowserCloseOutcome::NothingOpen);
        stage.settled(|status| status.state == BrowserState::Idle)?;
        assert!(!stage.user());
        assert_eq!(stage.host.status()?.control, BrowserControl::Assistant);
        let ended = record(&takeover.episode, "closed", tool_call, handoff);
        let records = stage.records();
        if handoff {
            assert_eq!((records.waiting, records.settled), (vec![ended], vec![]));
            eventually(|| stage.resumes().len() == 1)?;
            assert_eq!(stage.resumes()[0].reason, "closed");
        } else {
            assert_eq!((records.waiting, records.settled), (vec![], vec![ended]));
            stage.upkeep();
            assert_eq!(stage.resumes(), []);
        }
        assert_eq!(stage.acknowledge(OWNER), Some(BrowserError::Closed));
        assert_eq!(stage.stopped(BrowserChildKind::Chromium), 1);
        for kind in [Kind::Input, Kind::ViewOnly, Kind::Proxy] {
            assert_eq!(stage.alive(kind), None, "{kind:?}");
        }

        let stage = Stage::new()?;
        stage.run(OWNER, &open())?;
        let takeover = taken(stage.host.takeover(&request(OWNER, tool_call)))?;
        stage.host.cancel_takeover(OTHER);
        assert!(stage.user());
        stage.host.cancel_takeover(OWNER);
        assert!(!stage.user());
        stage.upkeep();
        let records = stage.records();
        assert_eq!(records.waiting, []);
        assert_eq!(
            records.settled,
            [record(&takeover.episode, "cancelled", tool_call, handoff)]
        );
        assert_eq!(stage.resumes(), []);
        let status = stage.host.status()?;
        assert_eq!(status.state, BrowserState::Running);
        assert_eq!(status.control, BrowserControl::Assistant);
        assert_eq!(status.view.state, BrowserViewState::Open);
        assert_eq!(stage.alive(Kind::Input), None);
        assert!(stage.host.close_for_session(OWNER));
        stage.settled(|status| status.state == BrowserState::Idle)?;
    }
    Ok(())
}

/// A `look` that has the browser slot and stays in its read until released.
struct HeldLook {
    release: std::sync::mpsc::Sender<()>,
    look: thread::JoinHandle<Result<Option<BrowserRead>, BrowserError>>,
}

impl HeldLook {
    fn start(stage: &Stage) -> Result<Self, Box<dyn Error>> {
        let (entered, inside) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        stage.page.lock().map_err(|_| "page")?.during_read = Some(Box::new(move || {
            let _ = entered.send(());
            let _ = released.recv();
        }));
        let (host, context) = (stage.host.clone(), stage.call(OWNER));
        let look = thread::spawn(move || {
            let arguments = json!({"action":"look"});
            let prepared = calm(|| host.prepare(OWNER, &arguments))?;
            let browser = calm(|| host.acquire(&context))?;
            calm(|| browser.execute_prepared(&context, &arguments, &prepared.digest))
        });
        inside.recv_timeout(Duration::from_secs(5))?;
        Ok(Self { release, look })
    }

    fn release(self) -> Result<Result<Option<BrowserRead>, BrowserError>, Box<dyn Error>> {
        self.release.send(())?;
        Ok(self.look.join().map_err(|_| "the look panicked")?)
    }

    fn finish(self) -> Result<BrowserRead, Box<dyn Error>> {
        Ok(self.release()??.ok_or("look reads the page")?)
    }
}

#[test]
fn takeover_start_rules() -> Result<(), Box<dyn Error>> {
    let untouched = |stage: &Stage| -> Result<_, Box<dyn Error>> {
        Ok((
            stage.host.status()?,
            stage.records(),
            stage.host.control_revision(),
            stage.world().specs.len(),
            stage.started(BrowserChildKind::Chromium),
            stage.stopped(BrowserChildKind::Chromium),
        ))
    };

    // A viewer that is not open refuses the takeover with its reason, and
    // nothing changes.
    for (config, dies, reason) in [
        (view(false, SECRET, ""), false, "disabled"),
        (view(true, "", ""), false, "password_missing"),
        (view(true, SECRET, ""), true, "process_exited"),
    ] {
        let refused = Err(BrowserTakeoverError::ViewUnavailable(reason.to_owned()));
        if !dies {
            let stage = Stage::with(Setup {
                view: config.clone(),
                ..Setup::default()
            })?;
            assert_eq!(stage.take(), refused, "{reason}");
            assert_eq!(stage.started(BrowserChildKind::Chromium), 0, "{reason}");
            assert!(stage.host.receipt()?.is_none(), "{reason}");
        }
        let stage = Stage::with(Setup {
            view: config,
            ..Setup::default()
        })?;
        stage.run(OWNER, &open())?;
        if dies {
            stage.kill(Kind::Proxy);
            stage.settled(|status| status.view.state == BrowserViewState::Failed)?;
        }
        let before = untouched(&stage)?;
        for tool_call in [None, Some("call-handoff")] {
            assert_eq!(
                stage.host.takeover(&request(OWNER, tool_call)),
                refused,
                "{reason}"
            );
        }
        assert_eq!(untouched(&stage)?, before, "{reason}");
        assert_eq!(before.0.control, BrowserControl::Assistant);
        assert_eq!(before.0.state, BrowserState::Running);
        assert_eq!(stage.host.closure_notice(OWNER), None, "{reason}");
        assert_eq!(stage.look(OWNER)?.document.url, "https://example.com/");
    }

    // With no browser, `/browser takeover` starts one the way an action
    // would, on a blank tab.
    let stage = Stage::with(Setup {
        enable: false,
        ..Setup::default()
    })?;
    assert_eq!(
        stage.take(),
        Err(BrowserTakeoverError::Browser(BrowserError::Disabled))
    );
    assert_eq!(stage.started(BrowserChildKind::Chromium), 0);
    assert!(!stage.user());

    let stage = Stage::with(Setup {
        page: "about:blank",
        ..Setup::default()
    })?;
    assert!(stage.host.receipt()?.is_none());
    let takeover = taken(stage.take())?;
    assert_eq!(
        stage
            .host
            .receipt()?
            .ok_or("the start did not pass the process gate")?
            .dispatch_count,
        1
    );
    assert_eq!(stage.started(BrowserChildKind::Chromium), 1);
    {
        let core = stage.core.lock().map_err(|_| "facts")?;
        let chromium = core
            .spawns
            .iter()
            .find(|spec| spec.kind == BrowserChildKind::Chromium)
            .ok_or("no Chromium")?;
        let pages: Vec<&String> = chromium
            .arguments
            .iter()
            .filter(|argument| !argument.starts_with('-'))
            .collect();
        assert_eq!(pages, ["about:blank"]);
    }
    assert_eq!(stage.sent("Page.navigate"), 0);
    assert_eq!(takeover.view_url, "http://127.0.0.1:6080/shacs-view.html");
    assert_eq!(takeover.minutes, 15);
    let described = stage.host.describe();
    assert!(matches!(described.state, BrowserDescriptionState::Open));
    assert_eq!(described.session.as_deref(), Some(OWNER));
    assert_eq!(described.reply_wait_remaining_minutes, None);
    let by = described.control.takeover.ok_or("not taken")?;
    assert_eq!(
        (by.session.as_str(), by.label.as_str(), by.remaining_minutes),
        (OWNER, takeover.label.as_str(), 15)
    );
    assert!(matches!(
        described.control.view,
        BrowserViewDescription::Open(url) if url == takeover.view_url
    ));
    assert_eq!(stage.two_routes()?, stage.routes().ok_or("no routes")?);
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);

    // An action in flight is waited for and the takeover is next.
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let held = HeldLook::start(&stage)?;
    let taking = {
        let host = stage.host.clone();
        thread::spawn(move || host.takeover(&request(OWNER, None)))
    };
    thread::sleep(Duration::from_millis(60));
    assert!(!taking.is_finished());
    assert!(!stage.user());
    assert!(matches!(
        stage.host.prepare(OTHER, &open()),
        Err(BrowserError::Busy)
    ));
    let read = held.finish()?;
    assert_eq!(read.document.url, "https://example.com/");
    let takeover = taken(taking.join().map_err(|_| "the takeover panicked")?)?;
    assert_eq!(
        stage.records().open.map(|open| open.episode),
        Some(takeover.episode.clone())
    );
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);

    // The wait ends after the page timeout and five seconds.
    let before = untouched(&stage)?.1;
    let held = HeldLook::start(&stage)?;
    let started = stage.now();
    let (calling, called) = std::sync::mpsc::channel();
    let taking = {
        let host = stage.host.clone();
        thread::spawn(move || {
            let _ = calling.send(());
            host.takeover(&request(OWNER, None))
        })
    };
    // The wait is counted from the clock the request saw first, so the
    // clock stands still until the request is in its wait.
    called.recv_timeout(Duration::from_secs(5))?;
    thread::sleep(Duration::from_millis(60));
    assert!(!taking.is_finished());
    stage.set_clock(started + PAGE_TIMEOUT_MS + 4_999);
    thread::sleep(Duration::from_millis(60));
    assert!(!taking.is_finished());
    stage.set_clock(started + PAGE_TIMEOUT_MS + 5_000);
    eventually(|| taking.is_finished())?;
    assert_eq!(
        taking.join().map_err(|_| "the takeover panicked")?,
        Err(BrowserTakeoverError::Browser(BrowserError::Busy))
    );
    assert!(!stage.user());
    // The action that outlasted the page timeout ends the way it does
    // without a takeover.
    assert_eq!(held.release()?, Err(BrowserError::PageTimeout));
    assert_eq!(stage.records(), before);
    assert_eq!(stage.host.status()?.control, BrowserControl::Assistant);
    // The reservation went with the request.
    stage.run(OWNER, &open())?;
    assert_eq!(stage.look(OWNER)?.document.url, "https://example.com/");

    // Another session's browser: `handoff` is the assistant's call and is
    // busy, `/browser takeover` is the user's and takes it as it is.
    let stage = Stage::new()?;
    stage.run(OTHER, &open())?;
    let before = untouched(&stage)?;
    assert_eq!(
        stage.host.takeover(&request(OWNER, Some("call-handoff"))),
        Err(BrowserTakeoverError::Browser(BrowserError::Busy))
    );
    assert_eq!(untouched(&stage)?, before);
    let takeover = taken(stage.take())?;
    let described = stage.host.describe();
    assert_eq!(described.session.as_deref(), Some(OTHER));
    assert_eq!(
        described.control.takeover.map(|by| by.session),
        Some(OWNER.to_owned())
    );
    assert_eq!(stage.started(BrowserChildKind::Chromium), 1);

    // Under the user nobody takes it again.
    for again in [
        request(OWNER, None),
        request(OWNER, Some("call-handoff")),
        request(OTHER, None),
        request(OTHER, Some("call-handoff")),
    ] {
        assert_eq!(
            stage.host.takeover(&again),
            Err(BrowserTakeoverError::AlreadyTaken)
        );
    }
    assert_eq!(
        stage.records().open.map(|open| open.episode),
        Some(takeover.episode.clone())
    );
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert_eq!(
        stage.records().settled,
        [record(&takeover.episode, "returned", None, false)]
    );
    assert_eq!(stage.host.describe().session.as_deref(), Some(OTHER));
    assert_eq!(stage.look(OTHER)?.document.url, "https://example.com/");
    assert!(matches!(
        stage.prepare(OWNER, &json!({"action":"look"})),
        Err(BrowserError::Busy)
    ));

    // inspect and diagnostics say who controls and until when, and not who
    // took it or which episode it is.
    let stage = Stage::with(Setup {
        state_dir: true,
        ..Setup::default()
    })?;
    stage.run(OWNER, &open())?;
    let takeover = taken(stage.take())?;
    let deadline = stage.now() + TAKEOVER_MS;
    let state_file = stage
        .bundle
        .context
        .data_dir
        .join("runtime/browser-state.json");
    let written = fs::read_to_string(&state_file)?;
    let recorded: Value = serde_json::from_str(&written)?;
    assert_eq!(recorded["status"]["control"], "user");
    assert_eq!(recorded["status"]["takeoverMinutes"], 15);
    assert_eq!(recorded["status"]["takeoverDeadlineMs"], deadline);
    let observed = stage.inspected("owner")?;
    assert!(observed.owner_observed);
    assert_eq!(
        (
            observed.control,
            observed.takeover_minutes,
            observed.takeover_deadline_ms
        ),
        (BrowserControl::User, 15, Some(deadline))
    );
    assert_eq!(
        observed.view.url.as_deref(),
        Some("http://127.0.0.1:6080/shacs-view.html")
    );
    // Another owner's snapshot claims nothing live.
    let unknown = stage.inspected("owner-before")?;
    assert!(!unknown.owner_observed);
    assert_eq!(
        (
            unknown.control,
            unknown.takeover_minutes,
            unknown.takeover_deadline_ms
        ),
        (BrowserControl::Assistant, 15, None)
    );
    let options = || {
        (
            Some(stage.bundle.context.config_path.clone()),
            Some(stage.bundle.context.workspace.clone()),
        )
    };
    let (config_path, workspace_override) = options();
    let report = shacs_cli::runtime_inspect(shacs_cli::RuntimeInspectOptions {
        config_path,
        workspace_override,
    })?;
    let inspected = serde_json::to_value(&report.browser)?;
    let (config_path, workspace_override) = options();
    let diagnostics = shacs_cli::runtime_diagnostics(shacs_cli::RuntimeDiagnosticsOptions {
        config_path,
        workspace_override,
        bundle_path: None,
    })?;
    let diagnosed = &diagnostics.snapshot.runtime["browser"];
    assert_eq!(inspected["control"], "assistant");
    assert_eq!(inspected["takeoverMinutes"], 15);
    assert_eq!(inspected["takeoverDeadlineMs"], Value::Null);
    for key in ["control", "takeoverMinutes", "takeoverDeadlineMs"] {
        assert!(inspected.get(key).is_some(), "{key}");
        assert_eq!(diagnosed.get(key), inspected.get(key), "{key}");
    }
    let nonce = takeover.episode.split('-').next().ok_or("episode")?;
    let label = format!("\"{}\"", takeover.label);
    for (name, text) in [
        ("state file", written),
        ("status", serde_json::to_string(&stage.host.status()?)?),
        ("owner inspect", serde_json::to_string(&observed)?),
        ("inspect", inspected.to_string()),
        ("diagnostics", diagnosed.to_string()),
    ] {
        for private in [OWNER, "chat-owner", nonce, label.as_str()] {
            assert!(!text.contains(private), "{name}");
        }
    }
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    let recorded: Value = serde_json::from_slice(&fs::read(&state_file)?)?;
    assert_eq!(recorded["status"]["control"], "assistant");
    assert_eq!(recorded["status"]["takeoverDeadlineMs"], Value::Null);
    assert_eq!(stage.inspected("owner")?.control, BrowserControl::Assistant);
    Ok(())
}
