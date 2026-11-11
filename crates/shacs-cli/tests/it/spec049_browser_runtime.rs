use serde_json::{json, Value};
#[cfg(unix)]
use shacs_cli::browser::CdpConnection;
use shacs_cli::browser::{
    self, BrowserCancellation, BrowserChild, BrowserChildKind, BrowserChildSpec, BrowserClock,
    BrowserConnector, BrowserEnvironment, BrowserLifecycleFactory, BrowserLifecycleParticipant,
    BrowserOwnerBinding, BrowserProcessRunner, BrowserResolver, BrowserSnapshotDestination,
    BrowserStartup, BrowserState, BrowserStateSnapshot, BrowserStatus, BrowserViewStatus,
    CdpDocument, CdpReply, DevToolsPeer, ManagedBrowserHost, PausedTargetAdmission, ViewerListener,
};
use shacs_cli::{
    RuntimeOwnerProcessEvidence, RuntimeOwnershipMarker, RuntimeOwnershipState,
    RuntimeOwnershipStatus,
};
use shacs_config::{
    self, BrowserConfig, Config, ConfigBundle, LoadOptions, PermissionMode, UserLocalBrowserEnable,
};
use shacs_core::runtime::{
    self as runtime, BrowserAction, BrowserCallContext, BrowserError, BrowserHost,
    BrowserStartEvidence, ContainmentSnapshotRef, NetworkGuard, ProcessAdapterKind,
    ProcessContainmentProofCandidate, ProcessGate, ProcessGateInput,
    ProcessGateTerminalPrecondition, ProcessSpawnReport, ProcessTerminalOutcome,
};
use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Clock(pub(super) AtomicU64);
impl BrowserClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Default)]
enum Change {
    #[default]
    None,
    Loader,
    Url,
    Internal,
    Navigation,
    Target,
    Detach,
    Chooser,
}

/// Shared with `spec049_browser_viewer`, which wraps this runner for the
/// xauth/Xvfb/Chromium children and handles the viewer children itself.
#[derive(Default)]
pub(super) struct Facts {
    pub(super) spawns: Vec<BrowserChildSpec>,
    pub(super) stops: Vec<BrowserChildKind>,
    commands: Vec<(String, Value)>,
    change: Change,
    fail_spawn: Option<BrowserChildKind>,
    pub(super) roots: Vec<PathBuf>,
    devtools: Option<Box<dyn DevToolsPeer>>,
    /// Events the browser sent between commands, handed out by the next poll.
    late_events: Vec<Value>,
}

struct Peer {
    facts: Arc<Mutex<Facts>>,
    sequence: u64,
    epoch: u64,
    frames: usize,
    clock: Arc<Clock>,
    advance_on_read: u64,
    cancellation: BrowserCancellation,
}

impl DevToolsPeer for Peer {
    fn set_cancellation(&mut self, cancellation: BrowserCancellation) {
        self.cancellation = cancellation;
    }
    fn request(
        &mut self,
        method: &str,
        params: Value,
        _session: Option<&str>,
        _deadline: Instant,
        _admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        self.cancellation.check()?;
        let mut facts = self
            .facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        facts.commands.push((method.to_owned(), params));
        self.sequence += 1;
        let mut events = Vec::new();
        let result = match method {
            "Target.getTargets" => json!({"targetInfos":[{"targetId":"target-1","type":"page"}]}),
            "Target.attachToTarget" => json!({"sessionId":"session-1"}),
            "Page.getFrameTree" => {
                self.frames += 1;
                let mut url = "https://example.com/";
                let mut loader = "loader-1";
                if self.frames.is_multiple_of(2) {
                    match facts.change {
                        Change::Loader => loader = "loader-2",
                        Change::Url => url = "https://other.example/",
                        Change::Internal => url = "file:///secret",
                        _ => {}
                    }
                }
                json!({"frameTree":{"frame":{"id":"frame-1","loaderId":loader,"url":url}}})
            }
            "Accessibility.getFullAXTree" => {
                let event = match facts.change {
                    Change::Navigation => Some(
                        json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame-1","url":"https://example.com/"}}}),
                    ),
                    Change::Target => Some(
                        json!({"method":"Target.attachedToTarget","params":{"sessionId":"session-2","targetInfo":{"targetId":"target-2","type":"page"}}}),
                    ),
                    Change::Detach => Some(
                        json!({"method":"Target.detachedFromTarget","params":{"sessionId":"session-1"}}),
                    ),
                    Change::Chooser => Some(json!({"method":"Page.fileChooserOpened","params":{}})),
                    _ => None,
                };
                if let Some(event) = event {
                    self.epoch += 1;
                    events.push(event);
                }
                self.clock
                    .0
                    .fetch_add(self.advance_on_read, Ordering::SeqCst);
                json!({"nodes":[{"backendDOMNodeId":1,"name":{"value":"PRIVATE PAGE TEXT"}}]})
            }
            "Runtime.evaluate" => {
                json!({"result":{"value":{"text":"PRIVATE PAGE TEXT","title":"PRIVATE TITLE"}}})
            }
            "DOM.getBoxModel" => json!({"model":{"content":[0,0,100,0,100,30,0,30]}}),
            "Page.getLayoutMetrics" => {
                json!({"cssVisualViewport":{"clientWidth":1280,"clientHeight":720}})
            }
            "Page.captureScreenshot" => json!({"data":"PRIVATE PNG"}),
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
        let mut facts = self
            .facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        Ok(std::mem::take(&mut facts.late_events))
    }
}

struct Child {
    kind: BrowserChildKind,
    facts: Arc<Mutex<Facts>>,
    dead: Arc<AtomicBool>,
    peer: Option<Box<dyn DevToolsPeer>>,
    stopped: bool,
}
impl BrowserChild for Child {
    fn exited(&mut self) -> Result<bool, BrowserError> {
        Ok(self.kind == BrowserChildKind::Xauth || self.dead.load(Ordering::SeqCst))
    }
    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        Ok(self.kind == BrowserChildKind::Xauth)
    }
    fn stop(&mut self) {
        if !self.stopped {
            self.facts
                .lock()
                .expect("fake child facts lock poisoned")
                .stops
                .push(self.kind);
            self.stopped = true;
        }
    }
    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        self.peer.take().ok_or(BrowserError::Protocol)
    }
}

pub(super) struct Runner {
    pub(super) facts: Arc<Mutex<Facts>>,
    pub(super) chromium_dead: Arc<AtomicBool>,
    pub(super) xvfb_dead: Arc<AtomicBool>,
    pub(super) clock: Arc<Clock>,
    pub(super) advance_start: u64,
    pub(super) advance_read: u64,
}
impl BrowserProcessRunner for Runner {
    fn available(&self) -> bool {
        true
    }
    fn viewer_available(&self) -> bool {
        true
    }
    fn viewer_listener(&self, _port: u16, _owner: Option<u32>) -> ViewerListener {
        ViewerListener::Absent
    }
    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        _deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        let mut facts = self
            .facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        if facts.fail_spawn == Some(spec.kind) {
            return Err(BrowserError::Unavailable);
        }
        facts.spawns.push(spec.clone());
        let root = Path::new(&spec.environment["HOME"])
            .parent()
            .ok_or(BrowserError::Protocol)?
            .to_path_buf();
        assert!(
            root.starts_with("/tmp/shacs-browser-")
                || root.to_string_lossy().starts_with("/tmp/shacs-browser-")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&root)
                    .map_err(|_| BrowserError::Protocol)?
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        facts.roots.push(root);
        self.clock.0.fetch_add(self.advance_start, Ordering::SeqCst);
        Ok(Box::new(Child {
            kind: spec.kind,
            facts: Arc::clone(&self.facts),
            stopped: false,
            dead: if spec.kind == BrowserChildKind::Xvfb {
                Arc::clone(&self.xvfb_dead)
            } else {
                Arc::clone(&self.chromium_dead)
            },
            peer: (spec.kind == BrowserChildKind::Chromium).then(|| {
                facts.devtools.take().unwrap_or_else(|| {
                    Box::new(Peer {
                        facts: Arc::clone(&self.facts),
                        sequence: 0,
                        epoch: 0,
                        frames: 0,
                        clock: Arc::clone(&self.clock),
                        advance_on_read: self.advance_read,
                        cancellation: BrowserCancellation::default(),
                    })
                })
            }),
        }))
    }
    fn display_ready(&self, _display: u16, _deadline: Instant) -> Result<(), BrowserError> {
        Ok(())
    }
}

