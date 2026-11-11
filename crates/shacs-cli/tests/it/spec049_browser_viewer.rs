//! Spec 049 PRD 040: the view-only screen viewer. Fake process runner only.
//! The xauth/Xvfb/Chromium children come from the PRD 010 fakes in
//! `spec049_browser_runtime`; this module fakes x11vnc and websockify.

use super::spec049_browser_runtime::{
    bundle, context, input as gate_input, Clock, Facts, Runner, Startup,
};
use serde_json::{json, Value};
use shacs_cli::browser::{
    self, BrowserChild, BrowserChildKind, BrowserChildSpec, BrowserEnvironment,
    BrowserLifecycleFactory, BrowserOwnerBinding, BrowserProcessRunner, BrowserSnapshotDestination,
    BrowserState, BrowserStatus, BrowserViewPasswordProblem, BrowserViewState,
    BrowserViewerFactory, DevToolsPeer, ManagedBrowserHost, ViewerListener,
};
use shacs_cli::{
    RuntimeOwnerProcessEvidence, RuntimeOwnershipMarker, RuntimeOwnershipState,
    RuntimeOwnershipStatus,
};
use shacs_config::{BrowserConfig, BrowserViewConfig, ConfigBundle, LoadOptions, PermissionMode};
use shacs_core::runtime::{BrowserCallContext, BrowserError, BrowserHost, ProcessGateInput};
use shacs_core::tools::{BrowserTool, Tool, ToolCallExecutionContext, ToolResult};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Every secret in these tests contains `secret-marker`; the parent process
/// markers contain `password-marker` and `secret-marker`. One check covers
/// argv, environment, Debug/log text, tool results, status and diagnostics.
const SECRET: &str = "view-secret-marker-A1b2";
const ENV_FILE_SECRET: &str = "env-file-secret-marker-9z";
const ALLOWLIST: [&str; 10] = [
    "PATH",
    "LANG",
    "DISPLAY",
    "XAUTHORITY",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "TMPDIR",
];

fn assert_no_secret(surface: &str, text: &str) {
    for marker in [SECRET, ENV_FILE_SECRET, "secret-marker", "password-marker"] {
        assert!(
            !text.contains(marker),
            "{surface} carries the secret: {text}"
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum X11vnc {
    /// The real program: reads the file, removes it, listens.
    #[default]
    ReadsAndDeletes,
    /// Exits before reading the file.
    ExitsBeforeReading,
    /// Keeps running without ever listening or reading.
    NeverReady,
}

#[derive(Default)]
struct ViewerFacts {
    x11vnc: X11vnc,
    fail_websockify: bool,
    not_installed: bool,
    /// Another process already listens on 127.0.0.1:5900 before x11vnc starts.
    port_occupied: bool,
    /// The IPv4 listener that appears after x11vnc starts is not x11vnc's
    /// (x11vnc lost the IPv4 bind and stayed alive on ::1 only).
    foreign_after_spawn: bool,
    /// The fake pid of the spawned x11vnc while it listens.
    x11vnc_pid: Option<u32>,
    spawns: Vec<BrowserChildSpec>,
    /// What the fake x11vnc read: content, file mode, parent folder mode.
    read_files: Vec<(String, u32, u32)>,
    stops: Vec<String>,
    x11vnc_dead: Arc<AtomicBool>,
    websockify_dead: Arc<AtomicBool>,
}

struct ViewerChild {
    program: String,
    pid: u32,
    dead: Arc<AtomicBool>,
    facts: Arc<Mutex<ViewerFacts>>,
    stopped: bool,
}

impl BrowserChild for ViewerChild {
    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }
    fn exited(&mut self) -> Result<bool, BrowserError> {
        Ok(self.dead.load(Ordering::SeqCst))
    }
    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        Ok(false)
    }
    fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            let mut facts = self.facts.lock().expect("viewer facts lock poisoned");
            facts.stops.push(self.program.clone());
            // A stopped x11vnc releases its listener.
            if facts.x11vnc_pid == Some(self.pid) {
                facts.x11vnc_pid = None;
            }
        }
    }
    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        Err(BrowserError::Protocol)
    }
}

/// The PRD 010 runner for xauth/Xvfb/Chromium, plus fake viewer programs.
struct ViewerRunner {
    inner: Runner,
    facts: Arc<Mutex<ViewerFacts>>,
}

impl BrowserProcessRunner for ViewerRunner {
    fn available(&self) -> bool {
        true
    }
    fn viewer_available(&self) -> bool {
        !self.facts.lock().expect("viewer facts").not_installed
    }
    fn viewer_listener(&self, port: u16, owner: Option<u32>) -> ViewerListener {
        let facts = self.facts.lock().expect("viewer facts");
        if port != browser::VNC_PORT {
            return ViewerListener::Absent;
        }
        if facts.port_occupied {
            return ViewerListener::Foreign;
        }
        // The listener exists only while a fake x11vnc that read its file is
        // running; a never-ready x11vnc never listens.
        let Some(pid) = facts.x11vnc_pid else {
            return ViewerListener::Absent;
        };
        if facts.foreign_after_spawn || owner != Some(pid) {
            // Ownership is real: the host must ask with the pid it spawned.
            return ViewerListener::Foreign;
        }
        ViewerListener::OwnedByChild
    }
    fn display_ready(&self, display: u16, deadline: Instant) -> Result<(), BrowserError> {
        self.inner.display_ready(display, deadline)
    }
    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        if spec.program != browser::X11VNC && spec.program != browser::WEBSOCKIFY {
            return self.inner.spawn(spec, deadline);
        }
        let mut facts = self
            .facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        facts.spawns.push(spec.clone());
        let pid = 5000 + facts.spawns.len() as u32;
        if spec.program == browser::WEBSOCKIFY {
            if facts.fail_websockify {
                return Err(BrowserError::Unavailable);
            }
            return Ok(Box::new(ViewerChild {
                program: spec.program.clone(),
                pid,
                dead: Arc::clone(&facts.websockify_dead),
                facts: Arc::clone(&self.facts),
                stopped: false,
            }));
        }
        let path = spec
            .arguments
            .iter()
            .position(|argument| argument == "-passwdfile")
            .and_then(|index| spec.arguments.get(index + 1))
            .and_then(|argument| argument.strip_prefix("rm:"))
            .map(PathBuf::from)
            .ok_or(BrowserError::Protocol)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| BrowserError::Protocol)?;
        assert!(metadata.is_file(), "password file must be a regular file");
        let parent = fs::metadata(path.parent().ok_or(BrowserError::Protocol)?)
            .map_err(|_| BrowserError::Protocol)?;
        let content = fs::read_to_string(&path).map_err(|_| BrowserError::Protocol)?;
        facts.read_files.push((
            content,
            metadata.permissions().mode() & 0o777,
            parent.permissions().mode() & 0o777,
        ));
        match facts.x11vnc {
            X11vnc::ReadsAndDeletes => {
                fs::remove_file(&path).map_err(|_| BrowserError::Protocol)?;
                // Listening from now on (as x11vnc, or as the stranger that
                // took IPv4 5900 when `foreign_after_spawn`).
                facts.x11vnc_pid = Some(pid);
            }
            X11vnc::ExitsBeforeReading => facts.x11vnc_dead.store(true, Ordering::SeqCst),
            X11vnc::NeverReady => {}
        }
        Ok(Box::new(ViewerChild {
            program: spec.program.clone(),
            pid,
            dead: Arc::clone(&facts.x11vnc_dead),
            facts: Arc::clone(&self.facts),
            stopped: false,
        }))
    }
}

