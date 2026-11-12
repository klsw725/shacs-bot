//! The real browser manager for the ingress tests, over fake processes and a
//! fake DevTools peer, on a clock only the test moves. A shorter copy of the
//! fakes in `tests/it/spec050_browser_takeover.rs`, which unit tests cannot
//! reach.

use crate::browser::{
    self, BrowserCancellation, BrowserChild, BrowserChildKind, BrowserChildSpec, BrowserClock,
    BrowserProcessRunner, BrowserStartup, BrowserViewerFactory, CdpReply, DevToolsPeer,
    ManagedBrowserHost, PausedTargetAdmission, ViewerListenAddress, ViewerListener,
};
use serde_json::{json, Value};
use shacs_config::{
    BrowserConfig, BrowserViewConfig, ConfigBundle, LoadOptions, UserLocalBrowserEnable,
};
use shacs_core::runtime::{
    self as runtime, BrowserError, BrowserStartEvidence, ContainmentSnapshotRef, NetworkGuard,
    ProcessContainmentProofCandidate, ProcessGateInput,
};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use std::{fs, thread};

const SECRET: &str = "view-secret-marker-A1b2";
/// What a fake adds to the clock each time the manager asks about a viewer
/// process that will not go away: four questions use up the two seconds the
/// manager allows.
const STOP_STALL_MS: u64 = 500;
/// The same for a viewer process that will not come up within ten seconds.
const START_STALL_MS: u64 = 2_500;

#[derive(Default)]
struct Clock(AtomicU64);

impl BrowserClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Tab {
    sequence: u64,
    url: String,
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
        let result = match method {
            "Target.getTargets" => json!({"targetInfos":[{"targetId":"target-1","type":"page"}]}),
            "Target.attachToTarget" => json!({"sessionId":"session-1"}),
            "Page.getFrameTree" => {
                json!({"frameTree":{"frame":{"id":"frame-1","loaderId":"loader-1","url":self.url}}})
            }
            "Page.navigate" => {
                self.url = params["url"].as_str().unwrap_or("").to_owned();
                json!({"frameId":"frame-1","loaderId":"loader-1"})
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    unreapable: bool,
}

impl Proc {
    fn gone(&self) -> bool {
        self.exited || (self.stopped && !self.unreapable)
    }
}

/// What goes wrong with the viewer processes started from now on.
#[derive(Default, Clone, Copy)]
pub(super) struct Faults {
    /// A websockify started next to a viewer that takes input never listens.
    pub(super) proxy_deaf_beside_input: bool,
    /// A viewer that takes input cannot be confirmed gone.
    pub(super) input_unreapable: bool,
    /// A view-only viewer cannot be confirmed gone.
    pub(super) view_only_unreapable: bool,
}

#[derive(Default)]
struct World {
    faults: Faults,
    procs: Vec<Proc>,
}

impl World {
    fn live(&self, kind: Kind) -> Option<&Proc> {
        self.procs
            .iter()
            .find(|proc| proc.kind == kind && !proc.gone())
    }
}

fn world(world: &Mutex<World>) -> MutexGuard<'_, World> {
    world.lock().expect("viewer facts")
}

struct ViewerProc {
    world: Arc<Mutex<World>>,
    clock: Arc<Clock>,
    index: usize,
}

impl ViewerProc {
    /// Whether it is still there, as asked after it was told to stop.
    fn lingers(&self) -> bool {
        let (stopped, gone) = {
            let world = world(&self.world);
            let proc = &world.procs[self.index];
            (proc.stopped, proc.gone())
        };
        if stopped && !gone {
            self.clock.0.fetch_add(STOP_STALL_MS, Ordering::SeqCst);
        }
        !gone
    }
}

impl BrowserChild for ViewerProc {
    fn process_id(&self) -> Option<u32> {
        Some(world(&self.world).procs[self.index].pid)
    }

    fn exited(&mut self) -> Result<bool, BrowserError> {
        Ok(!self.lingers())
    }

    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        Ok(false)
    }

    fn stop(&mut self) {
        world(&self.world).procs[self.index].stopped = true;
    }

    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        Err(BrowserError::Protocol)
    }

    fn group_alive(&mut self) -> bool {
        self.lingers()
    }
}

impl Drop for ViewerProc {
    fn drop(&mut self) {
        if let Ok(mut world) = self.world.lock() {
            world.procs[self.index].exited = true;
        }
    }
}

struct CoreProc {
    kind: BrowserChildKind,
    peer: Option<Box<dyn DevToolsPeer>>,
}