pub(super) struct Startup {
    pub(super) config: BrowserConfig,
    pub(super) input: ProcessGateInput,
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

struct Viewer {
    dead: Arc<AtomicBool>,
    child: Option<Box<dyn BrowserChild>>,
}
struct ViewerFactory(Arc<AtomicBool>);
impl BrowserLifecycleFactory for ViewerFactory {
    fn create(&self) -> Box<dyn BrowserLifecycleParticipant> {
        Box::new(Viewer {
            dead: Arc::clone(&self.0),
            child: None,
        })
    }
    fn view(&self, _runner: &dyn BrowserProcessRunner) -> BrowserViewStatus {
        BrowserViewStatus::closed()
    }
}
impl BrowserLifecycleParticipant for Viewer {
    fn start(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        self.child = Some(runner.spawn(
            &environment.child(BrowserChildKind::Viewer, "fake-viewer", Vec::new()),
            deadline,
        )?);
        Ok(())
    }
    fn failed(&mut self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.stop();
        }
    }
    fn view(&self) -> BrowserViewStatus {
        BrowserViewStatus::closed()
    }
}

pub(super) fn bundle(root: &Path, mode: PermissionMode) -> Result<ConfigBundle, Box<dyn Error>> {
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace)?;
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    config.permissions.set_mode(mode);
    config.tools.browser.enable = true;
    let path = root.join("config.json");
    fs::write(&path, serde_json::to_vec(&config)?)?;
    Ok(shacs_config::load_config(LoadOptions {
        config_path: Some(path),
        ..LoadOptions::default()
    })?)
}

