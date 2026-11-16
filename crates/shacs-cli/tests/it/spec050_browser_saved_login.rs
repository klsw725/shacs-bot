//! Spec 050 PRD 020: the saved login profile, the lock files beside it and
//! `browser forget`, on the real browser manager with a fake process runner,
//! a fake DevTools peer, a test clock and temporary data folders.
use super::spec049_browser_runtime::{bundle, input as gate_input, Startup};
use super::spec050_browser_takeover::{self as takeover, Setup, Stage};
use fs2::FileExt;
use serde_json::{json, Value};
use shacs_cli::browser::{
    self, BrowserCancellation, BrowserChild, BrowserChildKind, BrowserChildSpec, BrowserClock,
    BrowserFolder, BrowserOwnerBinding, BrowserProcessRunner, BrowserProfileState,
    BrowserSnapshotDestination, BrowserStartup, BrowserState, BrowserStatus, CdpReply,
    ConfigBrowserStartup, DevToolsPeer, FileIdentity, ForgetConfirmation, ForgetHeld, ForgetReport,
    ForgetRequest, ForgetStage, ManagedBrowserHost, PausedTargetAdmission, ViewerListenAddress,
    ViewerListener,
};
use shacs_cli::{
    CliCommand, CliError, RuntimeDiagnosticsOptions, RuntimeInspectOptions,
    RuntimeOwnerProcessEvidence, RuntimeOwnershipMarker, RuntimeOwnershipState,
    RuntimeOwnershipStatus, ScriptedPrompt,
};
use shacs_config::{ConfigBundle, LoadOptions, PermissionMode};
use shacs_core::runtime::{
    BrowserCallContext, BrowserCloseOutcome, BrowserError, BrowserHost, BrowserPreparedAction,
    BrowserRead, BrowserSession, ProcessGateInput,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::os::unix::fs::{symlink, DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use std::{io, thread};

const HOST: &str = "shacs-box";
const OWNER: &str = "telegram:chat-owner";
const OTHER: &str = "telegram:chat-other";
const IDLE_MS: u64 = 300_000;
const REPLY_WAIT_MS: u64 = 30 * 60_000;
const LOCK_CHECK_MS: u64 = 1_000;
const QUIT_MS: u64 = 5_000;
const LIVE_PID: u32 = 4242;
/// What the fake Chromium writes into its HTTP cache folder.
const CACHED: &str = "cached-response";
/// In the name of every file a test puts into the profile.
const KEPT: &str = "zq-kept";
const KEPT_BYTES: u64 = 1_546;
const QUIT: &str = "Browser.close";
const LOCK_REPLACED_REASON: &str = "browser_lock_replaced";

const CLOSED: &str = "브라우저를 닫았습니다.";
const REMOVED: &str = "로그인 기록을 지웠습니다.";
const NOTHING: &str = "지울 로그인 기록이 없습니다.";
const NOT_REMOVED: &str = "지우지 않았습니다.";
const NO_TERMINAL: &str =
    "터미널이 아니어서 물어볼 수 없습니다. 지우려면 --yes를 붙여 다시 실행하세요.";
const NOT_CLOSED: &str = "브라우저를 닫지 못해 지우지 않았습니다. 잠시 뒤 다시 하세요.";
const OTHER_FORGET: &str = "다른 browser forget이 실행 중입니다.";
const LOCK_REPLACED: &str = "브라우저 lock 파일이 바뀌어 지우지 않았습니다. 다시 실행하세요.";
const NOT_A_FOLDER: &str = "로그인 기록 폴더가 진짜 폴더가 아니어서 지우지 않았습니다.";
const ELSEWHERE: &str =
    "로그인 기록이 다른 곳에서 잠겨 있어 지우지 않았습니다. docs/USAGE.md의 브라우저 로그인 유지를 보세요.";

const FORGET_IN_PROGRESS: &str =
    "browser_forget_in_progress: 로그인 기록을 지우는 중입니다. 잠시 뒤 다시 하세요";
const PROFILE_UNREADABLE: &str = "browser_profile_unreadable: 로그인 기록 폴더를 쓸 수 없습니다";
const PROFILE_IN_USE: &str = "browser_profile_in_use: 로그인 기록을 다른 곳에서 쓰는 중입니다";
const PROFILE_LOCKED: &str =
    "browser_profile_locked: 로그인 기록이 잠겨 있습니다. docs/USAGE.md의 브라우저 로그인 유지를 보세요";
const STATE_UNAVAILABLE: &str = "browser_state_unavailable: 브라우저 상태를 읽을 수 없습니다";

/// The Spec 049 Chromium arguments, with the parts that differ between two
/// generations written as in `shape`.
const PLAIN_ARGUMENTS: [&str; 14] = [
    "--display=:N",
    "--user-data-dir=<root>/profile",
    "--no-first-run",
    "--no-default-browser-check",
    "--no-sandbox",
    "--remote-debugging-pipe",
    "--proxy-server=socks5://127.0.0.1:N",
    "--proxy-bypass-list=<-loopback>",
    "--disable-quic",
    "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
    "--disable-background-networking",
    "--disable-component-update",
    "--window-size=1280,720",
    "about:blank",
];

type PauseHook<'a> = Box<dyn FnMut(u64) + Send + 'a>;

/// A clock that only moves when it is told to. A wait moves it by the time
/// waited and then runs the hook, so a test acts inside the wait of the code
/// under test instead of racing it.
#[derive(Default)]
struct Tick<'a> {
    now: AtomicU64,
    on_pause: Mutex<Option<PauseHook<'a>>>,
}

impl<'a> Tick<'a> {
    fn with(hook: impl FnMut(u64) + Send + 'a) -> Self {
        let clock = Self::default();
        clock.hook(hook);
        clock
    }

    fn hook(&self, hook: impl FnMut(u64) + Send + 'a) {
        *self.on_pause.lock().expect("pause hook") = Some(Box::new(hook));
    }

    fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
}

impl BrowserClock for Tick<'_> {
    fn now_ms(&self) -> u64 {
        self.now()
    }

    fn pause(&self, ms: u64) {
        self.advance(ms);
        let hook = self.on_pause.lock().ok().and_then(|mut hook| hook.take());
        if let Some(mut hook) = hook {
            hook(ms);
            if let Ok(mut slot) = self.on_pause.lock() {
                slot.get_or_insert(hook);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Sent(String),
    Stopped(BrowserChildKind),
}

/// What the fake processes and the fake DevTools saw, with the manager's
/// clock at each step.
#[derive(Default)]
struct World {
    steps: Vec<(Step, u64)>,
    spawned: usize,
    chromium: Vec<BrowserChildSpec>,
    roots: Vec<PathBuf>,
    exits: Vec<Arc<AtomicBool>>,
    live_pids: Vec<u32>,
    /// Chromium keeps running after `Browser.close`.
    ignores_quit: bool,
    during_read: Option<Box<dyn FnOnce() + Send>>,
    before_spawn: Option<Box<dyn FnOnce() + Send>>,
    during_hostname: Option<Box<dyn FnOnce() + Send>>,
}

struct Peer {
    world: Arc<Mutex<World>>,
    clock: Arc<Tick<'static>>,
    exited: Arc<AtomicBool>,
    cancellation: BrowserCancellation,
    sequence: u64,
}

impl DevToolsPeer for Peer {
    fn set_cancellation(&mut self, cancellation: BrowserCancellation) {
        self.cancellation = cancellation;
    }

    fn request(
        &mut self,
        method: &str,
        _params: Value,
        _session: Option<&str>,
        _deadline: Instant,
        _admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        self.cancellation.check()?;
        self.sequence += 1;
        let mut world = self
            .world
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        world
            .steps
            .push((Step::Sent(method.to_owned()), self.clock.now()));
        let result = match method {
            "Target.getTargets" => json!({"targetInfos":[{"targetId":"target-1","type":"page"}]}),
            "Target.attachToTarget" => json!({"sessionId":"session-1"}),
            "Page.getFrameTree" => json!({"frameTree":{"frame":{
                "id":"frame-1","loaderId":"loader-1","url":"https://example.com/"}}}),
            "Accessibility.getFullAXTree" => {
                let during_read = world.during_read.take();
                drop(world);
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
            "Page.captureScreenshot" => json!({"data":"iVBORw0KGgo="}),
            QUIT => {
                if !world.ignores_quit {
                    self.exited.store(true, Ordering::SeqCst);
                }
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
        Ok(Vec::new())
    }
}

struct Child {
    kind: BrowserChildKind,
    world: Arc<Mutex<World>>,
    clock: Arc<Tick<'static>>,
    exited: Arc<AtomicBool>,
    peer: Option<Box<dyn DevToolsPeer>>,
    stopped: bool,
}

impl BrowserChild for Child {
    fn exited(&mut self) -> Result<bool, BrowserError> {
        Ok(self.kind == BrowserChildKind::Xauth || self.exited.load(Ordering::SeqCst))
    }

    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        Ok(self.kind == BrowserChildKind::Xauth)
    }

    fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.world
                .lock()
                .expect("fake child world")
                .steps
                .push((Step::Stopped(self.kind), self.clock.now()));
        }
    }

    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        self.peer.take().ok_or(BrowserError::Protocol)
    }
}

struct Runner {
    world: Arc<Mutex<World>>,
    clock: Arc<Tick<'static>>,
}

impl Runner {
    /// A runner for a `browser forget` that no manager of the test runs
    /// beside: it names this host and the processes in `live`.
    fn alone(live: &[u32]) -> Self {
        Self {
            world: Arc::new(Mutex::new(World {
                live_pids: live.to_vec(),
                ..World::default()
            })),
            clock: Arc::new(Tick::default()),
        }
    }
}

impl BrowserProcessRunner for Runner {
    fn available(&self) -> bool {
        true
    }

    fn viewer_available(&self) -> bool {
        true
    }

    fn viewer_listener(
        &self,
        _address: ViewerListenAddress,
        _port: u16,
        _owner: Option<u32>,
    ) -> ViewerListener {
        ViewerListener::Absent
    }

    fn hostname(&self) -> Option<String> {
        let hook = self
            .world
            .lock()
            .ok()
            .and_then(|mut world| world.during_hostname.take());
        if let Some(hook) = hook {
            hook();
        }
        Some(HOST.to_owned())
    }

    fn chromium_running(&self, pid: u32) -> bool {
        self.world
            .lock()
            .is_ok_and(|world| world.live_pids.contains(&pid))
    }

    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        _deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        let hook = self
            .world
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .before_spawn
            .take();
        if let Some(hook) = hook {
            hook();
        }
        let mut world = self
            .world
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        world.spawned += 1;
        let exited = Arc::new(AtomicBool::new(false));
        let chromium = spec.kind == BrowserChildKind::Chromium;
        if chromium {
            let root = Path::new(&spec.environment["HOME"])
                .parent()
                .ok_or(BrowserError::Protocol)?
                .to_path_buf();
            assert!(root.to_string_lossy().starts_with("/tmp/shacs-browser-"));
            assert_eq!(mode(&root).map_err(|_| BrowserError::Protocol)?, 0o700);
            if let Some(cache) = argument(spec, "--disk-cache-dir=") {
                fs::write(Path::new(cache).join(CACHED), b"page response")
                    .map_err(|_| BrowserError::Protocol)?;
            }
            world.chromium.push(spec.clone());
            world.roots.push(root);
            world.exits.push(Arc::clone(&exited));
        }
        Ok(Box::new(Child {
            kind: spec.kind,
            world: Arc::clone(&self.world),
            clock: Arc::clone(&self.clock),
            exited: Arc::clone(&exited),
            peer: chromium.then(|| {
                Box::new(Peer {
                    world: Arc::clone(&self.world),
                    clock: Arc::clone(&self.clock),
                    exited,
                    cancellation: BrowserCancellation::default(),
                    sequence: 0,
                }) as Box<dyn DevToolsPeer>
            }),
            stopped: false,
        }))
    }

    fn display_ready(
        &self,
        _display: u16,
        _deadline: Instant,
        cancellation: &BrowserCancellation,
    ) -> Result<(), BrowserError> {
        cancellation.check()?;
        Ok(())
    }
}

/// One temporary data folder with its config.
struct Home {
    _root: tempfile::TempDir,
    bundle: ConfigBundle,
    gate: ProcessGateInput,
}

impl Home {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let bundle = bundle(root.path(), PermissionMode::Default)?;
        let gate = gate_input(&bundle)?;
        Ok(Self {
            _root: root,
            bundle,
            gate,
        })
    }

    fn data(&self) -> &Path {
        &self.bundle.context.data_dir
    }

    fn config_path(&self) -> &Path {
        &self.bundle.context.config_path
    }

    fn folder(&self) -> BrowserFolder {
        BrowserFolder::new(self.data())
    }

    fn profile(&self) -> PathBuf {
        self.folder().profile()
    }

    /// `browser/` as a start leaves it, without any lock file.
    fn make_folder(&self) -> io::Result<()> {
        private_folder(self.folder().root())
    }

    /// A profile an earlier browser left behind: `KEPT_BYTES` in two files.
    fn seed_profile(&self) -> io::Result<()> {
        self.make_folder()?;
        seed(&self.profile())
    }

    fn rig(&self, persist: bool) -> Result<Rig, Box<dyn Error>> {
        Rig::new(self, persist)
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

    fn marker(&self) -> RuntimeOwnershipMarker {
        RuntimeOwnershipMarker {
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
            config_path: self.config_path().to_string_lossy().into_owned(),
            workspace: self.bundle.context.workspace.to_string_lossy().into_owned(),
            lock_protocol: None,
            process_evidence: RuntimeOwnerProcessEvidence {
                pid: std::process::id(),
                pid_alive: true,
                process_started_after_marker: false,
            },
        }
    }

    /// What `runtime inspect` reads of the browser the owner left a
    /// snapshot of.
    fn inspected(&self, now: u64) -> BrowserStatus {
        let ownership = RuntimeOwnershipStatus {
            state: RuntimeOwnershipState::Active,
            marker: Some(self.marker()),
            reason: "fixture".to_owned(),
        };
        browser::read_browser_snapshot(
            self.data(),
            &ownership,
            self.config_path(),
            &self.bundle.context.workspace,
            BrowserStatus::new(true, true, now),
            now,
        )
    }

    /// Writes `tools.browser.persistLogin` into the config file, as a user
    /// would, and loads it again.
    fn configured(&self, persist: bool) -> Result<ConfigBundle, Box<dyn Error>> {
        let mut config: Value = serde_json::from_slice(&fs::read(self.config_path())?)?;
        config["tools"]["browser"]["persistLogin"] = json!(persist);
        fs::write(self.config_path(), serde_json::to_vec(&config)?)?;
        Ok(shacs_config::load_config(LoadOptions {
            config_path: Some(self.config_path().to_owned()),
            ..LoadOptions::default()
        })?)
    }

    fn cli(&self, arguments: &[&str]) -> Result<String, CliError> {
        let config = self.config_path().to_string_lossy().into_owned();
        let mut line = arguments.to_vec();
        line.extend(["--config", config.as_str()]);
        shacs_cli::run_command(shacs_cli::parse_cli_args(line)?)
    }
}

/// One browser manager on a `Home`, as the gateway builds it except for the
/// fakes.
struct Rig {
    host: ManagedBrowserHost,
    world: Arc<Mutex<World>>,
    clock: Arc<Tick<'static>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.host.shutdown();
    }
}

