#[cfg(unix)]
use super::CdpConnection;
use super::{BrowserProxy, BrowserViewStatus, DevToolsPeer};
use shacs_core::runtime::{BrowserError, NetworkGuard};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
#[cfg(target_os = "linux")]
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserChildKind {
    Xauth,
    Xvfb,
    Chromium,
    Viewer,
}

#[derive(Debug, Clone)]
pub struct BrowserChildSpec {
    pub kind: BrowserChildKind,
    pub program: String,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

pub trait BrowserChild: Send {
    fn exited(&mut self) -> Result<bool, BrowserError>;
    fn succeeded(&mut self) -> Result<bool, BrowserError>;
    fn stop(&mut self);
    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError>;
    /// The operating-system process id, when the child is a real process.
    fn process_id(&self) -> Option<u32> {
        None
    }
}

/// Who holds the IPv4 loopback listener on a viewer port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerListener {
    Absent,
    /// The listening socket belongs to the child whose id was given.
    OwnedByChild,
    /// A listener exists but is not that child's: another process has the port.
    Foreign,
}

pub trait BrowserProcessRunner: Send + Sync {
    fn available(&self) -> bool;
    /// Whether the viewer programs (x11vnc, websockify) are installed here.
    fn viewer_available(&self) -> bool;
    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError>;
    fn display_ready(&self, display: u16, deadline: Instant) -> Result<(), BrowserError>;
    /// One non-blocking look at the `127.0.0.1:<port>` LISTEN socket and, when
    /// `owner` is given, whether that process holds it. Without an owner any
    /// listener is `Foreign`.
    fn viewer_listener(&self, port: u16, owner: Option<u32>) -> ViewerListener;
}

/// Viewer implementations receive only this sanitized generation environment.
/// `start` returns `Err` only for a viewer that was meant to run and could not;
/// a disabled or unavailable viewer returns `Ok` and says so in `view`.
pub trait BrowserLifecycleParticipant: Send {
    fn start(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        deadline: Instant,
    ) -> Result<(), BrowserError>;
    fn failed(&mut self) -> bool;
    fn stop(&mut self);
    fn view(&self) -> BrowserViewStatus;
}

pub trait BrowserLifecycleFactory: Send + Sync {
    fn create(&self) -> Box<dyn BrowserLifecycleParticipant>;
    /// The viewer facts while no generation runs: disabled, unavailable with
    /// its reason, or closed.
    fn view(&self, runner: &dyn BrowserProcessRunner) -> BrowserViewStatus;
}

pub struct BrowserEnvironment {
    root: PathBuf,
    display: u16,
    display_lease: PathBuf,
}

impl BrowserEnvironment {
    pub fn create() -> Result<Self, BrowserError> {
        let mut random = [0; 16];
        File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut random))
            .map_err(|_| BrowserError::Unavailable)?;
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let root = PathBuf::from(format!("/tmp/shacs-browser-{suffix}"));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        builder
            .create(&root)
            .map_err(|_| BrowserError::Unavailable)?;
        let result = (|| {
            for name in ["profile", "home", "config", "cache", "data", "run", "tmp"] {
                builder
                    .create(root.join(name))
                    .map_err(|_| BrowserError::Unavailable)?;
            }
            for display in 90..1000 {
                if Path::new(&format!("/tmp/.X11-unix/X{display}")).exists()
                    || Path::new(&format!("/tmp/.X{display}-lock")).exists()
                {
                    continue;
                }
                let lease = PathBuf::from(format!("/tmp/shacs-browser-display-{display}"));
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                options.mode(0o600);
                match options.open(&lease) {
                    Ok(_) => {
                        return Ok(Self {
                            root: root.clone(),
                            display,
                            display_lease: lease,
                        })
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(_) => return Err(BrowserError::Unavailable),
                }
            }
            Err(BrowserError::Unavailable)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&root);
        }
        result
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub const fn display(&self) -> u16 {
        self.display
    }

    pub fn child(
        &self,
        kind: BrowserChildKind,
        program: &str,
        arguments: Vec<String>,
    ) -> BrowserChildSpec {
        let mut environment = BTreeMap::from([
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("LANG".to_owned(), "C.UTF-8".to_owned()),
            ("DISPLAY".to_owned(), format!(":{}", self.display)),
            (
                "XAUTHORITY".to_owned(),
                self.root.join("Xauthority").to_string_lossy().to_string(),
            ),
        ]);
        for (key, folder) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_RUNTIME_DIR", "run"),
            ("TMPDIR", "tmp"),
        ] {
            environment.insert(
                key.to_owned(),
                self.root.join(folder).to_string_lossy().to_string(),
            );
        }
        BrowserChildSpec {
            kind,
            program: program.to_owned(),
            arguments,
            environment,
        }
    }

    pub fn chromium(&self, proxy_port: u16) -> BrowserChildSpec {
        self.child(
            BrowserChildKind::Chromium,
            "/usr/bin/chromium",
            vec![
                format!("--display=:{}", self.display),
                format!("--user-data-dir={}", self.root.join("profile").display()),
                "--no-first-run".to_owned(),
                "--no-default-browser-check".to_owned(),
                "--no-sandbox".to_owned(),
                "--remote-debugging-pipe".to_owned(),
                format!("--proxy-server=socks5://127.0.0.1:{proxy_port}"),
                "--proxy-bypass-list=<-loopback>".to_owned(),
                "--disable-quic".to_owned(),
                "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".to_owned(),
                "--disable-background-networking".to_owned(),
                "--disable-component-update".to_owned(),
                "--window-size=1280,720".to_owned(),
                "about:blank".to_owned(),
            ],
        )
    }
}