pub(super) fn input(bundle: &ConfigBundle) -> Result<ProcessGateInput, BrowserError> {
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
    // Production observes /proc. This test supplies those facts explicitly
    // rather than treating the macOS host as a container.
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

/// Tests that drive the transport directly never meet a paused target.
fn no_admission(
    _peer: &mut dyn DevToolsPeer,
    _event: &Value,
    _deadline: Instant,
) -> Result<Vec<Value>, BrowserError> {
    Ok(Vec::new())
}

#[cfg(unix)]
pub(super) fn host_with_devtools(
    root: &Path,
    peer: Box<dyn DevToolsPeer>,
) -> Result<ManagedBrowserHost, Box<dyn Error>> {
    let bundle = bundle(root, PermissionMode::Default)?;
    let facts = Arc::new(Mutex::new(Facts {
        devtools: Some(peer),
        ..Facts::default()
    }));
    let (host, _, _) = host(input(&bundle)?, facts, Arc::new(Clock::default()), 0, 0);
    Ok(host)
}

pub(super) fn context(input: &ProcessGateInput, owner: &str) -> BrowserCallContext {
    BrowserCallContext {
        effective_session_id: owner.to_owned(),
        permission_mode_snapshot: input.envelope.action.permission_mode_snapshot.clone(),
        permission_interactive: true,
        browser_screenshots: Some(true),
        native_image_input_supported: true,
    }
}

fn host(
    input: ProcessGateInput,
    facts: Arc<Mutex<Facts>>,
    clock: Arc<Clock>,
    advance_start: u64,
    advance_read: u64,
) -> (ManagedBrowserHost, Arc<AtomicBool>, Arc<AtomicBool>) {
    let chromium_dead = Arc::new(AtomicBool::new(false));
    let xvfb_dead = Arc::new(AtomicBool::new(false));
    let host = ManagedBrowserHost::new(
        Box::new(Startup {
            config: BrowserConfig {
                enable: true,
                ..BrowserConfig::default()
            },
            input,
        }),
        Box::new(Runner {
            facts,
            chromium_dead: Arc::clone(&chromium_dead),
            xvfb_dead: Arc::clone(&xvfb_dead),
            clock: Arc::clone(&clock),
            advance_start,
            advance_read,
        }),
        clock,
        None,
        None,
    );
    (host, chromium_dead, xvfb_dead)
}

#[test]
fn startup_gate_and_process_lifecycle() -> Result<(), Box<dyn Error>> {
    if std::env::var("SHACS_BROWSER_TEST_SECRETS").as_deref() != Ok("present") {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "spec049_browser_runtime::startup_gate_and_process_lifecycle",
                "--nocapture",
            ])
            .env("SHACS_BROWSER_TEST_SECRETS", "present")
            .env("OPENAI_API_KEY", "provider-secret-marker")
            .env("SHACS_BROWSER_VIEW_PASSWORD", "viewer-password-marker")
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    for (mode, interactive) in [
        (PermissionMode::Auto, true),
        (PermissionMode::Default, true),
        (PermissionMode::Auto, false),
        (PermissionMode::Default, false),
        (PermissionMode::BypassPermissions, false),
    ] {
        let root = tempfile::tempdir()?;
        let bundle = bundle(root.path(), mode)?;
        let mut input = input(&bundle)?;
        input.interactive = interactive;
        assert_eq!(input.envelope.action.permission_mode_snapshot.mode, mode);
        assert!(input.evaluator.is_none() && input.approval.is_none());
        let context = context(&input, "owner");
        let facts = Arc::new(Mutex::new(Facts::default()));
        let clock = Arc::new(Clock::default());
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                config: bundle.config.tools.browser.clone(),
                input: input.clone(),
            }),
            Box::new(Runner {
                facts: Arc::clone(&facts),
                chromium_dead: Arc::new(AtomicBool::new(false)),
                xvfb_dead: Arc::new(AtomicBool::new(false)),
                clock: Arc::clone(&clock),
                advance_start: 0,
                advance_read: 0,
            }),
            clock,
            None,
            Some(Box::new(ViewerFactory(Arc::new(AtomicBool::new(false))))),
        );
        let session = host.acquire(&context)?;
        session.act(
            &context,
            &BrowserAction::Navigate {
                url: "https://example.com".to_owned(),
            },
        )?;
        let read = session.read(&context)?;
        assert_eq!(read.visible_text, "PRIVATE PAGE TEXT");
        drop(session);
        let rebuilt_registry_handle = host.clone();
        let session = rebuilt_registry_handle.acquire(&context)?;
        let mut other = context.clone();
        other.effective_session_id = "other".to_owned();
        assert!(matches!(host.acquire(&other), Err(BrowserError::Busy)));
        assert_eq!(host.status()?.state, BrowserState::Busy);
        assert_eq!(host.receipt()?.ok_or("receipt missing")?.dispatch_count, 1);
        {
            let facts = facts.lock().unwrap();
            assert_eq!(facts.spawns.len(), 4);
            let chromium = &facts.spawns[2];
            assert!(chromium
                .arguments
                .iter()
                .any(|arg| arg == "--remote-debugging-pipe"));
            assert!(!chromium
                .arguments
                .iter()
                .any(|arg| arg.contains("remote-debugging-port")
                    || arg.contains("headless")
                    || arg.contains("disable-dev-shm-usage")));
            assert!(facts.spawns[1]
                .arguments
                .windows(2)
                .any(|args| args == ["-nolisten", "tcp"]));
            assert!(!facts.spawns[1].arguments.contains(&"-ac".to_owned()));
            for child in &facts.spawns {
                assert!(child.environment.keys().all(|key| [
                    "PATH",
                    "LANG",
                    "DISPLAY",
                    "XAUTHORITY",
                    "HOME",
                    "XDG_CONFIG_HOME",
                    "XDG_CACHE_HOME",
                    "XDG_DATA_HOME",
                    "XDG_RUNTIME_DIR",
                    "TMPDIR"
                ]
                .contains(&key.as_str())));
                assert!(!child.environment.contains_key("OPENAI_API_KEY"));
                assert!(!child
                    .environment
                    .contains_key("SHACS_BROWSER_VIEW_PASSWORD"));
                assert!(!format!("{:?}", child.environment).contains("secret-marker"));
                assert!(!format!("{:?}", child.environment).contains("password-marker"));
            }
        }
        session.close(&context)?;
        assert!(facts
            .lock()
            .unwrap()
            .roots
            .iter()
            .all(|root| !root.exists()));
        assert_eq!(
            facts.lock().unwrap().stops,
            [
                BrowserChildKind::Xauth,
                BrowserChildKind::Viewer,
                BrowserChildKind::Chromium,
                BrowserChildKind::Xvfb
            ]
        );

        for case in 0..8 {
            let mut denied = input.clone();
            match case {
                0 => denied.policy_materials.browser_start = None,
                1 => denied.containment_proof = ProcessContainmentProofCandidate::Missing,
                2 => {
                    denied
                        .permission_rules
                        .proc_exec_summary
                        .as_mut()
                        .ok_or("summary missing")?
                        .destructive = true
                }
                3 => denied.envelope.action.redacted_arguments = json!({"enable":true}),
                4 => denied.terminal_precondition = ProcessGateTerminalPrecondition::Replay,
                5 => {
                    denied
                        .envelope
                        .policy_safety_snapshot_ref
                        .expires_at_unix_ms = Some(0)
                }
                6 => denied.envelope.adapter = ProcessAdapterKind::McpStdio,
                _ => {
                    denied.permission_rules.containment =
                        runtime::DockerContainmentSnapshot::unknown();
                    denied.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(
                        runtime::containment_permission_proof_for_process_gate(
                            &denied.envelope,
                            &denied.permission_rules,
                            denied.inherited_context.as_ref(),
                            0,
                        )?,
                    ));
                }
            }
            if case == 5 {
                denied.now_unix_ms = 1;
            }
            if case == 6 && mode == PermissionMode::BypassPermissions {
                continue;
            }
            let spawns = AtomicUsize::new(0);
            let _ = ProcessGate::new().evaluate_and_maybe_spawn(denied, |_| {
                spawns.fetch_add(1, Ordering::SeqCst);
                ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
            });
            assert_eq!(
                spawns.load(Ordering::SeqCst),
                0,
                "case {case}, mode {mode:?}"
            );
        }
        let mut ask = input;
        ask.permission_rules.proc_exec_summary = None;
        let receipt = ProcessGate::new().evaluate_and_maybe_spawn(ask, |_| {
            ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
        })?;
        assert_eq!(receipt.dispatch_count, 1);
    }
    for (rule, list, denied) in [
        ("*", "deny", true),
        ("browser_startup", "deny", true),
        ("${SHACS_SPEC049_BROWSER_START_RULE}", "deny", true),
        ("web_fetch", "deny", false),
        ("*", "ask", false),
        ("${SHACS_SPEC049_BROWSER_START_RULE}", "ask", false),
    ] {
        let root = tempfile::tempdir()?;
        let original = bundle(root.path(), PermissionMode::Default)?;
        fs::write(
            root.path().join(".env.json"),
            r#"{"SHACS_SPEC049_BROWSER_START_RULE":"*"}"#,
        )?;
        let mut value: Value = serde_json::from_slice(&fs::read(&original.context.config_path)?)?;
        value["permissions"][list] = json!([rule]);
        fs::write(&original.context.config_path, serde_json::to_vec(&value)?)?;
        let mut configured = shacs_config::load_config(LoadOptions {
            config_path: Some(original.context.config_path.clone()),
            ..LoadOptions::default()
        })?;
        // A registry's cached/merged bundle cannot erase the selected file's deny.
        configured.config.permissions.user_rules.deny.clear();
        let gate = input(&configured)?;
        if list == "ask" {
            assert!(
                gate.policy_materials
                    .materials
                    .as_ref()
                    .and_then(|materials| materials.ask_rule.as_ref())
                    .is_some(),
                "the configured ask must be matched, then ignored only by browser admission"
            );
        }
        assert_eq!(
            gate.policy_materials
                .materials
                .as_ref()
                .and_then(|materials| materials.deny_rule.as_ref())
                .is_some(),
            denied
        );
        let context = context(&gate, "configured-owner");
        let facts = Arc::new(Mutex::new(Facts::default()));
        let (browser, _, _) = host(gate, Arc::clone(&facts), Arc::new(Clock::default()), 0, 0);
        let result = browser.acquire(&context);
        if denied {
            assert!(matches!(result, Err(BrowserError::StartDenied)));
            assert!(facts.lock().unwrap().spawns.is_empty());
            assert_eq!(
                browser
                    .receipt()?
                    .ok_or("missing denied receipt")?
                    .dispatch_count,
                0
            );
        } else {
            result?.close(&context)?;
            assert_eq!(facts.lock().unwrap().spawns.len(), 3);
        }
    }
    let root = tempfile::tempdir()?;
    let mut bundle = bundle(root.path(), PermissionMode::Default)?;
    let enabled_input = input(&bundle)?;
    let context = context(&enabled_input, "owner");
    let facts = Arc::new(Mutex::new(Facts::default()));
    let disabled = ManagedBrowserHost::new(
        Box::new(Startup {
            config: BrowserConfig::default(),
            input: enabled_input.clone(),
        }),
        Box::new(Runner {
            facts: Arc::clone(&facts),
            chromium_dead: Arc::new(AtomicBool::new(false)),
            xvfb_dead: Arc::new(AtomicBool::new(false)),
            clock: Arc::new(Clock::default()),
            advance_start: 0,
            advance_read: 0,
        }),
        Arc::new(Clock::default()),
        None,
        None,
    );
    assert!(matches!(
        disabled.acquire(&context),
        Err(BrowserError::Disabled)
    ));
    assert!(facts.lock().unwrap().spawns.is_empty());
    fs::write(
        &bundle.context.config_path,
        r#"{"tools":{"browser":{"enable":false}}}"#,
    )?;
    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(enabled_input, |_| {
        panic!("a stale user-local enable must not start a generation")
    })?;
    assert_eq!(receipt.dispatch_count, 0);
    assert!(browser::browser_start_input(&bundle, 2, None).is_err());
    bundle.context.config_path = bundle.context.workspace.join("config.json");
    fs::write(
        &bundle.context.config_path,
        r#"{"tools":{"browser":{"enable":true}}}"#,
    )?;
    assert!(browser::browser_start_input(&bundle, 2, None).is_err());
    Ok(())
}

struct Resolver {
    addresses: Vec<SocketAddr>,
    calls: AtomicUsize,
}
impl BrowserResolver for Resolver {
    fn resolve(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.addresses.clone())
    }
}
#[derive(Default)]
struct Connector(Mutex<Vec<SocketAddr>>);
impl BrowserConnector for Connector {
    type Connection = SocketAddr;
    fn connect(&self, address: SocketAddr) -> io::Result<SocketAddr> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("connector lock poisoned"))?
            .push(address);
        Ok(address)
    }
}
struct Duplex {
    input: io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}