impl Rig {
    fn new(home: &Home, persist: bool) -> Result<Self, Box<dyn Error>> {
        let clock = Arc::new(Tick::default());
        let world = Arc::new(Mutex::new(World::default()));
        let mut config = home.bundle.config.tools.browser.clone();
        config.persist_login = persist;
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                config,
                input: home.gate.clone(),
            }),
            Box::new(Runner {
                world: Arc::clone(&world),
                clock: Arc::clone(&clock),
            }),
            Arc::clone(&clock) as Arc<dyn BrowserClock>,
            Some(BrowserSnapshotDestination {
                data_dir: home.data().to_owned(),
                owner: Some(BrowserOwnerBinding::from_marker(&home.marker())?),
                config_path: home.config_path().to_owned(),
                workspace: home.bundle.context.workspace.clone(),
            }),
            None,
        );
        Ok(Self { host, world, clock })
    }

    fn world(&self) -> MutexGuard<'_, World> {
        self.world.lock().expect("fake world")
    }

    fn runner(&self) -> Runner {
        Runner {
            world: Arc::clone(&self.world),
            clock: Arc::clone(&self.clock),
        }
    }

    fn start(&self, call: &BrowserCallContext) -> Result<(), BrowserError> {
        acquire(&self.host, call).map(drop)
    }

    /// The Chromium of generation `index`, counted from the first.
    fn spec(&self, index: usize) -> BrowserChildSpec {
        self.world().chromium[index].clone()
    }

    fn root(&self, index: usize) -> PathBuf {
        self.world().roots[index].clone()
    }

    fn spawned(&self) -> usize {
        self.world().spawned
    }

    /// The manager's clock at each time `step` happened.
    fn times(&self, step: &Step) -> Vec<u64> {
        self.world()
            .steps
            .iter()
            .filter(|(seen, _)| seen == step)
            .map(|(_, at)| *at)
            .collect()
    }

    fn quits(&self) -> Vec<u64> {
        self.times(&Step::Sent(QUIT.to_owned()))
    }

    fn kills(&self) -> Vec<u64> {
        self.times(&Step::Stopped(BrowserChildKind::Chromium))
    }

    /// Whether Chromium was asked to quit exactly once, before it was
    /// killed.
    fn quit_before_kill(&self) -> bool {
        let world = self.world();
        let place = |step: &Step| world.steps.iter().position(|(seen, _)| seen == step);
        let asked = place(&Step::Sent(QUIT.to_owned()));
        let killed = place(&Step::Stopped(BrowserChildKind::Chromium));
        drop(world);
        self.quits().len() == 1
            && asked
                .zip(killed)
                .is_some_and(|(asked, killed)| asked < killed)
    }

    fn state(&self) -> Result<BrowserState, BrowserError> {
        self.host.status().map(|status| status.state)
    }

    /// Whether the browser is gone and its status says so. The lock files
    /// are released before the status is written.
    fn closed(&self) -> bool {
        self.state()
            .is_ok_and(|state| matches!(state, BrowserState::Idle | BrowserState::Failed))
    }

    fn settled(
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

    /// Lets the manager's upkeep act on the clock as it is now.
    fn upkeep(&self) {
        for _ in 0..6 {
            let _ = self.host.maintenance();
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Whether the session that had the browser is told it was closed, both
    /// by the notice and by its next call.
    fn told_closed(&self) -> bool {
        self.host.closure_notice(OWNER).map(|notice| notice.error) == Some(BrowserError::Closed)
            && prepare(&self.host, OWNER, &json!({"action":"look"})).err()
                == Some(BrowserError::Closed)
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

fn prepare(
    host: &ManagedBrowserHost,
    session: &str,
    arguments: &Value,
) -> Result<BrowserPreparedAction, BrowserError> {
    calm(|| host.prepare(session, arguments))
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

/// Every way a browser closes (scope 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Assistant,
    Idle,
    ReplyWait,
    UserCommand,
    SessionEnd,
    Forget,
    Shutdown,
}

const WAYS: [Way; 7] = [
    Way::Assistant,
    Way::Idle,
    Way::ReplyWait,
    Way::UserCommand,
    Way::SessionEnd,
    Way::Forget,
    Way::Shutdown,
];

/// Closes the browser `OWNER` started on `rig`, and returns once it is gone.
fn close(home: &Home, rig: &Rig, way: Way) -> Result<(), Box<dyn Error>> {
    let failed = |status: &BrowserStatus| status.state == BrowserState::Failed;
    let idle = |status: &BrowserStatus| status.state == BrowserState::Idle;
    match way {
        Way::Assistant => close_by_assistant(&rig.host, &home.call(OWNER))?,
        Way::Idle => {
            rig.clock.advance(IDLE_MS);
            let status = rig.settled(failed)?;
            assert_eq!(status.reason, Some(BrowserError::IdleTimeout.to_string()));
        }
        Way::ReplyWait => {
            rig.host.turn_finished(OWNER);
            rig.settled(|status| status.reply_wait_deadline_ms.is_some())?;
            rig.clock.advance(REPLY_WAIT_MS);
            let status = rig.settled(failed)?;
            assert_eq!(
                status.reason,
                Some(BrowserError::ReplyWaitTimeout.to_string())
            );
        }
        Way::UserCommand => {
            assert_ne!(rig.host.close_by_user(), BrowserCloseOutcome::NothingOpen);
            rig.settled(idle)?;
        }
        Way::SessionEnd => {
            assert!(rig.host.close_for_session(OWNER));
            rig.settled(idle)?;
        }
        Way::Forget => {
            let (report, waited) =
                forget_beside(home, rig, ForgetConfirmation::Skip, &mut silent());
            assert!(report.succeeded, "{report:?}");
            assert_eq!(report.lines.first().map(String::as_str), Some(CLOSED));
            assert_eq!(waited, LOCK_CHECK_MS);
        }
        Way::Shutdown => rig.host.shutdown()?,
    }
    Ok(())
}

/// The prompt of a run that must not ask.
fn silent() -> ScriptedPrompt {
    ScriptedPrompt::answering(None)
}

fn run_forget(
    data: &Path,
    runner: &dyn BrowserProcessRunner,
    clock: &dyn BrowserClock,
    confirmation: ForgetConfirmation,
    prompt: &mut ScriptedPrompt,
) -> ForgetReport {
    browser::forget(
        &ForgetRequest {
            data_dir: data,
            confirmation,
            runner,
            clock,
        },
        prompt,
    )
}

/// `browser forget` as another process runs it while `rig` has a browser.
/// Each 100 ms forget waits is also 100 ms on the manager's clock, followed
/// by its upkeep. After the one-second look at the lock files it is waited
/// for the manager to finish closing, so forget's next try at `active.lock`
/// meets a settled manager. Returns what forget reported and how long it
/// waited on its own clock.
fn forget_beside(
    home: &Home,
    rig: &Rig,
    confirmation: ForgetConfirmation,
    prompt: &mut ScriptedPrompt,
) -> (ForgetReport, u64) {
    let mut waited = 0;
    let clock = Tick::with(move |ms| {
        waited += ms;
        rig.clock.advance(ms);
        let _ = rig.host.maintenance();
        if waited >= LOCK_CHECK_MS {
            let _ = eventually(|| rig.closed());
        }
    });
    let runner = rig.runner();
    let report = run_forget(home.data(), &runner, &clock, confirmation, prompt);
    (report, clock.now())
}

fn mode(path: &Path) -> io::Result<u32> {
    Ok(fs::symlink_metadata(path)?.mode() & 0o7777)
}

fn private_folder(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

fn seed(profile: &Path) -> io::Result<()> {
    private_folder(profile)?;
    fs::write(profile.join(format!("{KEPT}-cookies")), vec![b'c'; 1_536])?;
    fs::create_dir(profile.join("Default"))?;
    fs::write(
        profile.join("Default").join(format!("{KEPT}-logins")),
        [b'l'; 10],
    )
}

/// Everything at and under `path`: kind, size, link target, mode and
/// modification time. A symlink is listed, not followed.
fn listing(path: &Path) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut pending = vec![path.to_owned()];
    while let Some(next) = pending.pop() {
        let metadata = fs::symlink_metadata(&next)?;
        let kind = metadata.file_type();
        let target = if kind.is_symlink() {
            fs::read_link(&next)?.display().to_string()
        } else {
            String::new()
        };
        lines.push(format!(
            "{} folder={} bytes={} link={target} mode={:o} changed={}.{:09}",
            next.strip_prefix(path).unwrap_or(&next).display(),
            kind.is_dir(),
            if kind.is_file() { metadata.len() } else { 0 },
            metadata.mode() & 0o7777,
            metadata.mtime(),
            metadata.mtime_nsec(),
        ));
        if kind.is_dir() {
            for entry in fs::read_dir(&next)? {
                pending.push(entry?.path());
            }
        }
    }
    lines.sort();
    Ok(lines)
}

/// Whether a file named `name` is anywhere under `folder`.
fn holds(folder: &Path, name: &str) -> io::Result<bool> {
    Ok(listing(folder)?.iter().any(|line| {
        line.split(' ')
            .next()
            .is_some_and(|path| path.ends_with(name))
    }))
}

/// Another handle on a lock file, as another process has it.
fn handle(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}

fn held_exclusive(path: &Path) -> io::Result<File> {
    let file = handle(path)?;
    FileExt::try_lock_exclusive(&file)?;
    Ok(file)
}

fn held_shared(path: &Path) -> io::Result<File> {
    let file = handle(path)?;
    FileExt::try_lock_shared(&file)?;
    Ok(file)
}

/// Whether nobody holds the lock file at `path`.
fn free(path: &Path) -> io::Result<bool> {
    let file = handle(path)?;
    Ok(FileExt::try_lock_exclusive(&file).is_ok())
}

fn identity(path: &Path) -> io::Result<FileIdentity> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Puts another file where `path` is. Both exist for a moment, so the new
/// one cannot be given the old one's inode.
fn replace_file(path: &Path) -> io::Result<()> {
    let fresh = path.with_extension("fresh");
    drop(handle(&fresh)?);
    fs::rename(&fresh, path)
}

fn argument<'a>(spec: &'a BrowserChildSpec, name: &str) -> Option<&'a str> {
    spec.arguments
        .iter()
        .find_map(|argument| argument.strip_prefix(name))
}

/// `text` of a generation with what differs between two generations taken
/// out: the temporary folder, the display and the proxy port.
fn shape(text: &str, root: &Path) -> String {
    let text = text.replace(&root.display().to_string(), "<root>");
    ["--display=:", "--proxy-server=socks5://127.0.0.1:"]
        .into_iter()
        .find(|prefix| text.starts_with(prefix))
        .map_or(text.clone(), |prefix| format!("{prefix}N"))
}

fn shaped_arguments(spec: &BrowserChildSpec, root: &Path) -> Vec<String> {
    spec.arguments
        .iter()
        .map(|argument| shape(argument, root))
        .collect()
}

fn shaped_environment(spec: &BrowserChildSpec, root: &Path) -> BTreeMap<String, String> {
    spec.environment
        .iter()
        .map(|(name, value)| {
            let value = if name == "DISPLAY" {
                ":N".to_owned()
            } else {
                shape(value, root)
            };
            (name.clone(), value)
        })
        .collect()
}

fn lines(report: &ForgetReport) -> Vec<&str> {
    report.lines.iter().map(String::as_str).collect()
}

/// What a manager that was never shut down leaves in `/tmp`.
struct Leftover {
    root: PathBuf,
    display_lease: PathBuf,
}

impl Drop for Leftover {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.display_lease);
    }
}