fn view(enable: bool, password: &str) -> BrowserViewConfig {
    BrowserViewConfig {
        enable,
        password: password.to_owned(),
    }
}

struct Harness {
    host: ManagedBrowserHost,
    facts: Arc<Mutex<Facts>>,
    viewer: Arc<Mutex<ViewerFacts>>,
    xvfb_dead: Arc<AtomicBool>,
    context: BrowserCallContext,
}

fn start_harness(
    input: &ProcessGateInput,
    view: BrowserViewConfig,
    viewer: ViewerFacts,
    host_port: u16,
    snapshot: Option<BrowserSnapshotDestination>,
) -> Harness {
    start_harness_with_config(
        input,
        BrowserConfig {
            enable: true,
            ..BrowserConfig::default()
        },
        view,
        viewer,
        host_port,
        snapshot,
    )
}

fn start_harness_with_config(
    input: &ProcessGateInput,
    config: BrowserConfig,
    view: BrowserViewConfig,
    viewer: ViewerFacts,
    host_port: u16,
    snapshot: Option<BrowserSnapshotDestination>,
) -> Harness {
    let facts = Arc::new(Mutex::new(Facts::default()));
    let viewer = Arc::new(Mutex::new(viewer));
    let xvfb_dead = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(Clock::default());
    let host = ManagedBrowserHost::new(
        Box::new(Startup {
            config,
            input: input.clone(),
        }),
        Box::new(ViewerRunner {
            inner: Runner {
                facts: Arc::clone(&facts),
                chromium_dead: Arc::new(AtomicBool::new(false)),
                xvfb_dead: Arc::clone(&xvfb_dead),
                clock: Arc::clone(&clock),
                advance_start: 0,
                advance_read: 0,
            },
            facts: Arc::clone(&viewer),
        }),
        clock,
        snapshot,
        Some(Box::new(BrowserViewerFactory::new(view, host_port))),
    );
    Harness {
        host,
        facts,
        viewer,
        xvfb_dead,
        context: context(input, "owner"),
    }
}

/// A user-local config with the given `tools.browser.view`, reloaded through
/// the normal loader (which resolves `${NAME}` from the process environment,
/// then `.env.json`).
fn view_bundle(
    root: &Path,
    view: Value,
    env_file: Option<&str>,
) -> Result<ConfigBundle, Box<dyn Error>> {
    let written = bundle(root, PermissionMode::Default)?;
    let mut value: Value = serde_json::from_slice(&fs::read(&written.context.config_path)?)?;
    value["tools"]["browser"]["view"] = view;
    fs::write(&written.context.config_path, serde_json::to_vec(&value)?)?;
    if let Some(env_file) = env_file {
        fs::write(root.join(".env.json"), env_file)?;
    }
    Ok(shacs_config::load_config(LoadOptions {
        config_path: Some(written.context.config_path.clone()),
        ..LoadOptions::default()
    })?)
}