impl Read for Duplex {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.input.read(bytes)
    }
}
impl Write for Duplex {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn proxy_checks_all_resolved_addresses_without_relookup() -> Result<(), Box<dyn Error>> {
    let public: SocketAddr = "93.184.216.34:443".parse()?;
    let resolver = Resolver {
        addresses: vec![public],
        calls: AtomicUsize::new(0),
    };
    let connector = Connector::default();
    assert_eq!(
        browser::guarded_connect(
            &NetworkGuard::default(),
            &resolver,
            &connector,
            "EXAMPLE.COM.",
            443
        )?,
        public
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(*connector.0.lock().unwrap(), [public]);
    for host in [
        "localhost",
        "x.LOCALHOST.",
        "127.0.0.1",
        "::1",
        "172.18.0.2",
    ] {
        assert!(browser::guarded_connect(
            &NetworkGuard::default(),
            &resolver,
            &connector,
            host,
            8765
        )
        .is_err());
    }
    let mixed = Resolver {
        addresses: vec![public, "10.0.0.2:443".parse()?],
        calls: AtomicUsize::new(0),
    };
    assert!(browser::guarded_connect(
        &NetworkGuard::default(),
        &mixed,
        &connector,
        "mixed.example",
        443
    )
    .is_err());
    assert_eq!(mixed.calls.load(Ordering::SeqCst), 1);
    assert_eq!(connector.0.lock().unwrap().len(), 1);
    let allowed = NetworkGuard::with_ssrf_whitelist(["172.18.0.2/32"]);
    for port in [80, 8900, 8765, 6080] {
        assert_eq!(
            browser::guarded_connect(&allowed, &resolver, &connector, "172.18.0.2", port)?.port(),
            port
        );
    }
    for command in [2, 3] {
        let mut stream = Duplex {
            input: io::Cursor::new(vec![5, 1, 0, 5, command, 0, 1]),
            output: Vec::new(),
        };
        assert!(browser::read_socks_connect(&mut stream).is_err());
        assert_eq!(&stream.output[2..4], [5, 7]);
    }
    Ok(())
}

#[test]
fn document_guard_discards_changed_and_internal_documents() -> Result<(), Box<dyn Error>> {
    for change in [
        Change::None,
        Change::Loader,
        Change::Url,
        Change::Internal,
        Change::Navigation,
        Change::Target,
        Change::Detach,
        Change::Chooser,
    ] {
        let facts = Arc::new(Mutex::new(Facts {
            change,
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
            clock: Arc::new(Clock::default()),
            advance_on_read: 0,
            cancellation: BrowserCancellation::default(),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut document = CdpDocument::initialize(Box::new(peer), deadline)?;
        let result = document.read(true, deadline);
        if matches!(change, Change::None) {
            assert_eq!(result?.screenshot.as_deref(), Some("PRIVATE PNG"));
        } else {
            let error = result
                .expect_err("changed document must never publish its payload")
                .to_string();
            assert!(!error.contains("PRIVATE") && !error.contains("file:"));
            if !matches!(change, Change::Detach) {
                assert!(facts
                    .lock()
                    .unwrap()
                    .commands
                    .iter()
                    .any(|(method, _)| method == "Target.closeTarget"));
            }
        }
        let commands = &facts.lock().unwrap().commands;
        assert!(commands
            .iter()
            .any(|(method, params)| method == "Browser.setDownloadBehavior"
                && params["behavior"] == "deny"));
        assert!(commands.iter().any(|(method, params)| method
            == "Page.setInterceptFileChooserDialog"
            && params["enabled"] == true));
        assert!(!format!("{commands:?}").contains("PRIVATE"));
    }
    Ok(())
}

/// A host that records its state file under the bundle's data directory.
fn recording_host(
    bundle: &ConfigBundle,
    input: ProcessGateInput,
    facts: Arc<Mutex<Facts>>,
) -> ManagedBrowserHost {
    let clock = Arc::new(Clock::default());
    ManagedBrowserHost::new(
        Box::new(Startup {
            config: BrowserConfig {
                enable: true,
                ..BrowserConfig::default()
            },
            input,
        }),
        Box::new(Runner {
            facts,
            chromium_dead: Arc::new(AtomicBool::new(false)),
            xvfb_dead: Arc::new(AtomicBool::new(false)),
            clock: Arc::clone(&clock),
            advance_start: 0,
            advance_read: 0,
        }),
        clock,
        Some(BrowserSnapshotDestination {
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
    )
}

#[test]
#[cfg(unix)]
fn busy_acquisition_publication_failure_disposes_the_generation() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), PermissionMode::Default)?;
    let input = input(&bundle)?;
    let facts = Arc::new(Mutex::new(Facts::default()));
    let host = recording_host(&bundle, input.clone(), Arc::clone(&facts));
    let _session = host.acquire(&context(&input, "owner"))?;
    let state_dir = bundle.context.data_dir.join("runtime");
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o500))?;
    let other = host.acquire(&context(&input, "other")).map(|_| ());
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700))?;
    // The busy state could not be recorded: the generation is not kept.
    assert_eq!(other, Err(BrowserError::StateUnavailable));
    assert_eq!(host.status()?.state, BrowserState::Failed);
    assert!(facts
        .lock()
        .expect("facts")
        .stops
        .contains(&BrowserChildKind::Chromium));
    Ok(())
}

#[test]
#[cfg(unix)]
fn state_publication_failure_disposes_the_generation() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), PermissionMode::Default)?;
    let input = input(&bundle)?;
    let context = context(&input, "owner");
    let facts = Arc::new(Mutex::new(Facts::default()));
    let data_dir = bundle.context.data_dir.clone();
    let host = recording_host(&bundle, input, Arc::clone(&facts));
    let session = host.acquire(&context)?;
    let state_dir = data_dir.join("runtime");
    assert!(state_dir.join("browser-state.json").is_file());
    // The state file can no longer be replaced: the next record fails, and a
    // browser whose state cannot be recorded is not kept.
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o500))?;
    let result = session.read(&context).map(|_| ());
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700))?;
    assert_eq!(result, Err(BrowserError::StateUnavailable));
    let status = host.status()?;
    assert_eq!(status.state, BrowserState::Failed);
    assert!(status
        .reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("browser_state_unavailable")));
    assert!(facts
        .lock()
        .expect("facts")
        .stops
        .contains(&BrowserChildKind::Chromium));
    Ok(())
}

#[test]
fn prepared_close_disposes_the_generation_on_a_fatal_error() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), PermissionMode::Default)?;
    let input = input(&bundle)?;
    let context = context(&input, "owner");
    let close = json!({"action":"close"});
    // A fatal event queued before the close ends the generation at once.
    let facts = Arc::new(Mutex::new(Facts::default()));
    let (fatal, _, _) = host(
        input.clone(),
        Arc::clone(&facts),
        Arc::new(Clock::default()),
        0,
        0,
    );
    let _session = fatal.acquire(&context)?;
    let prepared = fatal.prepare("owner", &close)?;
    facts
        .lock()
        .expect("facts")
        .late_events
        .push(json!({"method":"Page.fileChooserOpened","params":{}}));
    assert_eq!(
        fatal.close_prepared(&context, &prepared.digest),
        Err(BrowserError::FileTransferDenied)
    );
    let status = fatal.status()?;
    assert_eq!(status.state, BrowserState::Failed);
    assert!(status
        .reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("browser_file_transfer_denied")));
    {
        let facts = facts.lock().expect("facts");
        for kind in [BrowserChildKind::Chromium, BrowserChildKind::Xvfb] {
            assert!(facts.stops.contains(&kind), "{kind:?} still running");
        }
        assert!(
            facts.roots.iter().all(|path| !path.exists()),
            "profile remains"
        );
    }
    // A stale close approval keeps the browser; the right one closes it.
    let facts = Arc::new(Mutex::new(Facts::default()));
    let (stale, _, _) = host(input, Arc::clone(&facts), Arc::new(Clock::default()), 0, 0);
    let _session = stale.acquire(&context)?;
    let prepared = stale.prepare("owner", &close)?;
    assert_eq!(
        stale.close_prepared(&context, "not-the-approved-digest"),
        Err(BrowserError::StaleRevision)
    );
    assert_eq!(stale.status()?.state, BrowserState::Running);
    // Only the one-shot xauth helper has stopped; the browser itself runs on.
    assert!(!facts
        .lock()
        .expect("facts")
        .stops
        .contains(&BrowserChildKind::Chromium));
    stale.close_prepared(&context, &prepared.digest)?;
    assert_eq!(stale.status()?.state, BrowserState::Idle);
    assert!(facts
        .lock()
        .expect("facts")
        .stops
        .contains(&BrowserChildKind::Chromium));
    Ok(())
}