#[test]
fn persist_login_uses_data_profile_and_keeps_it() -> Result<(), Box<dyn Error>> {
    let plain_home = Home::new()?;
    let plain = plain_home.rig(false)?;
    plain.start(&plain_home.call(OWNER))?;
    let plain_spec = plain.spec(0);
    let plain_root = plain.root(0);

    let home = Home::new()?;
    let owner = home.call(OWNER);
    let rig = home.rig(true)?;
    let profile = home.profile();
    assert!(!home.folder().root().exists());
    rig.start(&owner)?;
    let spec = rig.spec(0);
    let root = rig.root(0);

    let mut expected = shaped_arguments(&plain_spec, &plain_root);
    assert_eq!(expected, PLAIN_ARGUMENTS);
    expected[1] = format!("--user-data-dir={}", profile.display());
    let page = expected.len() - 1;
    expected.splice(
        page..page,
        [
            "--disk-cache-dir=<root>/cache".to_owned(),
            "--password-store=basic".to_owned(),
        ],
    );
    assert_eq!(shaped_arguments(&spec, &root), expected);
    assert_eq!(spec.program, plain_spec.program);
    assert_eq!(
        shaped_environment(&spec, &root),
        shaped_environment(&plain_spec, &plain_root)
    );
    assert!(spec.environment["HOME"].starts_with(&root.display().to_string()));

    let cache = PathBuf::from(argument(&spec, "--disk-cache-dir=").ok_or("no cache folder")?);
    assert_eq!(cache, root.join("cache"));
    assert!(cache.is_dir());
    assert!(!cache.starts_with(&profile));
    assert!(!cache.starts_with(home.data()));
    assert!(!root.starts_with(home.data()));
    assert!(spec
        .arguments
        .iter()
        .all(|argument| !argument.starts_with("--disk-cache-size")));

    assert_eq!(mode(home.folder().root())?, 0o700);
    assert_eq!(mode(&profile)?, 0o700);
    assert!(fs::symlink_metadata(&profile)?.file_type().is_dir());

    fs::write(profile.join(format!("{KEPT}-session")), b"signed in")?;
    close_by_assistant(&rig.host, &owner)?;
    assert!(!root.exists());
    assert_eq!(
        fs::read(profile.join(format!("{KEPT}-session")))?,
        b"signed in"
    );
    assert!(rig.quit_before_kill());

    // A profile that is there keeps the mode its user gave it.
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o750))?;
    let kept = listing(&profile)?;
    rig.start(&owner)?;
    let next_spec = rig.spec(1);
    let next_root = rig.root(1);
    assert_ne!(next_root, root);
    assert_eq!(shaped_arguments(&next_spec, &next_root), expected);
    assert_eq!(
        argument(&next_spec, "--disk-cache-dir="),
        Some(next_root.join("cache").to_string_lossy().as_ref())
    );
    assert!(next_root.join("cache").join(CACHED).is_file());
    assert_eq!(listing(&profile)?, kept);
    assert_eq!(mode(&profile)?, 0o750);
    close_by_assistant(&rig.host, &owner)?;
    assert!(!next_root.exists());
    assert_eq!(listing(&profile)?, kept);

    // The value in the config file reaches the manager the gateway builds.
    assert!(
        !ConfigBrowserStartup::new(home.bundle.clone())
            .config()
            .persist_login
    );
    assert!(
        ConfigBrowserStartup::new(home.configured(true)?)
            .config()
            .persist_login
    );
    assert!(
        !ConfigBrowserStartup::new(home.configured(false)?)
            .config()
            .persist_login
    );
    Ok(())
}