impl BrowserChild for CoreProc {
    fn exited(&mut self) -> Result<bool, BrowserError> {
        Ok(self.kind == BrowserChildKind::Xauth)
    }

    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        Ok(self.kind == BrowserChildKind::Xauth)
    }

    fn stop(&mut self) {}

    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        self.peer.take().ok_or(BrowserError::Protocol)
    }
}

struct Runner {
    world: Arc<Mutex<World>>,
    clock: Arc<Clock>,
}

impl BrowserProcessRunner for Runner {
    fn available(&self) -> bool {
        true
    }

    fn viewer_available(&self) -> bool {
        true
    }

    fn display_ready(&self, _display: u16, _deadline: Instant) -> Result<(), BrowserError> {
        Ok(())
    }

    fn viewer_listener(
        &self,
        address: ViewerListenAddress,
        port: u16,
        owner: Option<u32>,
    ) -> ViewerListener {
        let (answer, stall) = {
            let world = world(&self.world);
            if (address, port) == (ViewerListenAddress::Loopback, browser::VNC_PORT) {
                let holder = world
                    .procs
                    .iter()
                    .find(|proc| proc.kind != Kind::Proxy && proc.listens && !proc.gone());
                match holder {
                    Some(proc) if owner == Some(proc.pid) => (ViewerListener::OwnedByChild, 0),
                    Some(_) => (ViewerListener::Foreign, 0),
                    None => (ViewerListener::Absent, 0),
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
        self.clock.0.fetch_add(stall, Ordering::SeqCst);
        answer
    }

    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        _deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        if spec.program != browser::X11VNC && spec.program != browser::WEBSOCKIFY {
            return Ok(Box::new(CoreProc {
                kind: spec.kind,
                peer: (spec.kind == BrowserChildKind::Chromium).then(|| {
                    Box::new(Tab {
                        sequence: 0,
                        url: "about:blank".to_owned(),
                    }) as Box<dyn DevToolsPeer>
                }),
            }));
        }
        let mut world = world(&self.world);
        let faults = world.faults;
        let mut proc = Proc {
            kind: Kind::Proxy,
            pid: 7000 + world.procs.len() as u32,
            stopped: false,
            exited: false,
            listens: true,
            unreapable: false,
        };
        if spec.program == browser::WEBSOCKIFY {
            proc.listens = !(faults.proxy_deaf_beside_input && world.live(Kind::Input).is_some());
        } else {
            let input = !spec
                .arguments
                .iter()
                .any(|argument| argument == "-viewonly");
            proc.kind = if input { Kind::Input } else { Kind::ViewOnly };
            proc.unreapable = if input {
                faults.input_unreapable
            } else {
                faults.view_only_unreapable
            };
            // x11vnc reads its password file and removes it.
            let path = spec
                .arguments
                .iter()
                .position(|argument| argument == "-passwdfile")
                .and_then(|index| spec.arguments.get(index + 1))
                .and_then(|argument| argument.strip_prefix("rm:"))
                .map(PathBuf::from)
                .ok_or(BrowserError::Protocol)?;
            fs::remove_file(&path).map_err(|_| BrowserError::Protocol)?;
        }
        world.procs.push(proc);
        Ok(Box::new(ViewerProc {
            world: Arc::clone(&self.world),
            clock: Arc::clone(&self.clock),
            index: world.procs.len() - 1,
        }))
    }
}

struct Startup {
    config: BrowserConfig,
    input: ProcessGateInput,
}

impl BrowserStartup for Startup {
    fn config(&self) -> BrowserConfig {
        self.config.clone()
    }

    fn gate(&self, _generation: u64) -> Result<ProcessGateInput, BrowserError> {
        Ok(self.input.clone())
    }

    fn network_guard(&self) -> NetworkGuard {
        NetworkGuard::default()
    }
}

/// The start evidence of a contained gateway. Production observes /proc; the
/// test supplies those facts rather than treating its host as a container.
fn gate_input(bundle: &ConfigBundle) -> Result<ProcessGateInput, BrowserError> {
    let mut input = browser::browser_start_input(
        bundle,
        1,
        Some(ContainmentSnapshotRef {
            contained: Some(true),
            backend: Some("official-container".to_owned()),
            digest: Some("fixture".to_owned()),
            summary: None,
        }),
    )?;
    input.permission_rules.containment = runtime::DockerContainmentSnapshot {
        contained: Some(true),
        runtime: runtime::ContainerRuntimeKind::Docker,
        root_user: Some(false),
        privileged: Some(false),
        host_mounts_summary: Vec::new(),
        network_mode: runtime::ContainerNetworkMode::Bridge,
        digest: Some("fixture".to_owned()),
        summary: None,
    };
    input.envelope.action.permission_mode_snapshot.mode = bundle.config.permissions.mode();
    if let Some(inherited) = input.inherited_context.as_mut() {
        inherited.ceiling.parent_mode = bundle.config.permissions.mode();
        inherited.requested_mode = bundle.config.permissions.mode();
    }
    let snapshot = runtime::PolicySafetySnapshot::create(runtime::PolicySafetySnapshotInput {
        snapshot_id: "browser-test".to_owned(),
        created_at_unix_ms: 0,
        expires_at_unix_ms: None,
        permission_mode: input.envelope.action.permission_mode_snapshot.clone(),
        capability_ceiling: runtime::CapabilityCeilingRef {
            capabilities: vec![runtime::SafetyCapability::ProcExec],
        },
        containment: input.envelope.action.containment_snapshot.clone(),
        source_refs: Vec::new(),
        provenance_refs: Vec::new(),
        creation_reason: runtime::PolicySafetySnapshotCreationReason::PermissionedAction,
    })
    .map_err(|_| BrowserError::Protocol)?;
    input.envelope.policy_safety_snapshot_ref = snapshot.reference();
    input.envelope.action.policy_safety_snapshot_ref = Some(snapshot.reference());
    input.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(
        runtime::containment_permission_proof_for_process_gate(
            &input.envelope,
            &input.permission_rules,
            input.inherited_context.as_ref(),
            0,
        )
        .map_err(|_| BrowserError::Protocol)?,
    ));
    let enable =
        UserLocalBrowserEnable::read(&bundle.context.config_path, &bundle.context.workspace)
            .map_err(|_| BrowserError::StartDenied)?
            .ok_or(BrowserError::StartDenied)?;
    input.policy_materials.browser_start =
        Some(BrowserStartEvidence::bind(enable, &input.envelope));
    Ok(input)
}

/// The browser's own times, as `tools.browser` would give them.
#[derive(Clone, Copy)]
pub(super) struct Timing {
    pub(super) takeover_minutes: u64,
    pub(super) reply_wait_minutes: u64,
    pub(super) idle_timeout_seconds: u64,
}

impl Default for Timing {
    fn default() -> Self {
        let config = BrowserConfig::default();
        Self {
            takeover_minutes: config.takeover_minutes,
            reply_wait_minutes: config.reply_wait_minutes,
            idle_timeout_seconds: config.idle_timeout_seconds,
        }
    }
}

pub(super) struct Stage {
    pub(super) host: ManagedBrowserHost,
    world: Arc<Mutex<World>>,
    clock: Arc<Clock>,
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = self.host.shutdown();
    }
}