impl Drop for BrowserEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.display_lease);
    }
}

pub struct BrowserProcesses {
    pub environment: BrowserEnvironment,
    children: Vec<Box<dyn BrowserChild>>,
    proxy: Option<BrowserProxy>,
    viewer: Option<Box<dyn BrowserLifecycleParticipant>>,
    pub viewer_failed: bool,
}

impl BrowserProcesses {
    /// Starts the browser core: proxy, xauth, Xvfb, Chromium. The optional
    /// viewer is started afterwards by `start_viewer`, once the browser has
    /// proven itself, so viewer time never counts against this deadline.
    pub fn start(
        runner: &dyn BrowserProcessRunner,
        guard: NetworkGuard,
        deadline: Instant,
    ) -> Result<(Self, Box<dyn DevToolsPeer>), BrowserError> {
        let proxy = BrowserProxy::start(guard).map_err(|_| BrowserError::Unavailable)?;
        let environment = BrowserEnvironment::create()?;
        let mut processes = Self {
            environment,
            children: Vec::new(),
            proxy: Some(proxy),
            viewer: None,
            viewer_failed: false,
        };
        let mut cookie = [0; 16];
        File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut cookie))
            .map_err(|_| BrowserError::Unavailable)?;
        let cookie: String = cookie.iter().map(|byte| format!("{byte:02x}")).collect();
        let auth = processes.environment.root.join("Xauthority");
        let xauth = processes.environment.child(
            BrowserChildKind::Xauth,
            "/usr/bin/xauth",
            vec![
                "-f".to_owned(),
                auth.to_string_lossy().to_string(),
                "add".to_owned(),
                format!(":{}", processes.environment.display),
                ".".to_owned(),
                cookie,
            ],
        );
        let mut child = runner.spawn(&xauth, deadline)?;
        while !child.exited()? {
            if Instant::now() >= deadline {
                child.stop();
                return Err(BrowserError::StartupTimeout);
            }
            thread::sleep(Duration::from_millis(5));
        }
        if !child.succeeded()? {
            child.stop();
            return Err(BrowserError::Unavailable);
        }
        child.stop();
        let xvfb = processes.environment.child(
            BrowserChildKind::Xvfb,
            "/usr/bin/Xvfb",
            vec![
                format!(":{}", processes.environment.display),
                "-screen".to_owned(),
                "0".to_owned(),
                "1280x720x24".to_owned(),
                "-nolisten".to_owned(),
                "tcp".to_owned(),
                "-auth".to_owned(),
                auth.to_string_lossy().to_string(),
            ],
        );
        processes.children.push(runner.spawn(&xvfb, deadline)?);
        runner.display_ready(processes.environment.display, deadline)?;
        let proxy_port = processes
            .proxy
            .as_ref()
            .ok_or(BrowserError::Unavailable)?
            .address()
            .port();
        let chromium = processes.environment.chromium(proxy_port);
        processes.children.push(runner.spawn(&chromium, deadline)?);
        let peer = processes
            .children
            .last_mut()
            .ok_or(BrowserError::Protocol)?
            .take_devtools()?;
        if Instant::now() >= deadline {
            return Err(BrowserError::StartupTimeout);
        }
        Ok((processes, peer))
    }

    /// Starts the optional viewer with its own `deadline`. A viewer that
    /// should run and cannot is stopped and recorded as failed; the browser
    /// is never affected. The participant stays attached either way so it
    /// keeps reporting its state for this generation.
    pub fn start_viewer(
        &mut self,
        runner: &dyn BrowserProcessRunner,
        mut viewer: Option<Box<dyn BrowserLifecycleParticipant>>,
        deadline: Instant,
    ) {
        if let Some(participant) = viewer.as_mut() {
            if participant
                .start(&self.environment, runner, deadline)
                .is_err()
            {
                participant.stop();
                self.viewer_failed = true;
            }
        }
        self.viewer = viewer;
    }

    pub fn alive(&mut self) -> Result<bool, BrowserError> {
        if let Some(viewer) = self.viewer.as_mut() {
            if !self.viewer_failed && viewer.failed() {
                viewer.stop();
                self.viewer_failed = true;
            }
        }
        for child in &mut self.children {
            if child.exited()? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn view(&self) -> Option<BrowserViewStatus> {
        self.viewer.as_ref().map(|viewer| viewer.view())
    }
}

impl Drop for BrowserProcesses {
    fn drop(&mut self) {
        if let Some(viewer) = self.viewer.as_mut() {
            viewer.stop();
        }
        for child in self.children.iter_mut().rev() {
            child.stop();
        }
        self.proxy.take();
        // environment is dropped after all children and the proxy have stopped.
    }
}

pub struct LinuxBrowserProcessRunner;

struct LinuxBrowserChild {
    child: Child,
    peer: Option<Box<dyn DevToolsPeer>>,
    stopped: bool,
}

impl BrowserChild for LinuxBrowserChild {
    fn exited(&mut self) -> Result<bool, BrowserError> {
        self.child
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|_| BrowserError::ProcessExited)
    }
    fn succeeded(&mut self) -> Result<bool, BrowserError> {
        self.child
            .try_wait()
            .map(|status| status.is_some_and(|status| status.success()))
            .map_err(|_| BrowserError::ProcessExited)
    }
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        #[cfg(unix)]
        // SAFETY: the PID is a child process group created by this runner.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn take_devtools(&mut self) -> Result<Box<dyn DevToolsPeer>, BrowserError> {
        self.peer.take().ok_or(BrowserError::Protocol)
    }
    fn process_id(&self) -> Option<u32> {
        Some(self.child.id())
    }
}