#[test]
fn http_cache_stays_outside_profile_and_is_removed_on_close() -> Result<(), Box<dyn Error>> {
    for way in WAYS {
        let home = Home::new()?;
        let rig = home.rig(true)?;
        rig.start(&home.call(OWNER))?;
        let root = rig.root(0);
        let cached = root.join("cache").join(CACHED);
        assert!(cached.is_file(), "{way:?}");
        assert!(!holds(&home.profile(), CACHED)?, "{way:?}");
        fs::write(home.profile().join(format!("{KEPT}-session")), b"signed in")?;

        close(&home, &rig, way)?;
        assert!(!cached.exists(), "{way:?}");
        assert!(!root.exists(), "{way:?}");
        if way == Way::Forget {
            assert!(!home.profile().exists());
        } else {
            assert!(
                home.profile().join(format!("{KEPT}-session")).is_file(),
                "{way:?}"
            );
            assert!(!holds(&home.profile(), CACHED)?, "{way:?}");
        }
    }

    // A manager that never got to close, as after SIGKILL: nothing of it
    // runs again. Its lock handles stay open inside this test process, which
    // a real kill would have released.
    let home = Home::new()?;
    let other = home.call(OTHER);
    let abandoned = home.rig(true)?;
    abandoned.start(&home.call(OWNER))?;
    let spec = abandoned.spec(0);
    let leftover = Leftover {
        root: abandoned.root(0),
        display_lease: PathBuf::from(format!(
            "/tmp/shacs-browser-display-{}",
            argument(&spec, "--display=:").ok_or("no display")?
        )),
    };
    std::mem::forget(abandoned);
    let cached = leftover.root.join("cache").join(CACHED);
    assert!(cached.is_file());
    let left = listing(&leftover.root)?;
    let profile = listing(&home.profile())?;

    let saved = home.rig(true)?;
    assert_eq!(
        saved.start(&other).err(),
        Some(BrowserError::ProfileInUse),
        "one profile never runs two browsers"
    );
    assert_eq!(saved.spawned(), 0);
    let plain = home.rig(false)?;
    plain.start(&other)?;
    assert_ne!(plain.root(0), leftover.root);
    assert_eq!(argument(&plain.spec(0), "--disk-cache-dir="), None);
    close_by_assistant(&plain.host, &other)?;
    assert!(!plain.root(0).exists());
    assert!(cached.is_file());
    assert_eq!(listing(&leftover.root)?, left);
    assert_eq!(listing(&home.profile())?, profile);
    Ok(())
}

#[test]
fn persist_login_off_leaves_profile_untouched() -> Result<(), Box<dyn Error>> {
    let home = Home::new()?;
    let owner = home.call(OWNER);
    let folder = home.folder();
    home.seed_profile()?;
    let before = listing(&home.profile())?;
    let rig = home.rig(false)?;
    rig.start(&owner)?;

    let spec = rig.spec(0);
    let root = rig.root(0);
    assert_eq!(shaped_arguments(&spec, &root), PLAIN_ARGUMENTS);
    assert_eq!(argument(&spec, "--disk-cache-dir="), None);
    assert_eq!(argument(&spec, "--password-store"), None);
    assert_eq!(
        argument(&spec, "--user-data-dir="),
        Some(root.join("profile").to_string_lossy().as_ref())
    );
    assert_eq!(listing(&home.profile())?, before);

    assert!(folder.active_lock().is_file());
    assert_eq!(mode(&folder.active_lock())?, 0o600);
    assert_eq!(mode(&folder.forget_lock())?, 0o600);
    assert_eq!(fs::metadata(folder.active_lock())?.len(), 0);
    assert!(!free(&folder.active_lock())?);
    assert!(free(&folder.forget_lock())?);
    assert!(!folder.profile_lock().exists());

    close_by_assistant(&rig.host, &owner)?;
    assert!(free(&folder.active_lock())?);
    assert!(!folder.profile_lock().exists());
    assert!(!root.exists());
    assert!(rig.quits().is_empty());
    assert_eq!(rig.kills().len(), 1);
    assert_eq!(listing(&home.profile())?, before);
    Ok(())
}

/// A start of `OWNER` on `rig` that must be refused before any process.
fn refused(
    home: &Home,
    rig: &Rig,
    case: &str,
    error: BrowserError,
    text: &str,
) -> Result<(), Box<dyn Error>> {
    let profile = home.profile();
    let before = listing(&profile).ok();
    let result = rig.start(&home.call(OWNER));
    assert_eq!(result.as_ref().err(), Some(&error), "{case}");
    assert_eq!(error.to_string(), text, "{case}");
    assert_eq!(rig.spawned(), 0, "{case}");
    let status = rig.host.status()?;
    assert_eq!(status.state, BrowserState::Failed, "{case}");
    assert_eq!(status.reason.as_deref(), Some(text), "{case}");
    assert_eq!(listing(&profile).ok(), before, "{case}");
    Ok(())
}