impl Stage {
    /// The manager a gateway would run for the config at `config_path`, with
    /// the view on.
    pub(super) fn start(config_path: &Path, timing: Timing) -> Result<Self, Box<dyn Error>> {
        let bundle = shacs_config::load_config(LoadOptions {
            config_path: Some(config_path.to_path_buf()),
            ..LoadOptions::default()
        })?;
        let view = BrowserViewConfig {
            enable: true,
            password: SECRET.to_owned(),
            url: String::new(),
        };
        let mut config = bundle.config.tools.browser.clone();
        config.view = view.clone();
        config.takeover_minutes = timing.takeover_minutes;
        config.reply_wait_minutes = timing.reply_wait_minutes;
        config.idle_timeout_seconds = timing.idle_timeout_seconds;
        let world = Arc::new(Mutex::new(World::default()));
        let clock = Arc::new(Clock::default());
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                config,
                input: gate_input(&bundle)?,
            }),
            Box::new(Runner {
                world: Arc::clone(&world),
                clock: Arc::clone(&clock),
            }),
            Arc::clone(&clock) as Arc<dyn BrowserClock>,
            None,
            Some(Box::new(BrowserViewerFactory::new(
                view,
                browser::GATEWAY_VIEWER_HOST_PORT,
            ))),
        );
        Ok(Self { host, world, clock })
    }

    pub(super) fn now(&self) -> u64 {
        self.clock.now_ms()
    }

    /// Moves the clock and lets the manager's upkeep see it.
    pub(super) fn advance(&self, milliseconds: u64) {
        self.clock.0.fetch_add(milliseconds, Ordering::SeqCst);
        self.upkeep();
    }

    pub(super) fn upkeep(&self) {
        for _ in 0..3 {
            let _ = self.host.maintenance();
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn fault(&self, faults: Faults) {
        world(&self.world).faults = faults;
    }
}