impl Drop for LinuxBrowserChild {
    fn drop(&mut self) {
        self.stop();
    }
}

impl BrowserProcessRunner for LinuxBrowserProcessRunner {
    fn available(&self) -> bool {
        cfg!(target_os = "linux")
            && ["/usr/bin/chromium", "/usr/bin/Xvfb", "/usr/bin/xauth"]
                .iter()
                .all(|path| Path::new(path).is_file())
            && policy_ready()
    }

    fn viewer_available(&self) -> bool {
        cfg!(target_os = "linux")
            && [
                super::browser_viewer::X11VNC,
                super::browser_viewer::WEBSOCKIFY,
            ]
            .iter()
            .all(|path| Path::new(path).is_file())
    }

    fn viewer_listener(&self, port: u16, owner: Option<u32>) -> ViewerListener {
        #[cfg(target_os = "linux")]
        {
            return linux_loopback_listener(port, owner);
        }
        #[cfg(not(target_os = "linux"))]
        {
            // Ownership cannot be proven here; the viewer must not start.
            let _ = (port, owner);
            ViewerListener::Foreign
        }
    }

    fn spawn(
        &self,
        spec: &BrowserChildSpec,
        deadline: Instant,
    ) -> Result<Box<dyn BrowserChild>, BrowserError> {
        if Instant::now() >= deadline {
            return Err(BrowserError::StartupTimeout);
        }
        if !cfg!(target_os = "linux") {
            return Err(BrowserError::Unavailable);
        }
        if !child_environment_is_confined(spec) {
            return Err(BrowserError::StartDenied);
        }
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.arguments)
            .env_clear()
            .envs(&spec.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(target_os = "linux")]
        {
            let parent = std::process::id();
            // SAFETY: only async-signal-safe syscalls are used after fork.
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() as u32 != parent {
                        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                    }
                    Ok(())
                });
            }
        }
        #[cfg(unix)]
        let pipes = if spec.kind == BrowserChildKind::Chromium {
            Some(devtools_pipes(&mut command)?)
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let child = spawn_from_persistent_thread(command).map_err(|_| BrowserError::Unavailable)?;
        #[cfg(not(target_os = "linux"))]
        let child = command.spawn().map_err(|_| BrowserError::Unavailable)?;
        #[cfg(unix)]
        let peer = pipes
            .map(|(reader, writer, child_input, child_output)| {
                drop(child_input);
                drop(child_output);
                CdpConnection::new(reader.into(), writer.into())
                    .map(|peer| Box::new(peer) as Box<dyn DevToolsPeer>)
            })
            .transpose();
        #[cfg(unix)]
        let peer = match peer {
            Ok(peer) => peer,
            Err(error) => {
                let mut child = LinuxBrowserChild {
                    child,
                    peer: None,
                    stopped: false,
                };
                child.stop();
                return Err(error);
            }
        };
        #[cfg(not(unix))]
        let peer = None;
        Ok(Box::new(LinuxBrowserChild {
            child,
            peer,
            stopped: false,
        }))
    }

    fn display_ready(&self, display: u16, deadline: Instant) -> Result<(), BrowserError> {
        #[cfg(unix)]
        while Instant::now() < deadline {
            if std::os::unix::net::UnixStream::connect(format!("/tmp/.X11-unix/X{display}")).is_ok()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(BrowserError::StartupTimeout)
    }
}