#[test]
fn locked_or_unreadable_profile_refuses_start_without_changes() -> Result<(), Box<dyn Error>> {
    let live = format!("{HOST}-{LIVE_PID}");
    let odd_pid = format!("{HOST}-007");
    for (case, target) in [
        ("another host", "other-box-4242"),
        ("this host, live Chromium", live.as_str()),
        ("no host and pid", "garbage"),
        ("pid not as Chromium writes it", odd_pid.as_str()),
    ] {
        let home = Home::new()?;
        home.seed_profile()?;
        symlink(target, home.profile().join("SingletonLock"))?;
        let rig = home.rig(true)?;
        rig.world().live_pids.push(LIVE_PID);
        refused(
            &home,
            &rig,
            case,
            BrowserError::ProfileLocked,
            PROFILE_LOCKED,
        )?;
        assert!(free(&home.folder().active_lock())?, "{case}");
        assert!(free(&home.folder().profile_lock())?, "{case}");
        assert_eq!(rig.clock.now(), 0, "{case}");
    }

    let home = Home::new()?;
    home.seed_profile()?;
    fs::write(home.profile().join("SingletonLock"), live.as_bytes())?;
    let rig = home.rig(true)?;
    refused(
        &home,
        &rig,
        "a lock that is no symlink",
        BrowserError::ProfileLocked,
        PROFILE_LOCKED,
    )?;
    assert!(free(&home.folder().active_lock())?);

    let home = Home::new()?;
    home.make_folder()?;
    let elsewhere = home.data().join("elsewhere");
    seed(&elsewhere)?;
    symlink(&elsewhere, home.profile())?;
    let target = listing(&elsewhere)?;
    let rig = home.rig(true)?;
    refused(
        &home,
        &rig,
        "a profile that is a symlink",
        BrowserError::ProfileUnreadable,
        PROFILE_UNREADABLE,
    )?;
    assert_eq!(listing(&elsewhere)?, target);
    assert!(free(&home.folder().active_lock())?);
    assert!(!home.folder().profile_lock().exists());

    let home = Home::new()?;
    home.seed_profile()?;
    let other_browser = held_exclusive(&home.folder().profile_lock())?;
    let rig = home.rig(true)?;
    refused(
        &home,
        &rig,
        "profile.lock held elsewhere",
        BrowserError::ProfileInUse,
        PROFILE_IN_USE,
    )?;
    assert_eq!(rig.clock.now(), 500);
    assert!(free(&home.folder().active_lock())?);
    drop(other_browser);

    // The lock of a Chromium of this host that is gone: Chromium clears it.
    let home = Home::new()?;
    home.seed_profile()?;
    symlink(&live, home.profile().join("SingletonLock"))?;
    let before = listing(&home.profile())?;
    let rig = home.rig(true)?;
    rig.start(&home.call(OWNER))?;
    assert_eq!(rig.state()?, BrowserState::Running);
    assert_eq!(listing(&home.profile())?, before);

    for persist in [true, false] {
        for forget_lock in [true, false] {
            let case = format!("persist={persist} forget.lock={forget_lock}");
            let home = Home::new()?;
            home.make_folder()?;
            let folder = home.folder();
            let lock = if forget_lock {
                folder.forget_lock()
            } else {
                folder.active_lock()
            };
            let forgetting = held_exclusive(&lock)?;
            let rig = home.rig(persist)?;
            refused(
                &home,
                &rig,
                &case,
                BrowserError::ForgetInProgress,
                FORGET_IN_PROGRESS,
            )?;
            assert_eq!(rig.clock.now(), 500, "{case}");
            assert!(!home.profile().exists(), "{case}");
            assert!(!folder.profile_lock().exists(), "{case}");
            if forget_lock {
                assert!(free(&folder.active_lock())?, "{case}");
            }
            drop(forgetting);
            rig.start(&home.call(OWNER))?;
            assert_eq!(rig.state()?, BrowserState::Running, "{case}");

            // A lock held for a moment, as inspect and the forget survey do.
            let home = Home::new()?;
            home.make_folder()?;
            let lock = if forget_lock {
                home.folder().forget_lock()
            } else {
                home.folder().active_lock()
            };
            let passing = Arc::new(Mutex::new(Some(held_exclusive(&lock)?)));
            let rig = home.rig(persist)?;
            let release = Arc::clone(&passing);
            let mut waited = 0;
            rig.clock.hook(move |ms| {
                waited += ms;
                if waited >= 300 {
                    if let Ok(mut held) = release.lock() {
                        held.take();
                    }
                }
            });
            rig.start(&home.call(OWNER))?;
            assert_eq!(rig.clock.now(), 300, "{case}");
            assert_eq!(rig.state()?, BrowserState::Running, "{case}");
        }

        let home = Home::new()?;
        let elsewhere = home.data().join("elsewhere");
        private_folder(&elsewhere)?;
        symlink(&elsewhere, home.folder().root())?;
        let rig = home.rig(persist)?;
        refused(
            &home,
            &rig,
            "browser/ is a symlink",
            BrowserError::StateUnavailable,
            STATE_UNAVAILABLE,
        )?;
        assert_eq!(fs::read_dir(&elsewhere)?.count(), 0, "persist={persist}");
    }
    Ok(())
}

#[test]
fn persist_generation_closes_gracefully_before_kill() -> Result<(), Box<dyn Error>> {
    for way in WAYS {
        let home = Home::new()?;
        let rig = home.rig(true)?;
        rig.start(&home.call(OWNER))?;
        assert!(rig.quits().is_empty(), "{way:?}");
        close(&home, &rig, way)?;
        assert!(rig.quit_before_kill(), "{way:?}");
        assert_eq!(rig.quits(), rig.kills(), "{way:?}: no wait when it ends");

        // The 049 profile has nothing to save: no question before the kill.
        let home = Home::new()?;
        let rig = home.rig(false)?;
        rig.start(&home.call(OWNER))?;
        close(&home, &rig, way)?;
        assert!(rig.quits().is_empty(), "{way:?}");
        assert_eq!(rig.kills().len(), 1, "{way:?}");
    }

    for way in [Way::Assistant, Way::UserCommand, Way::Shutdown] {
        let home = Home::new()?;
        let rig = home.rig(true)?;
        rig.start(&home.call(OWNER))?;
        rig.world().ignores_quit = true;
        close(&home, &rig, way)?;
        assert!(rig.quit_before_kill(), "{way:?}");
        assert_eq!(
            rig.kills(),
            [rig.quits()[0] + QUIT_MS],
            "{way:?}: killed five seconds after the question"
        );
        assert!(!rig.root(0).exists(), "{way:?}");
        assert!(free(&home.folder().profile_lock())?, "{way:?}");
        assert!(free(&home.folder().active_lock())?, "{way:?}");
    }

    // A Chromium that ended by itself has nobody to ask.
    let home = Home::new()?;
    let rig = home.rig(true)?;
    rig.start(&home.call(OWNER))?;
    rig.world().exits[0].store(true, Ordering::SeqCst);
    let status = rig.settled(|status| status.state == BrowserState::Failed)?;
    assert_eq!(status.reason, Some(BrowserError::ProcessExited.to_string()));
    assert!(rig.quits().is_empty());
    assert!(free(&home.folder().profile_lock())?);
    assert!(home.profile().is_dir());
    Ok(())
}

#[test]
fn browser_forget_confirms_closes_and_deletes() -> Result<(), Box<dyn Error>> {
    // Not confirmed: nothing is closed and nothing is removed.
    let home = Home::new()?;
    let folder = home.folder();
    let rig = home.rig(true)?;
    rig.start(&home.call(OWNER))?;
    fs::write(
        home.profile().join(format!("{KEPT}-cookies")),
        vec![b'c'; 1_536],
    )?;
    let before = listing(&home.profile())?;
    for (answer, said, succeeded) in [(None, NO_TERMINAL, false), (Some("n"), NOT_REMOVED, true)] {
        let mut prompt = ScriptedPrompt::answering(answer);
        let clock = Tick::default();
        let report = run_forget(
            home.data(),
            &rig.runner(),
            &clock,
            ForgetConfirmation::Ask,
            &mut prompt,
        );
        assert_eq!(lines(&report), [said]);
        assert_eq!(report.succeeded, succeeded);
        assert_eq!(report.stage, None);
        assert_eq!(
            prompt.shown,
            [
                format!("지울 폴더: {} (1.5 KB)", home.profile().display()),
                "떠 있는 브라우저: 닫습니다".to_owned()
            ]
        );
        assert_eq!(clock.now(), 0);
        rig.clock.advance(LOCK_CHECK_MS);
        rig.upkeep();
        assert_eq!(rig.state()?, BrowserState::Running);
        assert!(rig.kills().is_empty());
        assert_eq!(listing(&home.profile())?, before);
        assert!(free(&folder.forget_lock())?);
    }

    // `y`: the manager sees `forget.lock` held at its one-second look,
    // closes as scope 4 says and lets go of both of its locks.
    let mut prompt = ScriptedPrompt::answering(Some("y"));
    let (report, waited) = forget_beside(&home, &rig, ForgetConfirmation::Ask, &mut prompt);
    assert_eq!(lines(&report), [CLOSED, REMOVED]);
    assert!(report.succeeded);
    assert_eq!(report.stage, Some(ForgetStage::Removed));
    assert_eq!(waited, LOCK_CHECK_MS);
    assert_eq!(prompt.shown.len(), 2);
    assert!(rig.quit_before_kill());
    assert!(!rig.root(0).exists());
    assert!(!home.profile().exists());
    assert_eq!(mode(folder.root())?, 0o700);
    for lock in [
        folder.active_lock(),
        folder.forget_lock(),
        folder.profile_lock(),
    ] {
        assert!(lock.is_file());
        assert!(free(&lock)?);
    }
    assert_eq!(rig.state()?, BrowserState::Idle);
    assert!(rig.told_closed());
    rig.start(&home.call(OTHER))?;
    assert_eq!(fs::read_dir(home.profile())?.count(), 0);
    drop(rig);

    // `--yes` closes and removes without a question.
    let home = Home::new()?;
    let rig = home.rig(true)?;
    rig.start(&home.call(OWNER))?;
    let mut prompt = silent();
    let (report, waited) = forget_beside(&home, &rig, ForgetConfirmation::Skip, &mut prompt);
    assert_eq!(lines(&report), [CLOSED, REMOVED]);
    assert!(report.succeeded);
    assert_eq!(waited, LOCK_CHECK_MS);
    assert!(prompt.shown.is_empty());
    assert!(rig.quit_before_kill());
    assert!(!home.profile().exists());
    assert!(rig.told_closed());
    drop(rig);

    let home = Home::new()?;
    home.seed_profile()?;
    let mut prompt = ScriptedPrompt::answering(Some(" YES\n"));
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &Tick::default(),
        ForgetConfirmation::Ask,
        &mut prompt,
    );
    assert_eq!(lines(&report), [REMOVED]);
    assert_eq!(prompt.shown[1], "떠 있는 브라우저: 없음");
    assert!(!home.profile().exists());

    // While forget waits for a browser to close, no new one starts.
    let home = Home::new()?;
    home.seed_profile()?;
    let folder = home.folder();
    let closing = Mutex::new(Some(held_shared(&folder.active_lock())?));
    let starts = Mutex::new(Vec::new());
    let clock = Tick::with(|_| {
        let Some(closing) = closing.lock().ok().and_then(|mut closing| closing.take()) else {
            return;
        };
        for persist in [true, false] {
            let started = home.rig(persist).map(|rig| {
                let result = rig.start(&home.call(OTHER));
                (result.err(), rig.spawned(), rig.clock.now())
            });
            if let Ok(mut starts) = starts.lock() {
                starts.push(started.map_err(|error| error.to_string()));
            }
        }
        drop(closing);
    });
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &clock,
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(
        *starts.lock().map_err(|_| "starts")?,
        [
            Ok((Some(BrowserError::ForgetInProgress), 0, 500)),
            Ok((Some(BrowserError::ForgetInProgress), 0, 500))
        ]
    );
    assert_eq!(lines(&report), [CLOSED, REMOVED]);
    assert_eq!(clock.now(), 100);
    assert!(!home.profile().exists());

    // A browser that does not close within fifteen seconds: nothing is
    // removed.
    let home = Home::new()?;
    home.seed_profile()?;
    let folder = home.folder();
    let before = listing(&home.profile())?;
    let stuck = held_shared(&folder.active_lock())?;
    let clock = Tick::default();
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &clock,
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::ActiveLockBusy));
    assert_eq!(lines(&report), [NOT_CLOSED]);
    assert!(!report.succeeded);
    assert_eq!(clock.now(), 15_000);
    assert_eq!(listing(&home.profile())?, before);
    assert!(free(&folder.forget_lock())?);
    drop(stuck);

    let other_browser = held_exclusive(&folder.profile_lock())?;
    let clock = Tick::default();
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &clock,
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::ProfileLockBusy));
    assert_eq!(lines(&report), [NOT_CLOSED]);
    assert!(!report.succeeded);
    assert_eq!(clock.now(), 500);
    assert_eq!(listing(&home.profile())?, before);
    drop(other_browser);

    // A second forget beside one that runs.
    let first = held_exclusive(&folder.forget_lock())?;
    let clock = Tick::default();
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &clock,
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::ForgetLockBusy));
    assert_eq!(lines(&report), [OTHER_FORGET]);
    assert!(!report.succeeded);
    assert_eq!(clock.now(), 2_000);
    assert_eq!(listing(&home.profile())?, before);
    assert!(free(&folder.active_lock())?);
    drop(first);

    // Chromium's own lock names a Chromium of this host that runs.
    symlink(
        format!("{HOST}-{LIVE_PID}"),
        home.profile().join("SingletonLock"),
    )?;
    let before = listing(&home.profile())?;
    let report = run_forget(
        home.data(),
        &Runner::alone(&[LIVE_PID]),
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::SingletonLive));
    assert_eq!(lines(&report), [NOT_CLOSED]);
    assert!(!report.succeeded);
    assert!(report.held.is_some());
    assert_eq!(listing(&home.profile())?, before);
    for lock in [
        folder.active_lock(),
        folder.forget_lock(),
        folder.profile_lock(),
    ] {
        assert!(free(&lock)?);
    }
    // The same lock once that Chromium is gone.
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(lines(&report), [REMOVED]);
    assert!(!home.profile().exists());

    let home = Home::new()?;
    home.make_folder()?;
    let elsewhere = home.data().join("elsewhere");
    seed(&elsewhere)?;
    symlink(&elsewhere, home.profile())?;
    let target = listing(&elsewhere)?;
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::ProfileNotDirectory));
    assert_eq!(lines(&report), [NOT_A_FOLDER]);
    assert!(!report.succeeded);
    assert_eq!(listing(&elsewhere)?, target);
    assert_eq!(fs::read_link(home.profile())?, elsewhere);

    // The command itself, as the user types it.
    let home = Home::new()?;
    home.seed_profile()?;
    assert!(home.cli(&["browser", "forget", "--yes"])?.contains(REMOVED));
    assert!(!home.profile().exists());
    assert!(home.cli(&["browser", "forget", "-y"])?.contains(NOTHING));
    Ok(())
}