#[test]
#[cfg(unix)]
fn pipe_receive_order_prevents_event_response_race() -> Result<(), Box<dyn Error>> {
    use std::os::unix::net::UnixStream;
    use std::thread;
    let (client, mut server) = UnixStream::pair()?;
    let mut connection = CdpConnection::new(client.try_clone()?.into(), client.into())?;
    let peer = thread::spawn(move || -> io::Result<()> {
        for id in 1..=2 {
            let mut bytes = Vec::new();
            loop {
                let mut byte = [0];
                server.read_exact(&mut byte)?;
                if byte[0] == 0 {
                    break;
                }
                bytes.push(byte[0]);
            }
            let request: Value = serde_json::from_slice(&bytes)?;
            assert_eq!(request["id"], id);
            let response = json!({"id":id,"sessionId":"page-session","result":{"ok":true}});
            let mut batch = Vec::new();
            if id == 2 {
                batch.extend(serde_json::to_vec(&json!({"method":"Page.frameNavigated","sessionId":"page-session","params":{"frame":{"url":"file:///secret"}}}))?);
                batch.push(0);
            }
            batch.extend(serde_json::to_vec(&response)?);
            batch.push(0);
            // Both messages are already received in one read, before the caller
            // wakes. There is no independently scheduled event queue to race.
            server.write_all(&batch)?;
        }
        Ok(())
    });
    let first = connection.request(
        "Page.getFrameTree",
        json!({}),
        Some("page-session"),
        Instant::now() + Duration::from_secs(5),
        &mut no_admission,
    )?;
    let last = connection.request(
        "Page.getFrameTree",
        json!({}),
        Some("page-session"),
        Instant::now() + Duration::from_secs(5),
        &mut no_admission,
    )?;
    assert_eq!(last.document_epoch, first.document_epoch + 1);
    assert_eq!(last.sequence, first.sequence + 2);
    assert_eq!(last.events.len(), 1);
    peer.join().map_err(|_| "peer panicked")??;
    Ok(())
}

#[test]
fn timeouts_cleanup_and_owner_snapshot() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), PermissionMode::Default)?;
    let input = input(&bundle)?;
    let context = context(&input, "owner");
    for scenario in 0..6 {
        let facts = Arc::new(Mutex::new(Facts::default()));
        if scenario == 3 {
            facts.lock().unwrap().fail_spawn = Some(BrowserChildKind::Chromium);
        }
        let clock = Arc::new(Clock::default());
        let (host, chromium_dead, xvfb_dead) = host(
            input.clone(),
            Arc::clone(&facts),
            Arc::clone(&clock),
            if scenario == 0 { 7000 } else { 0 },
            if scenario == 2 { 31_000 } else { 0 },
        );
        let acquired = host.acquire(&context);
        match scenario {
            0 => assert!(matches!(acquired, Err(BrowserError::StartupTimeout))),
            3 => assert!(matches!(acquired, Err(BrowserError::Unavailable))),
            _ => {
                let session = acquired?;
                match scenario {
                    1 => {
                        clock.0.store(301_000, Ordering::SeqCst);
                        host.maintenance()?;
                    }
                    2 => assert_eq!(
                        session.act(
                            &context,
                            &BrowserAction::Navigate {
                                url: "https://example.com/".to_owned()
                            }
                        ),
                        Err(BrowserError::PageTimeout)
                    ),
                    4 => {
                        chromium_dead.store(true, Ordering::SeqCst);
                        host.maintenance()?;
                    }
                    _ => {
                        xvfb_dead.store(true, Ordering::SeqCst);
                        host.maintenance()?;
                    }
                }
            }
        }
        assert!(facts
            .lock()
            .unwrap()
            .roots
            .iter()
            .all(|path| !path.exists()));
        assert!(facts
            .lock()
            .unwrap()
            .stops
            .contains(&BrowserChildKind::Xvfb));
    }
    let facts = Arc::new(Mutex::new(Facts::default()));
    let viewer_dead = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(Clock::default());
    let runner = Runner {
        facts: Arc::clone(&facts),
        chromium_dead: Arc::new(AtomicBool::new(false)),
        xvfb_dead: Arc::new(AtomicBool::new(false)),
        clock: Arc::clone(&clock),
        advance_start: 0,
        advance_read: 0,
    };
    let host = ManagedBrowserHost::new(
        Box::new(Startup {
            config: bundle.config.tools.browser.clone(),
            input,
        }),
        Box::new(runner),
        clock,
        None,
        Some(Box::new(ViewerFactory(Arc::clone(&viewer_dead)))),
    );
    let session = host.acquire(&context)?;
    viewer_dead.store(true, Ordering::SeqCst);
    host.maintenance()?;
    assert_eq!(session.read(&context)?.visible_text, "PRIVATE PAGE TEXT");
    assert_eq!(
        facts
            .lock()
            .unwrap()
            .spawns
            .last()
            .ok_or("viewer missing")?
            .kind,
        BrowserChildKind::Viewer
    );
    session.close(&context)?;

    let marker = RuntimeOwnershipMarker {
        schema_version: 1,
        owner_id: "owner-a".to_owned(),
        pid: std::process::id(),
        started_at_ms: 100,
        updated_at_ms: 200,
        expires_at_ms: 1000,
        lifecycle: "running".to_owned(),
        binary_version: "test".to_owned(),
        data_schema_version: 1,
        mode: "gateway".to_owned(),
        config_path: bundle.context.config_path.to_string_lossy().to_string(),
        workspace: bundle.context.workspace.to_string_lossy().to_string(),
        lock_protocol: None,
        process_evidence: RuntimeOwnerProcessEvidence {
            pid: std::process::id(),
            pid_alive: true,
            process_started_after_marker: false,
        },
    };
    let binding = BrowserOwnerBinding::from_marker(&marker)?;
    let snapshot = BrowserStateSnapshot {
        owner: binding,
        generation: 1,
        holder_present: true,
        status: BrowserStatus::new(true, true, 250),
    };
    browser::write_browser_snapshot(root.path(), &snapshot)?;
    let ownership = RuntimeOwnershipStatus {
        state: RuntimeOwnershipState::Active,
        marker: Some(marker.clone()),
        reason: "fixture".to_owned(),
    };
    let read = |ownership: &RuntimeOwnershipStatus, now| {
        browser::read_browser_snapshot(
            root.path(),
            ownership,
            &bundle.context.config_path,
            &bundle.context.workspace,
            BrowserStatus::new(true, true, now),
            now,
        )
    };
    assert_eq!(read(&ownership, 300).state, BrowserState::Busy);
    assert!(read(&ownership, 300).owner_observed);
    let mut previous = ownership.clone();
    previous.marker.as_mut().ok_or("marker missing")?.owner_id = "owner-b".to_owned();
    assert_eq!(read(&previous, 300).state, BrowserState::Unknown);
    assert_eq!(read(&ownership, 1001).state, BrowserState::Unknown);
    fs::write(root.path().join("runtime/browser-state.json"), "broken")?;
    assert_eq!(read(&ownership, 300).state, BrowserState::Unknown);
    Ok(())
}