/// Spawns on one thread that lives as long as the process. `PR_SET_PDEATHSIG`
/// fires when the parent *thread* that forked the child exits, not only when
/// the process does; a child spawned from a per-turn worker thread (the
/// gateway's WebSocket turn, an idle-expired blocking thread) would be killed
/// the moment that thread returned. Process exit still ends this thread and
/// therefore every child.
#[cfg(target_os = "linux")]
fn spawn_from_persistent_thread(mut command: Command) -> std::io::Result<Child> {
    type Job = Box<dyn FnOnce() + Send>;
    static SPAWNER: OnceLock<Option<Mutex<mpsc::Sender<Job>>>> = OnceLock::new();
    let sender = SPAWNER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Job>();
            thread::Builder::new()
                .name("shacs-browser-spawner".to_owned())
                .spawn(move || {
                    for job in receiver {
                        job();
                    }
                })
                .ok()
                .map(|_| Mutex::new(sender))
        })
        .as_ref()
        .ok_or_else(|| std::io::Error::other("browser spawner thread unavailable"))?;
    let (reply, spawned) = mpsc::channel();
    let job: Job = Box::new(move || {
        let _ = reply.send(command.spawn());
    });
    sender
        .lock()
        .map_err(|_| std::io::Error::other("browser spawner lock poisoned"))?
        .send(job)
        .map_err(|_| std::io::Error::other("browser spawner thread gone"))?;
    spawned
        .recv()
        .map_err(|_| std::io::Error::other("browser spawner thread gone"))?
}

/// Reads `/proc/net/tcp` for a LISTEN socket on `127.0.0.1:<port>` and maps
/// its inode to the file descriptors of `owner`.
#[cfg(target_os = "linux")]
fn linux_loopback_listener(port: u16, owner: Option<u32>) -> ViewerListener {
    let Ok(table) = fs::read_to_string("/proc/net/tcp") else {
        return ViewerListener::Foreign;
    };
    let local = format!("0100007F:{port:04X}");
    let inodes: Vec<&str> = table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.get(1) == Some(&local.as_str()) && fields.get(3) == Some(&"0A"))
                .then(|| fields.get(9).copied())
                .flatten()
        })
        .collect();
    if inodes.is_empty() {
        return ViewerListener::Absent;
    }
    let Some(owner) = owner else {
        return ViewerListener::Foreign;
    };
    let Ok(descriptors) = fs::read_dir(format!("/proc/{owner}/fd")) else {
        return ViewerListener::Foreign;
    };
    let owned = descriptors
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_link(entry.path()).ok())
        .any(|target| {
            inodes
                .iter()
                .any(|inode| target.to_string_lossy() == format!("socket:[{inode}]"))
        });
    if owned {
        ViewerListener::OwnedByChild
    } else {
        ViewerListener::Foreign
    }
}