#[test]
fn start_refuses_profile_deleted_before_directory_flock() -> Result<(), Box<dyn Error>> {
    for replace_profile in [false, true] {
        let home = Home::new()?;
        home.seed_profile()?;
        let rig = home.rig(true)?;
        let directory = File::open(home.profile())?;
        FileExt::try_lock_exclusive(&directory)?;
        let mut held = Some(directory);
        let data = home.data().to_owned();
        let report = Arc::new(Mutex::new(None));
        let recorded = Arc::clone(&report);
        // The first directory flock cannot succeed. The existing clock hook runs
        // after open and before its retry, without adding a production test hook.
        rig.clock.hook(move |_| {
            if held.take().is_none() {
                return;
            }
            let folder = BrowserFolder::new(&data);
            replace_file(&folder.active_lock()).expect("replace active lock");
            replace_file(&folder.profile_lock()).expect("replace profile lock");
            *recorded.lock().expect("forget report") = Some(run_forget(
                &data,
                &Runner::alone(&[]),
                &Tick::default(),
                ForgetConfirmation::Skip,
                &mut silent(),
            ));
            if replace_profile {
                seed(&folder.profile()).expect("new inode at the same profile path");
            }
        });
        let started = rig.start(&home.call(OWNER));
        let report = report.lock().map_err(|_| "forget report")?;
        let report = report.as_ref().ok_or("directory wait hook did not run")?;
        assert_eq!(report.stage, Some(ForgetStage::Removed));
        assert!(report.succeeded, "{report:?}");
        assert_eq!(home.profile().exists(), replace_profile);
        assert_eq!(started, Err(BrowserError::ProfileUnreadable));
        assert_eq!(
            rig.spawned(),
            0,
            "an unlinked profile must never reach a child"
        );
        assert!(free(&home.folder().active_lock())?);
        assert!(free(&home.folder().profile_lock())?);
    }
    Ok(())
}

#[test]
fn forget_waits_for_temporary_browser_after_active_lock_replacement() -> Result<(), Box<dyn Error>>
{
    for saved_profile in [false, true] {
        let home = Home::new()?;
        if saved_profile {
            home.seed_profile()?;
        }
        let rig = home.rig(false)?;
        rig.start(&home.call(OWNER))?;
        let before = saved_profile
            .then(|| listing(&home.profile()))
            .transpose()?;
        replace_file(&home.folder().active_lock())?;
        assert!(free(&home.folder().active_lock())?);
        assert_eq!(
            rig.clock.now(),
            0,
            "maintenance must not see replacement yet"
        );
        // Spend five seconds waiting on the new active file, then the rest
        // on the live browser directory. These are one fifteen-second budget.
        {
            let mut active = Some(held_shared(&home.folder().active_lock())?);
            let mut elapsed = 0;
            let stalled = Tick::with(|ms| {
                elapsed += ms;
                if elapsed >= 5_000 {
                    active.take();
                }
            });
            let report = run_forget(
                home.data(),
                &rig.runner(),
                &stalled,
                ForgetConfirmation::Skip,
                &mut silent(),
            );
            assert_eq!(report.stage, Some(ForgetStage::BrowserDirectoryBusy));
            assert_eq!(lines(&report), [NOT_CLOSED]);
            assert!(!report.succeeded);
            assert_eq!(stalled.now(), 15_000);
            assert!(rig.kills().is_empty());
            assert_eq!(home.profile().exists(), saved_profile);
        }
        let clock = Tick::with(|ms| {
            assert!(
                rig.kills().is_empty(),
                "forget is still waiting for closure"
            );
            if let Some(before) = &before {
                assert_eq!(&listing(&home.profile()).expect("saved profile"), before);
            }
            rig.clock.advance(ms);
            let _ = rig.host.maintenance();
            if rig.clock.now() >= LOCK_CHECK_MS {
                eventually(|| rig.closed()).expect("browser must finish closing");
            }
        });
        let mut prompt = ScriptedPrompt::answering(Some("y"));
        let report = run_forget(
            home.data(),
            &rig.runner(),
            &clock,
            ForgetConfirmation::Ask,
            &mut prompt,
        );
        assert!(report.succeeded, "{report:?}");
        assert_eq!(
            clock.now(),
            LOCK_CHECK_MS,
            "forget must wait for maintenance"
        );
        assert_eq!(prompt.shown[1], "떠 있는 브라우저: 닫습니다");
        assert_eq!(
            lines(&report),
            [CLOSED, if saved_profile { REMOVED } else { NOTHING }]
        );
        assert_eq!(rig.kills().len(), 1);
        assert!(
            rig.quits().is_empty(),
            "temporary profiles keep the original close"
        );
        assert!(!rig.root(0).exists());
        assert!(!home.profile().exists());
        assert!(rig.told_closed());
    }
    // Forget's exclusive directory hold excludes new starts in both modes,
    // including after its sibling lock files have been replaced.
    for persist in [false, true] {
        let home = Home::new()?;
        home.make_folder()?;
        let directory = File::open(home.folder().root())?;
        FileExt::try_lock_exclusive(&directory)?;
        let rig = home.rig(persist)?;
        assert_eq!(
            rig.start(&home.call(OWNER)),
            Err(BrowserError::ForgetInProgress)
        );
        assert_eq!(rig.spawned(), 0);
        assert_eq!(rig.clock.now(), 500);
        assert!(free(&home.folder().active_lock())?);
    }
    Ok(())
}