#[test]
fn onboard_and_inspect_browser() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), PermissionMode::Default)?;
    shacs_cli::onboard(shacs_cli::OnboardOptions {
        config_path: Some(bundle.context.config_path.clone()),
        workspace: Some(bundle.context.workspace.clone()),
        wizard: false,
    })?;
    let saved: Value = serde_json::from_slice(&fs::read(&bundle.context.config_path)?)?;
    assert_eq!(saved["tools"]["browser"]["enable"], true);
    assert_eq!(saved["tools"]["browser"]["startupTimeoutSeconds"], 20);
    let report = shacs_cli::runtime_inspect(shacs_cli::RuntimeInspectOptions {
        config_path: Some(bundle.context.config_path.clone()),
        workspace_override: Some(bundle.context.workspace.clone()),
    })?;
    let value = serde_json::to_value(report.browser)?;
    assert_eq!(value["enabled"], true);
    assert_eq!(value["ownerObserved"], false);
    assert_eq!(value["state"], "unknown");
    let diagnostics = shacs_cli::runtime_diagnostics(shacs_cli::RuntimeDiagnosticsOptions {
        config_path: Some(bundle.context.config_path.clone()),
        workspace_override: Some(bundle.context.workspace.clone()),
        bundle_path: None,
    })?;
    assert_eq!(
        diagnostics.snapshot.runtime["browser"]["state"],
        value["state"]
    );
    assert_eq!(
        diagnostics.snapshot.runtime["browser"]["enabled"],
        value["enabled"]
    );
    assert!(!diagnostics.snapshot.runtime["browser"]
        .to_string()
        .contains("PRIVATE"));
    for key in ["available", "reason", "updatedAtMs"] {
        assert!(value.get(key).is_some());
    }
    let docker = include_str!("../../../../Dockerfile");
    assert!(
        docker.contains("chromium")
            && docker.contains("xvfb")
            && docker.contains("xauth")
            && docker.contains("fonts-noto-cjk")
    );
    assert!(
        docker.contains("--chown=root:root --chmod=0644")
            && docker.contains("/etc/chromium/policies/managed/shacs-browser.json")
    );
    let policy: Value =
        serde_json::from_str(include_str!("../../../../docker/shacs-browser.json"))?;
    assert_eq!(
        policy,
        json!({"URLBlocklist":["*"],"URLAllowlist":["http://*","https://*"],"DownloadRestrictions":3,"DefaultFileSystemReadGuardSetting":2,"DefaultFileSystemWriteGuardSetting":2})
    );
    let compose = include_str!("../../../../docker-compose.yml");
    assert_eq!(compose.matches("memory: 2G").count(), 2);
    assert_eq!(compose.matches("memory: 512M").count(), 2);
    assert_eq!(compose.matches("shm_size: \"256mb\"").count(), 2);
    Ok(())
}

#[cfg(unix)]
struct StalledRunner {
    runner: Runner,
    connection: Mutex<Option<CdpConnection>>,
    entered: std::sync::mpsc::Sender<()>,
}

#[cfg(unix)]
impl BrowserProcessRunner for StalledRunner {
    fn available(&self) -> bool {
        true
    }

    fn viewer_available(&self) -> bool {
        self.runner.viewer_available()
    }

    fn viewer_listener(&self, port: u16, owner: Option<u32>) -> ViewerListener {
        self.runner.viewer_listener(port, owner)
    }

    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        let child = self.runner.spawn(spec, deadline)?;
        if spec.kind != BrowserChildKind::Chromium {
            return Ok(child);
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .take()
            .ok_or(BrowserError::Protocol)?;
        Ok(Box::new(StalledChild {
            child,
            connection: Some(connection),
            entered: self.entered.clone(),
        }))
    }

    fn display_ready(&self, display: u16, deadline: Instant) -> Result<(), BrowserError> {
        self.runner.display_ready(display, deadline)
    }
}

#[cfg(unix)]
struct StalledChild {
    child: Box<dyn BrowserChild>,
    connection: Option<CdpConnection>,
    entered: std::sync::mpsc::Sender<()>,
}

#[cfg(unix)]
impl BrowserChild for StalledChild {
    fn exited(&mut self) -> Result<bool, BrowserError> {
        self.child.exited()
    }
    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        self.child.succeeded()
    }
    fn stop(&mut self) {
        self.child.stop();
    }
    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        Ok(Box::new(StalledPeer {
            setup: self.child.take_devtools()?,
            connection: self.connection.take().ok_or(BrowserError::Protocol)?,
            entered: self.entered.clone(),
        }))
    }
}

#[cfg(unix)]
struct StalledPeer {
    setup: Box<dyn DevToolsPeer>,
    connection: CdpConnection,
    entered: std::sync::mpsc::Sender<()>,
}

#[cfg(unix)]
impl DevToolsPeer for StalledPeer {
    fn set_cancellation(&mut self, cancellation: BrowserCancellation) {
        self.setup.set_cancellation(cancellation.clone());
        self.connection.set_cancellation(cancellation);
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        deadline: Instant,
        admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        if method == "Input.insertText" {
            self.entered.send(()).map_err(|_| BrowserError::Protocol)?;
            self.connection
                .request(method, params, session, deadline, admit)
        } else {
            self.setup.request(method, params, session, deadline, admit)
        }
    }

    fn drain_events(&mut self) -> Result<Vec<Value>, BrowserError> {
        self.setup.drain_events()
    }
}

#[cfg(unix)]
fn test_pipe() -> io::Result<(fs::File, fs::File)> {
    use std::os::fd::FromRawFd;
    let mut descriptors = [0; 2];
    // SAFETY: pipe writes two descriptors into a valid two-element array.
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the two new descriptors are each transferred to exactly one File.
    Ok(unsafe {
        (
            fs::File::from_raw_fd(descriptors[0]),
            fs::File::from_raw_fd(descriptors[1]),
        )
    })
}

#[cfg(unix)]
#[test]
fn stalled_pipe_write_times_out_and_shutdown_cancels_with_cleanup() -> Result<(), Box<dyn Error>> {
    use std::os::fd::AsRawFd;
    use std::sync::mpsc;
    use std::thread;

    for shutdown in [false, true] {
        let root = tempfile::tempdir()?;
        let bundle = bundle(root.path(), PermissionMode::Default)?;
        let input = input(&bundle)?;
        let context = context(&input, "stalled-owner");
        // The fake peer keeps BOTH ends open throughout the assertion and
        // deliberately never drains the browser's command pipe.
        let (peer_reader, command_writer) = test_pipe()?;
        let (response_reader, _peer_writer) = test_pipe()?;
        let connection = CdpConnection::new(response_reader.into(), command_writer.into())?;
        let facts = Arc::new(Mutex::new(Facts::default()));
        let clock = Arc::new(Clock::default());
        let (entered, started) = mpsc::channel();
        let host = ManagedBrowserHost::new(
            Box::new(Startup {
                config: BrowserConfig {
                    enable: true,
                    page_timeout_seconds: 5,
                    ..BrowserConfig::default()
                },
                input,
            }),
            Box::new(StalledRunner {
                runner: Runner {
                    facts: Arc::clone(&facts),
                    clock: Arc::clone(&clock),
                    chromium_dead: Arc::new(AtomicBool::new(false)),
                    xvfb_dead: Arc::new(AtomicBool::new(false)),
                    advance_start: 0,
                    advance_read: 0,
                },
                connection: Mutex::new(Some(connection)),
                entered,
            }),
            clock,
            None,
            None,
        );
        let session = host.acquire(&context)?;
        let (finished, result) = mpsc::channel();
        let actor = thread::spawn(move || {
            let outcome = session.act(
                &context,
                &BrowserAction::Type {
                    text: "x".repeat(8 * 1024 * 1024),
                },
            );
            let _ = finished.send(outcome);
        });
        started.recv_timeout(Duration::from_secs(2))?;
        let fill_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut queued: libc::c_int = 0;
            // SAFETY: FIONREAD writes one integer; the borrowed pipe descriptor is live.
            if unsafe { libc::ioctl(peer_reader.as_raw_fd(), libc::FIONREAD, &mut queued) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
            if queued > 0 {
                break;
            }
            assert!(
                Instant::now() < fill_deadline,
                "request must fill the pipe before shutdown"
            );
            thread::yield_now();
        }
        let expected = if shutdown {
            let (stopped, done) = mpsc::channel();
            let stopping_host = host.clone();
            let closer = thread::spawn(move || {
                let _ = stopped.send(stopping_host.shutdown());
            });
            done.recv_timeout(Duration::from_secs(2))??;
            closer.join().map_err(|_| "shutdown thread panicked")?;
            BrowserError::Cancelled
        } else {
            BrowserError::PageTimeout
        };
        assert_eq!(result.recv_timeout(Duration::from_secs(7))?, Err(expected));
        actor.join().map_err(|_| "browser actor panicked")?;
        let facts = facts.lock().unwrap();
        assert!(
            facts.roots.iter().all(|path| !path.exists()),
            "timeout/shutdown must remove the generation"
        );
        assert_eq!(
            facts.stops,
            [
                BrowserChildKind::Xauth,
                BrowserChildKind::Chromium,
                BrowserChildKind::Xvfb
            ]
        );
    }
    Ok(())
}