fn password_files(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("view-"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn viewer_programs(facts: &ViewerFacts) -> Vec<&str> {
    facts
        .spawns
        .iter()
        .map(|spec| spec.program.as_str())
        .collect()
}

fn assert_generation_environment(viewer: &ViewerFacts, facts: &Facts) {
    let chromium = facts
        .spawns
        .iter()
        .find(|spec| spec.kind == BrowserChildKind::Chromium)
        .expect("chromium spawn");
    for spec in &viewer.spawns {
        assert_eq!(spec.kind, BrowserChildKind::Viewer);
        assert!(spec
            .environment
            .keys()
            .all(|key| ALLOWLIST.contains(&key.as_str())));
        // The same generation environment as xauth/Xvfb/Chromium, built by the
        // same `BrowserEnvironment::child`: not a viewer-specific inheritance.
        assert_eq!(spec.environment, chromium.environment);
        assert!(browser::child_environment_is_confined(spec));
        for key in [
            "OPENAI_API_KEY",
            "SHACS_BROWSER_VIEW_PASSWORD",
            "SHACS_SPEC049_VIEW_PASSWORD",
        ] {
            assert!(!spec.environment.contains_key(key));
        }
        assert_no_secret("viewer argv", &spec.arguments.join(" "));
        assert_no_secret("viewer child spec", &format!("{spec:?}"));
    }
}

/// Reruns the calling test in a child whose environment carries the provider
/// credential and viewer password source markers of PRD 010 acceptance 1.
fn with_parent_secret_markers(test: &str) -> Result<bool, Box<dyn Error>> {
    if std::env::var("SHACS_BROWSER_TEST_SECRETS").as_deref() == Ok("present") {
        return Ok(false);
    }
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            &format!("spec049_browser_viewer::{test}"),
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
    Ok(true)
}

#[test]
fn viewer_password_gate_does_not_block_browser() -> Result<(), Box<dyn Error>> {
    if with_parent_secret_markers("viewer_password_gate_does_not_block_browser")? {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), PermissionMode::Default)?;
    let input = gate_input(&bundle)?;
    let format_invalid = [
        "with\nnewline1",
        "with\rreturn1",
        "nul\0byte12345",
        " leadingspace",
        "trailingspace ",
        "#commentlike1",
        "__SKIP__abcdefgh",
        "abcd__SKIP__efgh",
        "abcdefgh__SKIP__",
        "__COMM__abcdefgh",
        "abcd__COMM__efgh",
        "abcdefgh__COMM__",
        "__EMPTY__abcdef",
        "__BEGIN_VIEWONLY__",
        // Printable ASCII only: noVNC keys DES from UTF-16 code units while
        // the file is UTF-8, so these could never authenticate.
        "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}",
        "abcd\u{a0}efgh",
        "\u{d55c}\u{ae00}\u{d55c}\u{ae00}\u{d55c}\u{ae00}\u{d55c}\u{ae00}",
        "abcd efgh",
        "abcd\tefgh",
        "abcd\u{7f}efgh",
    ];
    // Longer than one x11vnc password-file record would be read as two
    // passwords, both accepted; 128 is the ceiling and 129 is too long.
    let too_long = ["A".repeat(129), format!("{}B", "A".repeat(1023))];
    for password in &too_long {
        assert_eq!(
            browser::validate_view_password(password),
            Err(BrowserViewPasswordProblem::TooLong)
        );
    }
    assert_eq!(browser::validate_view_password(&"A".repeat(128)), Ok(()));
    let printable: String = (0x21u8..=0x7e).map(char::from).collect();
    assert_eq!(
        browser::validate_view_password(&printable),
        Ok(()),
        "every printable ASCII character is accepted, # included when not first"
    );
    assert_eq!(
        browser::validate_view_password(&format!("#{}", &printable[1..])),
        Err(BrowserViewPasswordProblem::FormatInvalid),
        "a leading # is a comment line"
    );
    let mut cases: Vec<(BrowserViewConfig, bool, BrowserViewState, &str)> = vec![
        (
            view(false, ""),
            true,
            BrowserViewState::Disabled,
            "disabled",
        ),
        (
            view(false, SECRET),
            true,
            BrowserViewState::Disabled,
            "disabled",
        ),
        (
            view(true, ""),
            true,
            BrowserViewState::Unavailable,
            "password_missing",
        ),
        (
            view(true, "short7!"),
            true,
            BrowserViewState::Unavailable,
            "password_too_short",
        ),
        (
            view(true, "valid-but-not-installed-secret-marker"),
            false,
            BrowserViewState::Unavailable,
            "package_missing",
        ),
    ];
    for password in format_invalid {
        assert!(password.chars().count() >= 8, "{password:?}");
        assert_eq!(
            browser::validate_view_password(password),
            Err(BrowserViewPasswordProblem::FormatInvalid),
            "{password:?}"
        );
        cases.push((
            view(true, password),
            true,
            BrowserViewState::Unavailable,
            "password_format_invalid",
        ));
    }
    for password in &too_long {
        cases.push((
            view(true, password),
            true,
            BrowserViewState::Unavailable,
            "password_too_long",
        ));
    }
    assert_eq!(
        browser::validate_view_password(""),
        Err(BrowserViewPasswordProblem::Missing)
    );
    assert_eq!(
        browser::validate_view_password("1234567"),
        Err(BrowserViewPasswordProblem::TooShort)
    );
    assert_eq!(browser::validate_view_password("12345678"), Ok(()));
    for (view, installed, state, reason) in cases {
        let harness = start_harness(
            &input,
            view,
            ViewerFacts {
                not_installed: !installed,
                ..ViewerFacts::default()
            },
            browser::GATEWAY_VIEWER_HOST_PORT,
            None,
        );
        let before = harness.host.status()?.view;
        assert_eq!(
            (before.state, before.reason.as_deref()),
            (state, Some(reason))
        );
        let session = harness.host.acquire(&harness.context)?;
        assert_eq!(
            session.read(&harness.context)?.visible_text,
            "PRIVATE PAGE TEXT"
        );
        let status = harness.host.status()?;
        assert_eq!(status.state, BrowserState::Running);
        assert_eq!(status.reason, None, "viewer gate is not a browser failure");
        assert_eq!(status.view.state, state, "{reason}");
        assert_eq!(status.view.reason.as_deref(), Some(reason));
        assert_eq!(status.view.url, None);
        {
            let viewer = harness.viewer.lock().expect("viewer facts");
            assert!(viewer.spawns.is_empty(), "{reason}: viewer must not spawn");
            assert!(viewer.read_files.is_empty());
            let facts = harness.facts.lock().expect("facts");
            assert_eq!(facts.spawns.len(), 3, "xauth, Xvfb, Chromium only");
            for root in &facts.roots {
                assert!(
                    password_files(root).is_empty(),
                    "{reason}: no password file may exist"
                );
            }
        }
        assert_no_secret("status", &format!("{status:?}"));
        assert_no_secret("status json", &serde_json::to_string(&status)?);
        session.close(&harness.context)?;
    }

    // The recommended reference with no value anywhere is not a load error:
    // the browser starts, the viewer is `password_missing`, and no variable
    // name is reported.
    for enable in [true, false] {
        let missing_root = tempfile::tempdir()?;
        let missing = view_bundle(
            missing_root.path(),
            json!({"enable":enable,"password":"${SHACS_SPEC049_ABSENT_VIEW_PASSWORD}"}),
            None,
        )?;
        assert_eq!(missing.config.tools.browser.view.password, "");
        let missing_input = gate_input(&missing)?;
        let harness = start_harness(
            &missing_input,
            missing.config.tools.browser.view.clone(),
            ViewerFacts::default(),
            browser::GATEWAY_VIEWER_HOST_PORT,
            None,
        );
        let session = harness.host.acquire(&harness.context)?;
        assert_eq!(
            session.read(&harness.context)?.visible_text,
            "PRIVATE PAGE TEXT"
        );
        let status = harness.host.status()?;
        assert_eq!(status.state, BrowserState::Running);
        let (state, reason) = if enable {
            (BrowserViewState::Unavailable, "password_missing")
        } else {
            (BrowserViewState::Disabled, "disabled")
        };
        assert_eq!(status.view.state, state);
        assert_eq!(status.view.reason.as_deref(), Some(reason));
        assert!(harness
            .viewer
            .lock()
            .expect("viewer facts")
            .spawns
            .is_empty());
        let text = format!("{status:?}{}", serde_json::to_string(&status)?);
        assert!(!text.contains("SHACS_SPEC049_ABSENT_VIEW_PASSWORD"));
        let report = shacs_cli::runtime_inspect(shacs_cli::RuntimeInspectOptions {
            config_path: Some(missing.context.config_path.clone()),
            workspace_override: Some(missing.context.workspace.clone()),
        })?;
        assert_eq!(
            report.browser.view.reason.as_deref(),
            Some(reason),
            "separate inspect agrees on the reason code"
        );
        assert!(!format!("{:?}", report.browser).contains("SHACS_SPEC049_ABSENT_VIEW_PASSWORD"));
        session.close(&harness.context)?;
    }

    // `${NAME}` resolves through the loader from `.env.json`; the resolved
    // value reaches only the password file, never a child environment.
    let env_root = tempfile::tempdir()?;
    let resolved = view_bundle(
        env_root.path(),
        json!({"enable":true,"password":"${SHACS_SPEC049_VIEW_PASSWORD}"}),
        Some(&format!(
            r#"{{"SHACS_SPEC049_VIEW_PASSWORD":"{ENV_FILE_SECRET}"}}"#
        )),
    )?;
    assert_eq!(resolved.config.tools.browser.view.password, ENV_FILE_SECRET);
    assert!(std::env::var_os("SHACS_SPEC049_VIEW_PASSWORD").is_none());
    assert_eq!(
        std::env::var("SHACS_BROWSER_VIEW_PASSWORD")?,
        "viewer-password-marker"
    );
    let resolved_input = gate_input(&resolved)?;
    let harness = start_harness(
        &resolved_input,
        resolved.config.tools.browser.view.clone(),
        ViewerFacts::default(),
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let session = harness.host.acquire(&harness.context)?;
    let status = harness.host.status()?;
    assert_eq!(status.view.state, BrowserViewState::Open);
    assert_eq!(
        status.view.url.as_deref(),
        Some("http://127.0.0.1:6080/vnc.html")
    );
    {
        let viewer = harness.viewer.lock().expect("viewer facts");
        let facts = harness.facts.lock().expect("facts");
        assert_eq!(
            viewer_programs(&viewer),
            [browser::X11VNC, browser::WEBSOCKIFY]
        );
        assert_eq!(
            viewer.read_files,
            [(format!("{ENV_FILE_SECRET}\n"), 0o600, 0o700)]
        );
        assert_generation_environment(&viewer, &facts);
        for spec in &facts.spawns {
            assert_no_secret("generation child spec", &format!("{spec:?}"));
        }
    }
    assert_no_secret("status", &format!("{status:?}"));
    session.close(&harness.context)?;
    Ok(())
}

#[test]
fn viewer_arguments_and_password_file_cleanup() -> Result<(), Box<dyn Error>> {
    if with_parent_secret_markers("viewer_arguments_and_password_file_cleanup")? {
        return Ok(());
    }
    let now = Instant::now();
    assert_eq!(browser::VIEWER_START_LIMIT, Duration::from_secs(10));
    let runner = |facts: ViewerFacts| ViewerRunner {
        inner: Runner {
            facts: Arc::new(Mutex::new(Facts::default())),
            chromium_dead: Arc::new(AtomicBool::new(false)),
            xvfb_dead: Arc::new(AtomicBool::new(false)),
            clock: Arc::new(Clock::default()),
            advance_start: 0,
            advance_read: 0,
        },
        facts: Arc::new(Mutex::new(facts)),
    };
    let factory = BrowserViewerFactory::new(view(true, SECRET), browser::API_VIEWER_HOST_PORT);

    // Success: the fake x11vnc reads and removes the file; success is reported
    // only once the listener is up and the file is gone.
    let environment = BrowserEnvironment::create()?;
    let runner_ok = runner(ViewerFacts::default());
    let mut participant = factory.create();
    participant.start(&environment, &runner_ok, now + Duration::from_secs(5))?;
    let open = participant.view();
    assert_eq!(open.state, BrowserViewState::Open);
    assert_eq!(open.reason, None);
    assert_eq!(open.url.as_deref(), Some("http://127.0.0.1:6081/vnc.html"));
    {
        let viewer = runner_ok.facts.lock().expect("viewer facts");
        assert_eq!(
            viewer_programs(&viewer),
            [browser::X11VNC, browser::WEBSOCKIFY]
        );
        let x11vnc = &viewer.spawns[0];
        let password_file = x11vnc.arguments[17]
            .strip_prefix("rm:")
            .ok_or("passwdfile argument")?;
        assert!(
            Path::new(password_file).starts_with(environment.root())
                && Path::new(password_file)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.len() == "view-".len() + 32),
            "{password_file}"
        );
        let xauthority = environment.root().join("Xauthority");
        let expected = [
            "-norc",
            "-noremote",
            "-display",
            &format!(":{}", environment.display()),
            "-auth",
            &xauthority.to_string_lossy(),
            "-rfbport",
            "5900",
            "-listen",
            "127.0.0.1",
            "-localhost",
            "-viewonly",
            "-nosel",
            "-noclipboard",
            "-forever",
            "-shared",
            "-passwdfile",
            &format!("rm:{password_file}"),
        ];
        assert_eq!(x11vnc.arguments, expected);
        assert!(
            !x11vnc.arguments.iter().any(|argument| argument == "-passwd"
                || argument == "-env"
                || argument == "-rfbauth"
                || argument == "-http"
                || argument == "-ac")
        );
        assert_eq!(
            viewer.spawns[1].arguments,
            ["--web=/usr/share/novnc", "0.0.0.0:6080", "127.0.0.1:5900"]
        );
        assert_eq!(viewer.read_files, [(format!("{SECRET}\n"), 0o600, 0o700)]);
        assert!(!Path::new(password_file).exists());
        assert!(password_files(environment.root()).is_empty());
        for spec in &viewer.spawns {
            assert!(spec
                .environment
                .keys()
                .all(|key| ALLOWLIST.contains(&key.as_str())));
            assert!(browser::child_environment_is_confined(spec));
            assert_no_secret("viewer argv", &spec.arguments.join(" "));
            assert_no_secret("viewer environment", &format!("{:?}", spec.environment));
        }
    }
    assert!(!participant.failed());
    participant.stop();
    assert_eq!(participant.view().state, BrowserViewState::Closed);
    assert_eq!(participant.view().url, None);
    assert_eq!(
        runner_ok.facts.lock().expect("viewer facts").stops,
        [browser::WEBSOCKIFY, browser::X11VNC],
        "stop order: websockify, then x11vnc"
    );
    drop(environment);

    // Host-side cleanup: the port is already taken (nothing is spawned and
    // no file is written), the IPv4 listener turns out not to be x11vnc's,
    // x11vnc dies before reading, never becomes ready (the viewer's own
    // deadline), or websockify fails.
    for (facts, deadline, error, reason) in [
        (
            ViewerFacts {
                port_occupied: true,
                ..ViewerFacts::default()
            },
            Duration::from_secs(5),
            BrowserError::Unavailable,
            "port_in_use",
        ),
        (
            ViewerFacts {
                foreign_after_spawn: true,
                ..ViewerFacts::default()
            },
            Duration::from_secs(5),
            BrowserError::Unavailable,
            "port_in_use",
        ),
        (
            ViewerFacts {
                x11vnc: X11vnc::ExitsBeforeReading,
                ..ViewerFacts::default()
            },
            Duration::from_secs(5),
            BrowserError::ProcessExited,
            "process_exited",
        ),
        (
            ViewerFacts {
                x11vnc: X11vnc::NeverReady,
                ..ViewerFacts::default()
            },
            Duration::from_millis(150),
            BrowserError::StartupTimeout,
            "startup_timeout",
        ),
        (
            ViewerFacts {
                fail_websockify: true,
                ..ViewerFacts::default()
            },
            Duration::from_secs(5),
            BrowserError::Unavailable,
            "startup_failed",
        ),
    ] {
        let environment = BrowserEnvironment::create()?;
        let runner = runner(facts);
        let mut participant = factory.create();
        let started = Instant::now();
        assert_eq!(
            participant.start(&environment, &runner, started + deadline),
            Err(error)
        );
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{reason}: cleanup must not wait for the fixed 10 s limit"
        );
        let failed = participant.view();
        assert_eq!(failed.state, BrowserViewState::Failed, "{reason}");
        assert_eq!(failed.reason.as_deref(), Some(reason));
        assert_eq!(failed.url, None);
        let viewer = runner.facts.lock().expect("viewer facts");
        assert!(
            password_files(environment.root()).is_empty(),
            "{reason}: host must delete the password file"
        );
        let occupied_before_spawn = reason == "port_in_use" && !viewer.foreign_after_spawn;
        if occupied_before_spawn {
            assert!(viewer.spawns.is_empty(), "an occupied port spawns nothing");
            assert!(viewer.read_files.is_empty(), "no password file was written");
        } else {
            assert!(
                viewer.stops.contains(&browser::X11VNC.to_owned()),
                "{reason}: started viewer children are stopped"
            );
        }
        assert!(
            !viewer_programs(&viewer).contains(&browser::WEBSOCKIFY) || reason == "startup_failed",
            "{reason}: websockify must not start before x11vnc owns the listener"
        );
        if reason == "startup_failed" {
            assert_eq!(
                viewer_programs(&viewer),
                [browser::X11VNC, browser::WEBSOCKIFY]
            );
        } else if !occupied_before_spawn {
            assert_eq!(viewer_programs(&viewer), [browser::X11VNC]);
        }
        drop(viewer);
        // A second stop is harmless and does not resurrect anything.
        participant.stop();
        assert_eq!(participant.view().state, BrowserViewState::Failed);
    }

    // Through the manager: the same secret marker is absent from argv,
    // environment, Debug text, the tool result and diagnostics.
    let root = tempfile::tempdir()?;
    let bundle = view_bundle(
        root.path(),
        json!({"enable":true,"password":"${SHACS_SPEC049_VIEW_PASSWORD}"}),
        Some(&format!(r#"{{"SHACS_SPEC049_VIEW_PASSWORD":"{SECRET}"}}"#)),
    )?;
    let input = gate_input(&bundle)?;
    let harness = start_harness(
        &input,
        bundle.config.tools.browser.view.clone(),
        ViewerFacts::default(),
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let tool = BrowserTool::new(Arc::new(harness.host.clone()));
    let arguments = json!({"action":"open","url":"https://example.com/"});
    let prepared = harness.host.prepare("owner", &arguments)?;
    let mut execution =
        ToolCallExecutionContext::default().with_browser_context(Some(harness.context.clone()));
    execution.browser_binding_digest = Some(prepared.digest);
    let result = tool.execute_with_context(
        arguments.as_object().ok_or("arguments")?.clone(),
        &execution,
    );
    let ToolResult::Json(result) = result else {
        return Err("browser tool result".into());
    };
    let result_text = result.to_string();
    assert!(
        result_text.contains("PRIVATE PAGE TEXT"),
        "tool executed: {result_text}"
    );
    assert_no_secret("tool result (model input)", &result_text);
    let status = harness.host.status()?;
    assert_eq!(status.view.state, BrowserViewState::Open);
    assert_no_secret("status debug", &format!("{status:?}"));
    assert_no_secret("status json", &serde_json::to_string(&status)?);
    {
        let viewer = harness.viewer.lock().expect("viewer facts");
        let facts = harness.facts.lock().expect("facts");
        assert_eq!(viewer.read_files, [(format!("{SECRET}\n"), 0o600, 0o700)]);
        assert_generation_environment(&viewer, &facts);
        assert_no_secret(
            "all child specs",
            &format!("{:?}{:?}", facts.spawns, viewer.spawns),
        );
    }
    let diagnostics = shacs_cli::runtime_diagnostics(shacs_cli::RuntimeDiagnosticsOptions {
        config_path: Some(bundle.context.config_path.clone()),
        workspace_override: Some(bundle.context.workspace.clone()),
        bundle_path: None,
    })?;
    let diagnostics_text = serde_json::to_string(&diagnostics.snapshot.runtime)?;
    assert!(diagnostics_text.contains("\"view\""));
    assert_no_secret("diagnostics", &diagnostics_text);
    assert_no_secret(
        "diagnostics report",
        &format!("{:?}", diagnostics.snapshot.runtime["browser"]),
    );
    harness.host.shutdown()?;
    Ok(())
}

fn marker(bundle: &ConfigBundle, owner_id: &str) -> RuntimeOwnershipMarker {
    RuntimeOwnershipMarker {
        schema_version: 1,
        owner_id: owner_id.to_owned(),
        pid: std::process::id(),
        started_at_ms: 0,
        updated_at_ms: 100,
        expires_at_ms: 1000,
        lifecycle: "running".to_owned(),
        binary_version: "test".to_owned(),
        data_schema_version: 1,
        mode: "serve".to_owned(),
        config_path: bundle.context.config_path.to_string_lossy().to_string(),
        workspace: bundle.context.workspace.to_string_lossy().to_string(),
        lock_protocol: None,
        process_evidence: RuntimeOwnerProcessEvidence {
            pid: std::process::id(),
            pid_alive: true,
            process_started_after_marker: false,
        },
    }
}

#[test]
fn viewer_lifecycle_and_owner_inspect() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bundle = view_bundle(root.path(), json!({"enable":true,"password":SECRET}), None)?;
    let input = gate_input(&bundle)?;
    let view = bundle.config.tools.browser.view.clone();

    // Browser close ends websockify, then x11vnc, before Chromium and Xvfb.
    let harness = start_harness(
        &input,
        view.clone(),
        ViewerFacts::default(),
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let session = harness.host.acquire(&harness.context)?;
    assert_eq!(harness.host.status()?.view.state, BrowserViewState::Open);
    session.close(&harness.context)?;
    {
        let viewer = harness.viewer.lock().expect("viewer facts");
        let facts = harness.facts.lock().expect("facts");
        assert_eq!(viewer.stops, [browser::WEBSOCKIFY, browser::X11VNC]);
        assert_eq!(
            &facts.stops[1..],
            [BrowserChildKind::Chromium, BrowserChildKind::Xvfb]
        );
        assert!(facts.roots.iter().all(|root| !root.exists()));
    }
    let closed = harness.host.status()?;
    assert_eq!(closed.state, BrowserState::Idle);
    assert_eq!(closed.view.state, BrowserViewState::Closed);
    assert_eq!(closed.view.url, None);

    // An occupied VNC port is a viewer-only failure through the manager: no
    // viewer child, no websockify, the browser keeps answering.
    let harness = start_harness(
        &input,
        view.clone(),
        ViewerFacts {
            port_occupied: true,
            ..ViewerFacts::default()
        },
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let session = harness.host.acquire(&harness.context)?;
    let occupied = harness.host.status()?;
    assert_eq!(occupied.state, BrowserState::Running);
    assert_eq!(occupied.view.state, BrowserViewState::Failed);
    assert_eq!(occupied.view.reason.as_deref(), Some("port_in_use"));
    assert_eq!(occupied.view.url, None);
    assert!(harness
        .viewer
        .lock()
        .expect("viewer facts")
        .spawns
        .is_empty());
    assert_eq!(
        session.read(&harness.context)?.visible_text,
        "PRIVATE PAGE TEXT"
    );
    assert!(!harness
        .facts
        .lock()
        .expect("facts")
        .stops
        .contains(&BrowserChildKind::Chromium));
    session.close(&harness.context)?;

    // A viewer that never becomes ready spends its own 10 s budget, not the
    // browser's: with the minimum browser startup limit the browser is kept.
    let harness = start_harness_with_config(
        &input,
        BrowserConfig {
            enable: true,
            startup_timeout_seconds: 5,
            ..BrowserConfig::default()
        },
        view.clone(),
        ViewerFacts {
            x11vnc: X11vnc::NeverReady,
            ..ViewerFacts::default()
        },
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let started = Instant::now();
    let session = harness.host.acquire(&harness.context)?;
    assert!(
        started.elapsed() >= browser::VIEWER_START_LIMIT,
        "the viewer used its own budget"
    );
    let slow = harness.host.status()?;
    assert_eq!(slow.state, BrowserState::Running);
    assert_eq!(slow.view.state, BrowserViewState::Failed);
    assert_eq!(slow.view.reason.as_deref(), Some("startup_timeout"));
    assert_eq!(
        session.read(&harness.context)?.visible_text,
        "PRIVATE PAGE TEXT"
    );
    {
        let viewer = harness.viewer.lock().expect("viewer facts");
        assert_eq!(viewer_programs(&viewer), [browser::X11VNC]);
        assert_eq!(viewer.stops, [browser::X11VNC]);
        let facts = harness.facts.lock().expect("facts");
        assert!(!facts.stops.contains(&BrowserChildKind::Chromium));
        assert!(!facts.stops.contains(&BrowserChildKind::Xvfb));
        for root in &facts.roots {
            assert!(password_files(root).is_empty(), "host deleted the file");
        }
    }
    session.close(&harness.context)?;

    // Xvfb death ends the viewer pair with the generation.
    let harness = start_harness(
        &input,
        view.clone(),
        ViewerFacts::default(),
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let _session = harness.host.acquire(&harness.context)?;
    harness.xvfb_dead.store(true, Ordering::SeqCst);
    harness.host.maintenance()?;
    assert_eq!(harness.host.status()?.state, BrowserState::Failed);
    assert_eq!(
        harness.viewer.lock().expect("viewer facts").stops,
        [browser::WEBSOCKIFY, browser::X11VNC]
    );

    // Viewer-only death: the pair closes, `failed` is recorded, the browser
    // keeps answering, nothing restarts in this generation, and the next
    // generation tries again.
    let harness = start_harness(
        &input,
        view.clone(),
        ViewerFacts::default(),
        browser::GATEWAY_VIEWER_HOST_PORT,
        None,
    );
    let session = harness.host.acquire(&harness.context)?;
    harness
        .viewer
        .lock()
        .expect("viewer facts")
        .x11vnc_dead
        .store(true, Ordering::SeqCst);
    harness.host.maintenance()?;
    let status = harness.host.status()?;
    assert_eq!(status.state, BrowserState::Running);
    assert_eq!(status.reason.as_deref(), Some("viewer_process_exited"));
    assert_eq!(status.view.state, BrowserViewState::Failed);
    assert_eq!(status.view.reason.as_deref(), Some("process_exited"));
    assert_eq!(status.view.url, None);
    assert_eq!(
        harness.viewer.lock().expect("viewer facts").stops,
        [browser::WEBSOCKIFY, browser::X11VNC]
    );
    assert_eq!(
        session.read(&harness.context)?.visible_text,
        "PRIVATE PAGE TEXT"
    );
    harness.host.maintenance()?;
    let after = harness.host.status()?;
    assert_eq!(after.state, BrowserState::Running);
    assert_eq!(after.view.state, BrowserViewState::Failed);
    assert_eq!(
        harness.viewer.lock().expect("viewer facts").spawns.len(),
        2,
        "no automatic viewer restart within a generation"
    );
    assert!(!harness
        .facts
        .lock()
        .expect("facts")
        .stops
        .contains(&BrowserChildKind::Chromium));
    session.close(&harness.context)?;
    harness
        .viewer
        .lock()
        .expect("viewer facts")
        .x11vnc_dead
        .store(false, Ordering::SeqCst);
    let session = harness.host.acquire(&harness.context)?;
    let restarted = harness.host.status()?;
    assert_eq!(restarted.view.state, BrowserViewState::Open);
    assert_eq!(
        viewer_programs(&harness.viewer.lock().expect("viewer facts")),
        [
            browser::X11VNC,
            browser::WEBSOCKIFY,
            browser::X11VNC,
            browser::WEBSOCKIFY
        ]
    );
    session.close(&harness.context)?;

    // Owner-bound snapshot: the live `view` is visible only to the owner the
    // snapshot was written for; a changed owner or an ended lease is unknown.
    let marker = marker(&bundle, "owner-a");
    let binding = BrowserOwnerBinding::from_marker(&marker)?;
    let harness = start_harness(
        &input,
        view.clone(),
        ViewerFacts::default(),
        browser::API_VIEWER_HOST_PORT,
        Some(BrowserSnapshotDestination {
            data_dir: bundle.context.data_dir.clone(),
            owner: Some(binding),
            config_path: bundle.context.config_path.clone(),
            workspace: bundle.context.workspace.clone(),
        }),
    );
    let session = harness.host.acquire(&harness.context)?;
    let written: Value = serde_json::from_slice(&fs::read(
        bundle.context.data_dir.join("runtime/browser-state.json"),
    )?)?;
    assert_eq!(written["status"]["view"]["state"], "open");
    assert_eq!(
        written["status"]["view"]["url"],
        "http://127.0.0.1:6081/vnc.html"
    );
    assert_no_secret("state file", &written.to_string());
    let read = |ownership: &RuntimeOwnershipStatus, now| {
        browser::read_browser_snapshot(
            &bundle.context.data_dir,
            ownership,
            &bundle.context.config_path,
            &bundle.context.workspace,
            BrowserStatus::new(true, true, now),
            now,
        )
    };
    let ownership = RuntimeOwnershipStatus {
        state: RuntimeOwnershipState::Active,
        marker: Some(marker.clone()),
        reason: "fixture".to_owned(),
    };
    let observed = read(&ownership, 300);
    assert!(observed.owner_observed);
    assert_eq!(observed.state, BrowserState::Busy);
    assert_eq!(observed.view.state, BrowserViewState::Open);
    assert_eq!(
        observed.view.url.as_deref(),
        Some("http://127.0.0.1:6081/vnc.html")
    );
    let mut previous = ownership.clone();
    previous.marker.as_mut().ok_or("marker")?.owner_id = "owner-b".to_owned();
    for (status, label) in [
        (read(&previous, 300), "previous owner"),
        (read(&ownership, 1001), "ended lease"),
    ] {
        assert_eq!(status.state, BrowserState::Unknown, "{label}");
        assert_eq!(status.view.state, BrowserViewState::Unknown, "{label}");
        assert_eq!(status.view.url, None, "{label}");
    }
    session.close(&harness.context)?;

    // Separate inspect: configuration facts (enabled, reason) are reported,
    // the live state is unknown without the owner, and JSON, text and
    // diagnostics agree. The password never appears.
    let installed = browser::LinuxBrowserProcessRunner.viewer_available();
    for (view, state, reason) in [
        (
            json!({"enable":true,"password":"${SHACS_SPEC049_VIEW_PASSWORD}"}),
            "unknown",
            if installed {
                None
            } else {
                Some("package_missing")
            },
        ),
        (
            json!({"enable":true,"password":"short"}),
            "unknown",
            Some("password_too_short"),
        ),
        (
            json!({"enable":false,"password":""}),
            "unknown",
            Some("disabled"),
        ),
    ] {
        let root = tempfile::tempdir()?;
        let bundle = view_bundle(
            root.path(),
            view,
            Some(&format!(r#"{{"SHACS_SPEC049_VIEW_PASSWORD":"{SECRET}"}}"#)),
        )?;
        let report = shacs_cli::runtime_inspect(shacs_cli::RuntimeInspectOptions {
            config_path: Some(bundle.context.config_path.clone()),
            workspace_override: Some(bundle.context.workspace.clone()),
        })?;
        let value = serde_json::to_value(&report.browser)?;
        assert_eq!(value["state"], "unknown");
        assert_eq!(value["view"]["state"], state);
        assert_eq!(value["view"]["url"], Value::Null);
        assert_eq!(
            value["view"]["reason"],
            reason.map_or(Value::Null, Value::from)
        );
        assert_eq!(
            value["view"]["enabled"],
            bundle.config.tools.browser.view.enable
        );
        for key in ["enabled", "state", "reason", "url"] {
            assert!(value["view"].get(key).is_some(), "{key}");
        }
        assert_no_secret("inspect json", &value.to_string());
        let diagnostics = shacs_cli::runtime_diagnostics(shacs_cli::RuntimeDiagnosticsOptions {
            config_path: Some(bundle.context.config_path.clone()),
            workspace_override: Some(bundle.context.workspace.clone()),
            bundle_path: None,
        })?;
        assert_eq!(
            diagnostics.snapshot.runtime["browser"]["view"],
            value["view"]
        );
        assert_no_secret(
            "diagnostics",
            &diagnostics.snapshot.runtime["browser"].to_string(),
        );
        let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
            .env_clear()
            .env("HOME", root.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .arg("--config")
            .arg(&bundle.context.config_path)
            .args(["runtime", "inspect", "--workspace"])
            .arg(&bundle.context.workspace)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let line = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Browser: "))
            .ok_or("Browser line")?;
        let text: Value = serde_json::from_str(line)?;
        assert_eq!(text["view"], value["view"], "text and JSON agree");
        assert_no_secret("inspect text", &stdout);
        assert_no_secret("inspect stderr", &String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[test]
fn onboard_preserves_view_and_images_publish_loopback_viewer_ports() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bundle = view_bundle(
        root.path(),
        json!({"enable":true,"password":"${SHACS_BROWSER_VIEW_PASSWORD}"}),
        Some(&format!(r#"{{"SHACS_BROWSER_VIEW_PASSWORD":"{SECRET}"}}"#)),
    )?;
    shacs_cli::onboard(shacs_cli::OnboardOptions {
        config_path: Some(bundle.context.config_path.clone()),
        workspace: Some(bundle.context.workspace.clone()),
        wizard: false,
    })?;
    let saved_text = fs::read_to_string(&bundle.context.config_path)?;
    let saved: Value = serde_json::from_str(&saved_text)?;
    assert_eq!(
        saved["tools"]["browser"]["view"],
        json!({"enable":true,"password":"${SHACS_BROWSER_VIEW_PASSWORD}"}),
        "onboard keeps the reference and never writes the resolved value"
    );
    assert_no_secret("saved config", &saved_text);
    let stub_root = tempfile::tempdir()?;
    let stub = bundle_without_view(stub_root.path())?;
    shacs_cli::onboard(shacs_cli::OnboardOptions {
        config_path: Some(stub.clone()),
        workspace: Some(stub_root.path().join("workspace")),
        wizard: false,
    })?;
    let saved: Value = serde_json::from_slice(&fs::read(&stub)?)?;
    assert_eq!(
        saved["tools"]["browser"]["view"],
        json!({"enable":false,"password":""})
    );

    let docker = include_str!("../../../../Dockerfile");
    for package in ["x11vnc", "novnc", "websockify"] {
        assert!(
            docker.contains(&format!("        {package} \\\n")),
            "{package}"
        );
    }
    assert!(docker.contains("EXPOSE 18790 8900 8765 6080"));
    assert!(!docker.contains("5900"));
    let compose = include_str!("../../../../docker-compose.yml");
    assert_eq!(compose.matches("\"127.0.0.1:6080:6080\"").count(), 1);
    assert_eq!(compose.matches("\"127.0.0.1:6081:6080\"").count(), 1);
    assert!(!compose.contains("5900"));
    for line in compose.lines().filter(|line| line.contains(":6080")) {
        assert!(
            line.trim().starts_with("- \"127.0.0.1:"),
            "viewer publish must stay on host loopback: {line}"
        );
    }
    let gateway = compose.find("shacs-gateway:").ok_or("gateway service")?;
    let api = compose.find("shacs-api:").ok_or("api service")?;
    let gateway_ports = &compose[gateway..api];
    assert!(gateway_ports.contains("\"127.0.0.1:6080:6080\""));
    assert!(!gateway_ports.contains("6081"));
    let api_ports = &compose[api..];
    assert!(api_ports.contains("\"127.0.0.1:6081:6080\""));
    assert!(!api_ports.contains("\"127.0.0.1:6080:6080\""));
    Ok(())
}

fn bundle_without_view(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let written = bundle(root, PermissionMode::Default)?;
    let mut value: Value = serde_json::from_slice(&fs::read(&written.context.config_path)?)?;
    value["tools"]["browser"]
        .as_object_mut()
        .ok_or("browser object")?
        .remove("view");
    fs::write(&written.context.config_path, serde_json::to_vec(&value)?)?;
    Ok(written.context.config_path.clone())
}