#[test]
fn forget_cannot_delete_profile_before_chromium_creates_singleton() -> Result<(), Box<dyn Error>> {
    for replace_directory in [false, true] {
        let home = Home::new()?;
        home.seed_profile()?;
        let rig = home.rig(true)?;
        let before = listing(&home.profile())?;
        let data = home.data().to_owned();
        let (entered, paused) = mpsc::sync_channel(0);
        let (resume, resumed) = mpsc::sync_channel(0);
        rig.world().before_spawn = Some(Box::new(move || {
            entered.send(()).expect("announce lock-to-launch window");
            resumed
                .recv_timeout(Duration::from_secs(10))
                .expect("release launch");
        }));
        thread::scope(|scope| -> Result<(), Box<dyn Error>> {
            let starting = scope.spawn(|| rig.start(&home.call(OWNER)));
            paused.recv_timeout(Duration::from_secs(5))?;
            let report = (|| -> Result<ForgetReport, Box<dyn Error>> {
                let folder = BrowserFolder::new(&data);
                if replace_directory {
                    let moved = data.join("old-browser");
                    fs::rename(folder.root(), &moved)?;
                    private_folder(folder.root())?;
                    fs::rename(moved.join("profile"), folder.profile())?;
                } else {
                    replace_file(&folder.active_lock())?;
                    replace_file(&folder.profile_lock())?;
                }
                assert!(!folder.profile().join("SingletonLock").exists());
                Ok(run_forget(
                    &data,
                    &Runner::alone(&[]),
                    &Tick::default(),
                    ForgetConfirmation::Skip,
                    &mut silent(),
                ))
            })();
            resume.send(())?;
            starting.join().map_err(|_| "start panicked")??;
            let report = report?;
            assert!(
                !report.succeeded,
                "forget deleted the starting profile: {report:?}"
            );
            assert_eq!(
                report.stage,
                Some(if replace_directory {
                    ForgetStage::ProfileDirectoryBusy
                } else {
                    ForgetStage::BrowserDirectoryBusy
                })
            );
            assert_eq!(listing(&home.profile())?, before);
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn browser_forget_closes_temporary_profile_browser() -> Result<(), Box<dyn Error>> {
    for waiting in [false, true] {
        let home = Home::new()?;
        home.seed_profile()?;
        let rig = home.rig(false)?;
        rig.start(&home.call(OWNER))?;
        if waiting {
            rig.host.turn_finished(OWNER);
            rig.settled(|status| status.reply_wait_deadline_ms.is_some())?;
        }
        let (report, _) = forget_beside(&home, &rig, ForgetConfirmation::Skip, &mut silent());
        assert_eq!(lines(&report), [CLOSED, REMOVED]);
        assert!(report.succeeded);
        assert!(rig.quits().is_empty());
        assert!(!home.profile().exists());
        assert!(!rig.root(0).exists());
        assert!(rig.told_closed());
    }
    // An action holds the browser slot. Forget signals a close, which must
    // wait until that action returns rather than tearing it down underneath.
    let home = Home::new()?;
    home.seed_profile()?;
    let rig = home.rig(false)?;
    let session = acquire(&rig.host, &home.call(OWNER))?;
    let (entered, reading) = mpsc::sync_channel(0);
    let (release, released) = mpsc::sync_channel(0);
    rig.world().during_read = Some(Box::new(move || {
        entered.send(()).expect("reading");
        released
            .recv_timeout(Duration::from_secs(10))
            .expect("finish read");
    }));
    thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        let action = scope.spawn(|| read(session.as_ref(), &home.call(OWNER)));
        reading.recv_timeout(Duration::from_secs(5))?;
        let mut release = Some(release);
        let clock = Tick::with(|ms| {
            rig.clock.advance(ms);
            let _ = rig.host.maintenance();
            if rig.clock.now() >= LOCK_CHECK_MS {
                if let Some(release) = release.take() {
                    assert!(home.profile().exists());
                    assert!(rig.kills().is_empty());
                    release.send(()).expect("release action");
                }
                eventually(|| rig.closed()).expect("deferred close");
            }
        });
        let report = run_forget(
            home.data(),
            &rig.runner(),
            &clock,
            ForgetConfirmation::Skip,
            &mut silent(),
        );
        action.join().map_err(|_| "read panicked")??;
        assert_eq!(lines(&report), [CLOSED, REMOVED]);
        assert!(rig.told_closed());
        Ok(())
    })?;
    // The same real manager, with the existing real viewer state machine.
    let stage = Stage::with(Setup {
        state_dir: true,
        ..Setup::default()
    })?;
    let data = &stage.bundle.context.data_dir;
    let taken = takeover::taken(stage.take())?;
    let profile = BrowserFolder::new(data).profile();
    seed(&profile)?;
    let clock = Tick::with(|ms| {
        stage.set_clock(stage.now() + ms);
        let _ = stage.host.maintenance();
        if stage.now() >= LOCK_CHECK_MS {
            eventually(|| stage.host.episode_records().open.is_none()).expect("closed episode");
        }
    });
    let report = run_forget(
        data,
        &Runner::alone(&[]),
        &clock,
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(lines(&report), [CLOSED, REMOVED]);
    let records = stage.records();
    assert_eq!(records.settled.len(), 1);
    assert_eq!(records.settled[0].episode, taken.episode);
    assert_eq!(records.settled[0].reason, "closed");
    assert!(stage.host.episode_records().open.is_none());
    assert!(!profile.exists());
    Ok(())
}

#[test]
fn browser_forget_without_saved_profile_closes_running_browser() -> Result<(), Box<dyn Error>> {
    let home = Home::new()?;
    let mut prompt = ScriptedPrompt::answering(None);
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &Tick::default(),
        ForgetConfirmation::Ask,
        &mut prompt,
    );
    assert_eq!(lines(&report), [NOTHING]);
    assert!(report.succeeded);
    assert!(prompt.shown.is_empty());
    assert!(!home.folder().root().exists());
    let rig = home.rig(false)?;
    rig.start(&home.call(OWNER))?;
    let mut prompt = ScriptedPrompt::answering(Some("y"));
    let (report, _) = forget_beside(&home, &rig, ForgetConfirmation::Ask, &mut prompt);
    assert_eq!(
        prompt.shown,
        ["지울 폴더: 없음", "떠 있는 브라우저: 닫습니다"]
    );
    assert_eq!(lines(&report), [CLOSED, NOTHING]);
    assert!(report.succeeded);
    assert!(rig.told_closed());
    assert!(rig.quits().is_empty());
    Ok(())
}

#[test]
fn inspect_reports_login_profile_without_contents() -> Result<(), Box<dyn Error>> {
    let home = Home::new()?;
    home.configured(true)?;
    let inspect = || {
        shacs_cli::runtime_inspect(RuntimeInspectOptions {
            config_path: Some(home.config_path().to_owned()),
            workspace_override: Some(home.bundle.context.workspace.clone()),
        })
    };
    let absent = inspect()?
        .browser
        .login
        .ok_or("login absent from inspect")?;
    assert!(absent.persist);
    assert!(!absent.profile.exists);
    assert_eq!(absent.profile.state, BrowserProfileState::Absent);
    assert!(!home.folder().root().exists());
    home.make_folder()?;
    let forgetting = held_exclusive(&home.folder().forget_lock())?;
    assert_eq!(
        inspect()?.browser.login.ok_or("login")?.profile.state,
        BrowserProfileState::InUse
    );
    drop(forgetting);
    fs::remove_file(home.folder().forget_lock())?;
    fs::remove_dir(home.folder().root())?;
    home.seed_profile()?;
    let before = listing(home.folder().root())?;
    let ready = inspect()?.browser.login.ok_or("login missing")?;
    assert_eq!(ready.profile.path, home.profile().to_string_lossy());
    assert_eq!(ready.profile.size_bytes, KEPT_BYTES);
    assert!(!ready.profile.size_truncated);
    assert_eq!(ready.profile.state, BrowserProfileState::Ready);
    let text = shacs_cli::run_command(CliCommand::RuntimeInspect(RuntimeInspectOptions {
        config_path: Some(home.config_path().to_owned()),
        workspace_override: Some(home.bundle.context.workspace.clone()),
    }))?;
    assert!(text.contains("\"login\""));
    assert!(!text.contains(KEPT));
    let value = serde_json::to_value(inspect()?.browser)?;
    assert_eq!(value["login"]["profile"]["size_bytes"], KEPT_BYTES);
    assert!(!value.to_string().contains(KEPT));
    let diagnostics = shacs_cli::runtime_diagnostics(RuntimeDiagnosticsOptions {
        config_path: Some(home.config_path().to_owned()),
        workspace_override: Some(home.bundle.context.workspace.clone()),
        bundle_path: None,
    })?;
    assert_eq!(
        diagnostics.snapshot.runtime["browser"]["login"],
        json!({"persist":true,"profile":{"exists":true,"state":"ready"}})
    );
    assert!(!serde_json::to_string(&diagnostics.snapshot)?.contains(KEPT));
    assert_eq!(listing(home.folder().root())?, before);
    let lock = held_exclusive(&home.folder().profile_lock())?;
    assert_eq!(
        inspect()?.browser.login.ok_or("login")?.profile.state,
        BrowserProfileState::InUse
    );
    drop(lock);
    symlink("another-container-42", home.profile().join("SingletonLock"))?;
    assert_eq!(
        inspect()?.browser.login.ok_or("login")?.profile.state,
        BrowserProfileState::Locked
    );
    fs::remove_file(home.profile().join("SingletonLock"))?;
    fs::set_permissions(home.profile(), fs::Permissions::from_mode(0o000))?;
    assert_eq!(
        inspect()?.browser.login.ok_or("login")?.profile.state,
        BrowserProfileState::Unreadable
    );
    fs::set_permissions(home.profile(), fs::Permissions::from_mode(0o700))?;
    // Exercise onboard, not just serde round-tripping.
    shacs_cli::onboard(shacs_cli::OnboardOptions {
        config_path: Some(home.config_path().to_owned()),
        workspace: Some(home.bundle.context.workspace.clone()),
        wizard: false,
    })?;
    let configured: Value = serde_json::from_slice(&fs::read(home.config_path())?)?;
    assert_eq!(configured["tools"]["browser"]["persistLogin"], true);
    Ok(())
}

#[test]
fn forget_traverses_only_the_opened_profile_after_path_replacement() -> Result<(), Box<dyn Error>> {
    let home = Home::new()?;
    home.seed_profile()?;
    symlink(format!("{HOST}-42"), home.profile().join("SingletonLock"))?;
    let runner = Runner::alone(&[]);
    let profile = home.profile();
    let moved = home.data().join("old-profile");
    let moved_hook = moved.clone();
    runner.world.lock().map_err(|_| "world")?.during_hostname = Some(Box::new(move || {
        fs::rename(&profile, &moved_hook).expect("move the opened identity");
        seed(&profile).expect("replacement profile");
    }));
    let report = run_forget(
        home.data(),
        &runner,
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert!(
        !report.succeeded,
        "a replacement must not be reported removed"
    );
    assert_eq!(
        fs::read(home.profile().join(format!("{KEPT}-cookies")))?,
        vec![b'c'; 1536]
    );
    assert_eq!(
        fs::read_dir(&moved)?.count(),
        0,
        "only the opened identity was emptied"
    );
    Ok(())
}

#[test]
fn renamed_live_profile_survives_forget_of_its_replacement() -> Result<(), Box<dyn Error>> {
    let home = Home::new()?;
    let rig = home.rig(true)?;
    rig.start(&home.call(OWNER))?;
    fs::write(home.profile().join("cookie"), b"live")?;
    let moved = home.data().join("live-profile");
    fs::rename(home.profile(), &moved)?;
    seed(&home.profile())?;
    replace_file(&home.folder().active_lock())?;
    replace_file(&home.folder().profile_lock())?;
    let (report, waited) = forget_beside(&home, &rig, ForgetConfirmation::Skip, &mut silent());
    assert!(report.succeeded, "{report:?}");
    assert_eq!(waited, LOCK_CHECK_MS);
    assert_eq!(lines(&report), [CLOSED, REMOVED]);
    assert!(!home.profile().exists());
    assert_eq!(fs::read(moved.join("cookie"))?, b"live");
    assert_eq!(
        rig.kills().len(),
        1,
        "even a different profile waits for closure"
    );
    Ok(())
}

#[test]
fn profile_size_does_not_follow_links_and_stops_at_twenty_thousand_files(
) -> Result<(), Box<dyn Error>> {
    let home = Home::new()?;
    let outside = home.data().join("outside");
    seed(&outside)?;
    home.make_folder()?;
    symlink(&outside, home.profile())?;
    assert_eq!(browser::profile_size(&home.profile()).bytes, 0);
    fs::remove_file(home.profile())?;
    private_folder(&home.profile())?;
    symlink(&outside, home.profile().join("outside"))?;
    assert!(browser::profile_size(&home.profile()).bytes < KEPT_BYTES);
    for number in 0..20_001 {
        fs::write(home.profile().join(format!("{number}.data")), [1])?;
    }
    assert!(browser::profile_size(&home.profile()).truncated);
    Ok(())
}

#[test]
fn replaced_lock_path_closes_browser_and_forget_keeps_profile() -> Result<(), Box<dyn Error>> {
    for persist in [false, true] {
        for changed in [
            "unchanged",
            "active",
            "forget",
            "profile",
            "folder",
            "symlink",
        ] {
            if changed == "profile" && !persist {
                continue;
            }
            let home = Home::new()?;
            let rig = home.rig(persist)?;
            rig.start(&home.call(OWNER))?;
            let folder = home.folder();
            match changed {
                "unchanged" => {}
                "active" => replace_file(&folder.active_lock())?,
                "forget" => replace_file(&folder.forget_lock())?,
                "profile" => replace_file(&folder.profile_lock())?,
                "folder" | "symlink" => {
                    let moved = home.data().join("old-browser");
                    fs::rename(folder.root(), &moved)?;
                    if changed == "symlink" {
                        symlink(&moved, folder.root())?;
                    } else {
                        private_folder(folder.root())?;
                    }
                }
                _ => unreachable!(),
            }
            rig.clock.advance(LOCK_CHECK_MS);
            if changed == "unchanged" {
                rig.upkeep();
                assert_eq!(rig.state()?, BrowserState::Running);
                continue;
            }
            let status = rig.settled(|status| status.state == BrowserState::Failed)?;
            assert_eq!(status.reason.as_deref(), Some(LOCK_REPLACED_REASON));
            assert_eq!(
                home.inspected(rig.clock.now()).reason.as_deref(),
                Some(LOCK_REPLACED_REASON)
            );
            assert!(rig.told_closed());
            assert!(!rig.root(0).exists());
            assert_eq!(rig.quit_before_kill(), persist);
        }
    }
    // Neither sibling lock refers to the one held by the running generation.
    let home = Home::new()?;
    let rig = home.rig(true)?;
    rig.start(&home.call(OWNER))?;
    let folder = home.folder();
    let old_active = identity(&folder.active_lock())?;
    let old_profile = identity(&folder.profile_lock())?;
    replace_file(&folder.active_lock())?;
    replace_file(&folder.profile_lock())?;
    symlink(
        format!("{HOST}-{LIVE_PID}"),
        home.profile().join("SingletonLock"),
    )?;
    rig.world().live_pids.push(LIVE_PID);
    let before = listing(&home.profile())?;
    let report = run_forget(
        home.data(),
        &rig.runner(),
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::BrowserDirectoryBusy));
    assert_eq!(lines(&report), [NOT_CLOSED]);
    assert!(!report.succeeded);
    assert_eq!(
        report.held,
        Some(ForgetHeld {
            forget: identity(&folder.forget_lock())?,
            active: identity(&folder.active_lock())?,
            profile: identity(&folder.profile_lock())?,
        })
    );
    assert_ne!(identity(&folder.active_lock())?, old_active);
    assert_ne!(identity(&folder.profile_lock())?, old_profile);
    assert_eq!(listing(&home.profile())?, before);
    let busy = held_exclusive(&folder.profile_lock())?;
    let report = run_forget(
        home.data(),
        &rig.runner(),
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::ProfileLockBusy));
    drop(busy);
    // Even when the browser directory itself is replaced, the independent
    // SingletonLock check still refuses the same live Chromium profile.
    let moved = home.data().join("old-browser");
    fs::rename(folder.root(), &moved)?;
    private_folder(folder.root())?;
    fs::rename(moved.join("profile"), home.profile())?;
    let report = run_forget(
        home.data(),
        &rig.runner(),
        &Tick::default(),
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::SingletonLive));
    assert_eq!(lines(&report), [NOT_CLOSED]);
    assert!(!report.succeeded);
    assert_eq!(listing(&home.profile())?, before);
    drop(rig);

    // Replacement while forget is waiting is detected by its own identity check.
    let home = Home::new()?;
    home.seed_profile()?;
    let folder = home.folder();
    let before = listing(&home.profile())?;
    let mut active = Some(held_shared(&folder.active_lock())?);
    let clock = Tick::with(|_| {
        if active.take().is_some() {
            replace_file(&folder.forget_lock()).expect("replace forget lock");
        }
    });
    let report = run_forget(
        home.data(),
        &Runner::alone(&[]),
        &clock,
        ForgetConfirmation::Skip,
        &mut silent(),
    );
    assert_eq!(report.stage, Some(ForgetStage::LockReplaced));
    assert_eq!(lines(&report), [LOCK_REPLACED]);
    assert_eq!(listing(&home.profile())?, before);
    Ok(())
}