/// A scripted DevTools peer behind a real `CdpConnection` pipe. Each popup
/// attaches paused while the opener's command still awaits its reply; the
/// next popup attaches while the previous one's `Page.enable` is pending, as
/// a page that opens windows from a blocked renderer produces.
#[cfg(unix)]
type RequestLog = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// A `CdpConnection` wired to a `PopupChainServer` thread, with the logs the
/// server fills in.
/// Error replies the server injects: a resolve error (optionally preceded by
/// a file chooser event in the same batch) and describe errors in order.
#[cfg(unix)]
#[derive(Default)]
struct ServerFaults {
    resolve_error: Option<(bool, &'static str)>,
    describe_errors: Vec<&'static str>,
    box_model_errors: Vec<&'static str>,
}

#[cfg(unix)]
struct PopupChain {
    connection: CdpConnection,
    server: std::thread::JoinHandle<io::Result<()>>,
    requests: RequestLog,
    closed: Arc<Mutex<Vec<String>>>,
    faults: Arc<Mutex<ServerFaults>>,
}

#[cfg(unix)]
struct PopupChainServer {
    stream: std::os::unix::net::UnixStream,
    /// Popups still to open; `usize::MAX` keeps nesting until the client stops.
    remaining: usize,
    opened: usize,
    /// Whether a resume replies to the held requests of the previous popup
    /// and of the opener.
    resume: bool,
    held_release: Option<u64>,
    held_enable: Option<(u64, String)>,
    requests: RequestLog,
    closed: Arc<Mutex<Vec<String>>>,
    faults: Arc<Mutex<ServerFaults>>,
}

#[cfg(unix)]
impl PopupChainServer {
    fn attachment(index: usize) -> Value {
        json!({"method":"Target.attachedToTarget","params":{"sessionId":format!("session-{index}"),
            "waitingForDebugger":true,"targetInfo":{"targetId":format!("popup-{index}"),"type":"page",
            "url":"","browserContextId":"context"}}})
    }

    fn navigation(index: usize) -> Value {
        json!({"method":"Page.frameNavigated","sessionId":format!("session-{index}"),
            "params":{"frame":{"id":format!("frame-{index}"),"loaderId":format!("loader-{index}"),
            "url":format!("https://example.com/{index}")}}})
    }

    fn page(session: Option<&str>) -> (String, String, String) {
        match session.and_then(|session| session.strip_prefix("session-")) {
            Some(index) => (
                format!("frame-{index}"),
                format!("loader-{index}"),
                format!("https://example.com/{index}"),
            ),
            None => (
                "frame-page".to_owned(),
                "loader-page".to_owned(),
                "https://example.com/".to_owned(),
            ),
        }
    }