pub fn child_environment_is_confined(spec: &BrowserChildSpec) -> bool {
    let Some(temporary) = spec.environment.get("TMPDIR") else {
        return false;
    };
    let Some(root) = Path::new(temporary).parent() else {
        return false;
    };
    if root.parent() != Some(Path::new("/tmp"))
        || !root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("shacs-browser-"))
    {
        return false;
    }
    spec.environment
        .iter()
        .all(|(key, value)| match key.as_str() {
            "PATH" => value == "/usr/bin:/bin",
            "LANG" | "LC_ALL" | "LC_CTYPE" => value == "C.UTF-8",
            "DISPLAY" => value
                .strip_prefix(':')
                .is_some_and(|number| number.parse::<u16>().is_ok()),
            "XAUTHORITY" => Path::new(value) == root.join("Xauthority"),
            "HOME" => Path::new(value) == root.join("home"),
            "XDG_CONFIG_HOME" => Path::new(value) == root.join("config"),
            "XDG_CACHE_HOME" => Path::new(value) == root.join("cache"),
            "XDG_DATA_HOME" => Path::new(value) == root.join("data"),
            "XDG_RUNTIME_DIR" => Path::new(value) == root.join("run"),
            "TMPDIR" => Path::new(value) == root.join("tmp"),
            _ => false,
        })
}

#[cfg(unix)]
fn devtools_pipes(command: &mut Command) -> Result<(File, File, File, File), BrowserError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    fn pipe() -> std::io::Result<(File, File)> {
        let mut descriptors = [0; 2];
        // SAFETY: pipe writes two valid descriptors into the provided array.
        if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: ownership of both new descriptors is transferred exactly once.
        let (read, write) = unsafe {
            (
                File::from_raw_fd(descriptors[0]),
                File::from_raw_fd(descriptors[1]),
            )
        };
        for fd in [read.as_raw_fd(), write.as_raw_fd()] {
            // SAFETY: fd is live and F_SETFD takes an integer.
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok((read, write))
    }
    let (input, writer) = pipe().map_err(|_| BrowserError::Unavailable)?;
    let (reader, output) = pipe().map_err(|_| BrowserError::Unavailable)?;
    // Duplicate above 4 before assigning 3/4, so remapping cannot clobber a source.
    let input_fd = input.as_raw_fd();
    let output_fd = output.as_raw_fd();
    // SAFETY: descriptors live until spawn returns; closure uses only safe-to-fork syscalls.
    unsafe {
        command.pre_exec(move || {
            let input_copy = libc::fcntl(input_fd, libc::F_DUPFD_CLOEXEC, 5);
            if input_copy < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let output_copy = libc::fcntl(output_fd, libc::F_DUPFD_CLOEXEC, 5);
            if output_copy < 0 {
                libc::close(input_copy);
                return Err(std::io::Error::last_os_error());
            }
            let first = libc::dup2(input_copy, 3);
            let second = libc::dup2(output_copy, 4);
            libc::close(input_copy);
            libc::close(output_copy);
            if first < 0 || second < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok((reader, writer, input, output))
}

pub fn policy_ready() -> bool {
    let path = Path::new("/etc/chromium/policies/managed/shacs-browser.json");
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != 0 || metadata.mode() & 0o777 != 0o644 {
            return false;
        }
    }
    if !metadata.is_file() {
        return false;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    value
        == serde_json::json!({
            "URLBlocklist":["*"],"URLAllowlist":["http://*","https://*"],
            "DownloadRestrictions":3,"DefaultFileSystemReadGuardSetting":2,"DefaultFileSystemWriteGuardSetting":2
        })
}