#[test]
fn browser_forget_refuses_profile_locked_by_other_host() -> Result<(), Box<dyn Error>> {
    for kind in ["other-host", "malformed", "regular"] {
        let home = Home::new()?;
        home.seed_profile()?;
        let singleton = home.profile().join("SingletonLock");
        match kind {
            "other-host" => symlink("other-container-42", &singleton)?,
            "malformed" => symlink("not-a-pid", &singleton)?,
            "regular" => fs::write(&singleton, "other-container-42")?,
            _ => unreachable!(),
        }
        let before = listing(&home.profile())?;
        let report = run_forget(
            home.data(),
            &Runner::alone(&[]),
            &Tick::default(),
            ForgetConfirmation::Skip,
            &mut silent(),
        );
        assert_eq!(report.stage, Some(ForgetStage::SingletonElsewhere));
        assert_eq!(lines(&report), [ELSEWHERE]);
        assert!(!report.succeeded);
        assert_eq!(listing(&home.profile())?, before);
        let rig = home.rig(false)?;
        rig.start(&home.call(OWNER))?;
        let (report, _) = forget_beside(&home, &rig, ForgetConfirmation::Skip, &mut silent());
        assert_eq!(lines(&report), [CLOSED, ELSEWHERE]);
        assert!(!report.succeeded);
        assert_eq!(listing(&home.profile())?, before);
        fs::remove_file(&singleton)?;
        let report = run_forget(
            home.data(),
            &Runner::alone(&[]),
            &Tick::default(),
            ForgetConfirmation::Skip,
            &mut silent(),
        );
        assert!(report.succeeded);
        assert!(!home.profile().exists());
    }
    Ok(())
}