    fn handle(&mut self, request: &Value) -> Vec<Value> {
        let id = request["id"].as_u64().unwrap_or(0);
        let method = request["method"].as_str().unwrap_or("");
        let session = request["sessionId"].as_str();
        self.requests
            .lock()
            .expect("requests")
            .push((method.to_owned(), session.map(str::to_owned)));
        let reply = |id: u64, result: Value| {
            let mut reply = json!({"id":id,"result":result});
            if let Some(session) = session {
                reply["sessionId"] = json!(session);
            }
            reply
        };
        let (frame, loader, url) = Self::page(session);
        let error = |id: u64, message: &str| {
            let mut reply = json!({"id":id,"error":{"code":-32000,"message":message}});
            if let Some(session) = session {
                reply["sessionId"] = json!(session);
            }
            reply
        };
        let mut faults = self.faults.lock().expect("faults");
        match method {
            "DOM.resolveNode" if faults.resolve_error.is_some() => {
                let (with_chooser, message) = faults.resolve_error.take().unwrap_or((false, ""));
                let mut out = Vec::new();
                if with_chooser {
                    out.push(
                        json!({"method":"Page.fileChooserOpened","sessionId":session,"params":{}}),
                    );
                }
                out.push(error(id, message));
                out
            }
            "DOM.describeNode" if !faults.describe_errors.is_empty() => {
                let message = faults.describe_errors.remove(0);
                vec![error(id, message)]
            }
            "DOM.getBoxModel" if !faults.box_model_errors.is_empty() => {
                let message = faults.box_model_errors.remove(0);
                vec![error(id, message)]
            }
            "Target.getTargets" => {
                vec![reply(
                    id,
                    json!({"targetInfos":[{"targetId":"target","type":"page"}]}),
                )]
            }
            "Target.attachToTarget" => vec![reply(id, json!({"sessionId":"page"}))],
            "Input.dispatchMouseEvent"
                if request["params"]["type"] == "mouseReleased" && self.remaining > 0 =>
            {
                self.remaining -= 1;
                self.opened += 1;
                self.held_release = Some(id);
                vec![Self::attachment(self.opened)]
            }
            "Page.enable"
                if self.remaining > 0 && session == Some(&format!("session-{}", self.opened)) =>
            {
                self.remaining -= 1;
                self.opened += 1;
                self.held_enable = Some((id, session.unwrap_or("").to_owned()));
                vec![Self::attachment(self.opened)]
            }
            "Runtime.runIfWaitingForDebugger" => {
                let index: usize = session
                    .and_then(|session| session.strip_prefix("session-"))
                    .and_then(|index| index.parse().ok())
                    .unwrap_or(0);
                let mut out = vec![reply(id, json!({}))];
                if !self.resume {
                    return out;
                }
                out.push(Self::navigation(index));
                if let Some((held, held_session)) = self.held_enable.take() {
                    let mut enabled = json!({"id":held,"result":{}});
                    enabled["sessionId"] = json!(held_session);
                    out.push(enabled);
                } else if let Some(held) = self.held_release.take() {
                    let mut released = json!({"id":held,"result":{}});
                    released["sessionId"] = json!("page");
                    out.push(released);
                }
                out
            }
            "Target.closeTarget" => {
                self.closed.lock().expect("closed").push(
                    request["params"]["targetId"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                );
                vec![reply(id, json!({}))]
            }
            "Page.getFrameTree" => vec![reply(
                id,
                json!({"frameTree":{"frame":{"id":frame,"loaderId":loader,"url":url}}}),
            )],
            "Runtime.evaluate" => {
                let expression = request["params"]["expression"].as_str().unwrap_or("");
                let value = if expression == "document.readyState" {
                    json!("complete")
                } else if expression.contains("text:document.body") {
                    json!({"text":"popup text","title":"Popup"})
                } else {
                    json!(true)
                };
                vec![reply(id, json!({"result":{"value":value}}))]
            }
            "Accessibility.getFullAXTree" => vec![reply(id, json!({"nodes":[]}))],
            "DOM.getDocument" => vec![reply(
                id,
                json!({"root":{"backendNodeId":1,"documentURL":url,"children":[]}}),
            )],
            "DOM.resolveNode" => vec![reply(id, json!({"object":{"objectId":"1"}}))],
            "Runtime.callFunctionOn" => {
                vec![reply(id, json!({"result":{"subtype":"null","value":null}}))]
            }
            "Page.getLayoutMetrics" => vec![reply(
                id,
                json!({"cssVisualViewport":{"clientWidth":1280,"clientHeight":720,"pageX":0,"pageY":0}}),
            )],
            _ => vec![reply(id, json!({}))],
        }
    }

    fn serve(mut self) -> io::Result<()> {
        let mut buffer = Vec::new();
        let mut byte = [0u8];
        loop {
            match self.stream.read(&mut byte) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if byte[0] != 0 {
                buffer.push(byte[0]);
                continue;
            }
            let request: Value = serde_json::from_slice(&buffer)?;
            buffer.clear();
            let mut out = Vec::new();
            for message in self.handle(&request) {
                out.extend(serde_json::to_vec(&message)?);
                out.push(0);
            }
            self.stream.write_all(&out)?;
        }
    }
}

#[cfg(unix)]
fn popup_chain(remaining: usize, resume: bool) -> Result<PopupChain, Box<dyn Error>> {
    use std::os::unix::net::UnixStream;
    let (client, stream) = UnixStream::pair()?;
    let connection = CdpConnection::new(client.try_clone()?.into(), client.into())?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(Mutex::new(Vec::new()));
    let faults = Arc::new(Mutex::new(ServerFaults::default()));
    let server = PopupChainServer {
        faults: Arc::clone(&faults),
        stream,
        remaining,
        opened: 0,
        resume,
        held_release: None,
        held_enable: None,
        requests: Arc::clone(&requests),
        closed: Arc::clone(&closed),
    };
    let server = std::thread::spawn(move || server.serve());
    Ok(PopupChain {
        connection,
        server,
        requests,
        closed,
        faults,
    })
}

#[test]
#[cfg(unix)]
fn pipe_admits_overlapping_popups_in_arrival_order() -> Result<(), Box<dyn Error>> {
    let PopupChain {
        connection,
        server,
        requests,
        closed,
        ..
    } = popup_chain(2, true)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut document = CdpDocument::initialize(Box::new(connection), deadline)?;
    document.act(&BrowserAction::Click { x: 10.0, y: 10.0 }, deadline)?;
    // Both popups were admitted while the click's reply was pending, the
    // second inside the first; the document switched in creation order and
    // kept the last one. The first popup's later navigation did not touch it.
    assert_eq!(*closed.lock().expect("closed"), ["target", "popup-1"]);
    let look = json!({"action":"look"});
    let prepared = document.prepare(&look)?;
    assert_eq!(prepared.description["target"], "popup-2");
    assert_eq!(prepared.description["url"], "https://example.com/2");
    let read = document
        .execute_prepared(&look, &prepared.digest, false, deadline)?
        .ok_or("popup read")?;
    assert_eq!(read.document.target_id, "popup-2");
    assert_eq!(read.document.url, "https://example.com/2");
    {
        let requests = requests.lock().expect("requests");
        let position = |method: &str, session: &str| {
            requests
                .iter()
                .position(|(name, own)| name == method && own.as_deref() == Some(session))
                .ok_or_else(|| format!("{method} on {session}: {requests:?}"))
        };
        assert!(position("Page.enable", "session-1")? < position("Page.enable", "session-2")?);
        assert!(
            position("Runtime.runIfWaitingForDebugger", "session-2")?
                < position("DOM.enable", "session-1")?
        );
        for session in ["session-1", "session-2"] {
            let enables = requests
                .iter()
                .filter(|(name, own)| name == "Page.enable" && own.as_deref() == Some(session))
                .count();
            assert_eq!(
                enables, 1,
                "an admitted target is configured once: {session}"
            );
        }
    }
    drop(document);
    let _ = server.join();
    Ok(())
}

#[test]
#[cfg(unix)]
fn pipe_admission_nesting_is_bounded_and_fatal() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let PopupChain {
        connection,
        server,
        requests,
        ..
    } = popup_chain(usize::MAX, false)?;
    let host = host_with_devtools(root.path(), Box::new(connection))?;
    let context = BrowserCallContext {
        effective_session_id: "owner".to_owned(),
        permission_mode_snapshot: runtime::PermissionModeSnapshot::default(),
        permission_interactive: true,
        browser_screenshots: Some(false),
        native_image_input_supported: false,
    };
    let session = host.acquire(&context)?;
    let started = Instant::now();
    let result = session.act(&context, &BrowserAction::Click { x: 10.0, y: 10.0 });
    assert_eq!(result, Err(BrowserError::Protocol));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "failed fast, not by deadline"
    );
    let admitted = requests
        .lock()
        .expect("requests")
        .iter()
        .filter(|(name, _)| name == "Browser.setDownloadBehavior")
        .count();
    // Initialization and the current target each configure once; then at most
    // MAX_ADMISSION_DEPTH nested popups before the transport gives up.
    assert!(
        admitted <= 2 + browser::MAX_ADMISSION_DEPTH,
        "{admitted} admissions"
    );
    let status = host.status()?;
    assert_eq!(status.state, BrowserState::Failed);
    assert!(status
        .reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("browser_protocol")));
    host.shutdown()?;
    let _ = server.join();
    Ok(())
}

#[test]
#[cfg(unix)]
fn pipe_error_replies_apply_preceding_events_and_classify_stale_nodes() -> Result<(), Box<dyn Error>>
{
    // A chooser event arrives in the batch before a stale-node error reply:
    // the chooser is the outcome, and the document is closed.
    let PopupChain {
        connection,
        server,
        faults,
        ..
    } = popup_chain(0, false)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut document = CdpDocument::initialize(Box::new(connection), deadline)?;
    faults.lock().expect("faults").resolve_error = Some((true, "No node with given id found"));
    assert_eq!(
        document.read(false, deadline).err(),
        Some(BrowserError::FileTransferDenied)
    );
    assert_eq!(document.poll(deadline), Err(BrowserError::DocumentChanged));
    drop(document);
    let _ = server.join();
    // Only the recognized stale-node messages are recoverable.
    let PopupChain {
        mut connection,
        server,
        faults,
        ..
    } = popup_chain(0, false)?;
    faults.lock().expect("faults").describe_errors = vec![
        "No node found for given backend id",
        "Could not find object with given id",
        "DOM agent hasn't been enabled",
    ];
    for expected in [
        BrowserError::StaleRevision,
        BrowserError::StaleRevision,
        BrowserError::Protocol,
    ] {
        let result = connection.request(
            "DOM.describeNode",
            json!({"backendNodeId":7}),
            Some("page"),
            deadline,
            &mut no_admission,
        );
        assert_eq!(result.err(), Some(expected));
    }
    // Box model: only the no-box message skips the element; a gone node is
    // stale; anything else is fatal.
    faults.lock().expect("faults").box_model_errors = vec![
        "Could not compute box model.",
        "No node found for given backend id",
        "Could not compute content quads.",
    ];
    for expected in [
        BrowserError::GeometryUnavailable,
        BrowserError::StaleRevision,
        BrowserError::Protocol,
    ] {
        let result = connection.request(
            "DOM.getBoxModel",
            json!({"backendNodeId":7}),
            Some("page"),
            deadline,
            &mut no_admission,
        );
        assert_eq!(result.err(), Some(expected));
    }
    assert!(browser::STALE_NODE_MESSAGES
        .contains(&"Node with given id does not belong to the document"));
    assert_eq!(
        browser::reply_error(
            "DOM.focus",
            &json!({"code":-32602,"message":"Element is not focusable"})
        ),
        BrowserError::Protocol,
        "only server errors (-32000) are stale-node errors"
    );
    drop(connection);
    let _ = server.join();
    Ok(())
}
