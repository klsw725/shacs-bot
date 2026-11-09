//! Spec 052 PRD 020 acceptance 1–7: `scripts/docker-setup.sh` copied into a
//! temporary repository layout and run with a fake `docker` that logs each
//! call, the user ids it got, its environment and the token it was given.
//! PATH holds only the fake tools and links to the few real ones the script
//! needs, so no real `docker` is ever found.

use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;

const TOKEN: &str = "spec052-setup-token-canary-6610";
const WAIT: Duration = Duration::from_secs(60);
const NON_ROOT: u32 = 1001;
const MISSING_PROVIDER: &str =
    "provider: missing none\nreason: no provider\nnext: shacs-bot provider codex login";
const READY_PROVIDER: &str = "provider: ready openai_codex\ncredential: sign-in";
const NOT_CONNECTED: &str = "discord: not connected\nreason: channels.discord.enabled is not true";
const CONNECTED: &str = "discord: connected\ntoken: channels.discord.token";

const FAKE_DOCKER: &str = r#"#!/bin/sh
log="$FAKE_DOCKER_DIR"
printf '%s\n' "$*" >> "$log/calls"
printf '%s %s\n' "${SHACS_UID-unset}" "${SHACS_GID-unset}" >> "$log/ids"
case "$*" in
  "compose version") echo "Docker Compose version v2.30.0"; exit "${FAKE_VERSION_EXIT:-0}" ;;
  "compose build") exit "${FAKE_BUILD_EXIT:-0}" ;;
  "compose run --rm -T shacs-cli onboard") exit "${FAKE_ONBOARD_EXIT:-0}" ;;
  "compose run --rm -T shacs-cli provider status")
    count=$(cat "$log/provider-count" 2>/dev/null || echo 0)
    count=$((count + 1))
    echo "$count" > "$log/provider-count"
    if [ "$count" -gt 1 ] && [ -n "${FAKE_PROVIDER_AFTER:-}" ]; then
      printf '%s\n' "$FAKE_PROVIDER_AFTER"
    else
      printf '%s\n' "$FAKE_PROVIDER"
    fi
    exit "${FAKE_PROVIDER_EXIT:-0}" ;;
  "compose run --rm -T shacs-cli provider codex login --headless") exit "${FAKE_LOGIN_EXIT:-0}" ;;
  "compose run --rm -T shacs-cli channels connect discord --status")
    printf '%s\n' "$FAKE_DISCORD"
    exit "${FAKE_DISCORD_EXIT:-0}" ;;
  "compose run --rm -T shacs-cli channels connect discord --token-stdin"*)
    cat > "$log/stdin"
    env > "$log/env"
    exit "${FAKE_CONNECT_EXIT:-0}" ;;
  "compose ps --status running --services")
    if [ -n "${FAKE_RUNNING:-}" ]; then echo shacs-gateway; fi
    exit "${FAKE_PS_EXIT:-0}" ;;
  "compose up -d shacs-gateway") exit "${FAKE_UP_EXIT:-0}" ;;
  "compose restart shacs-gateway") exit 0 ;;
esac
echo "fake docker: unexpected call: $*" >&2
exit 97
"#;

/// The real tools the script and the fake `docker` use.
const TOOLS: [&str; 13] = [
    "awk", "cat", "cp", "dirname", "env", "grep", "id", "ls", "mkdir", "mktemp", "mv", "rm", "stty",
];

/// A copied repository, a home folder and the fake tools in one temporary
/// folder.
struct Setup {
    root: tempfile::TempDir,
    /// The user the script runs as when the test itself runs as root.
    as_user: Option<u32>,
}

impl Setup {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("scripts"))?;
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::copy(
            source.join("scripts/docker-setup.sh"),
            repo.join("scripts/docker-setup.sh"),
        )?;
        fs::copy(
            source.join("docker-compose.yml"),
            repo.join("docker-compose.yml"),
        )?;
        fs::create_dir_all(root.path().join("home"))?;
        fs::create_dir_all(root.path().join("log"))?;
        let tools = root.path().join("tools");
        fs::create_dir_all(&tools)?;
        for tool in TOOLS {
            let real = ["/bin", "/usr/bin"]
                .iter()
                .map(|folder| Path::new(folder).join(tool))
                .find(|path| path.exists())
                .ok_or_else(|| format!("no {tool} in /bin or /usr/bin"))?;
            std::os::unix::fs::symlink(real, tools.join(tool))?;
        }
        let setup = Self {
            root,
            as_user: None,
        };
        setup.write_tool("docker", FAKE_DOCKER)?;
        // SAFETY: `geteuid` only reads this process's effective user id.
        let as_user = (unsafe { libc::geteuid() } == 0).then_some(NON_ROOT);
        if let Some(user) = as_user {
            chown_tree(setup.root.path(), user)?;
        }
        Ok(Self { as_user, ..setup })
    }

    fn repo(&self) -> PathBuf {
        self.root.path().join("repo")
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn data(&self) -> PathBuf {
        self.home().join(".shacs-bot")
    }

    fn env_file(&self) -> PathBuf {
        self.repo().join(".env")
    }

    fn log(&self, name: &str) -> PathBuf {
        self.root.path().join("log").join(name)
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    /// Writes an executable fake tool that PATH finds before the real ones.
    fn write_tool(&self, name: &str, script: &str) -> TestResult {
        fs::create_dir_all(self.bin())?;
        let path = self.bin().join(name);
        fs::write(&path, script)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        if let Some(user) = self.as_user {
            std::os::unix::fs::chown(&path, Some(user), Some(user))?;
        }
        Ok(())
    }

    /// The user and group the script sees as its own.
    fn ids(&self) -> (u32, u32) {
        match self.as_user {
            Some(user) => (user, user),
            // SAFETY: `getuid` and `getgid` only read this process's ids.
            None => unsafe { (libc::getuid(), libc::getgid()) },
        }
    }

    fn command(&self, environment: &[(&str, &str)], with_fake_docker: bool) -> Command {
        let tools = self.root.path().join("tools");
        let path = if with_fake_docker {
            format!("{}:{}", self.bin().display(), tools.display())
        } else {
            tools.display().to_string()
        };
        let mut command = Command::new("/bin/sh");
        command
            .arg(self.repo().join("scripts/docker-setup.sh"))
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", path)
            .env("FAKE_DOCKER_DIR", self.root.path().join("log"))
            .env("FAKE_PROVIDER", MISSING_PROVIDER)
            .env("FAKE_DISCORD", NOT_CONNECTED);
        for (name, value) in environment {
            command.env(name, value);
        }
        if let Some(user) = self.as_user {
            command.uid(user).gid(user);
        }
        command
    }

    /// Runs the script without a terminal.
    fn run(&self, environment: &[(&str, &str)]) -> Result<Output, Box<dyn Error>> {
        Ok(self
            .command(environment, true)
            .stdin(Stdio::null())
            .output()?)
    }

    fn calls(&self) -> Result<Vec<String>, Box<dyn Error>> {
        read_lines(&self.log("calls"))
    }

    fn leftover_temp_files(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut leftovers = Vec::new();
        for entry in fs::read_dir(self.repo())? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if name.starts_with(".env.docker-setup.") {
                leftovers.push(name);
            }
        }
        Ok(leftovers)
    }
}

fn chown_tree(path: &Path, user: u32) -> TestResult {
    std::os::unix::fs::lchown(path, Some(user), Some(user))?;
    if path.is_dir() && !path.is_symlink() {
        for entry in fs::read_dir(path)? {
            chown_tree(&entry?.path(), user)?;
        }
    }
    Ok(())
}

fn read_lines(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn first_run_calls() -> Vec<String> {
    [
        "compose version",
        "compose build",
        "compose run --rm -T shacs-cli onboard",
        "compose run --rm -T shacs-cli provider status",
        "compose run --rm -T shacs-cli channels connect discord --status",
        "compose ps --status running --services",
        "compose up -d shacs-gateway",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn real_repo_env() -> Option<Vec<u8>> {
    fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env")).ok()
}

#[test]
fn a_first_run_without_a_terminal_sets_up_and_starts_the_gateway() -> TestResult {
    // Given
    let real_env_before = real_repo_env();
    let setup = Setup::new()?;
    let (uid, gid) = setup.ids();

    // When
    let first = setup.run(&[])?;
    let env_after_first = fs::metadata(setup.env_file())?;
    let env_bytes_after_first = fs::read(setup.env_file())?;
    let first_calls = setup.calls()?;
    let second = setup.run(&[])?;

    // Then
    assert!(first.status.success(), "{}", text(&first));
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(fs::metadata(setup.data())?.uid(), uid);
    assert_eq!(
        String::from_utf8(env_bytes_after_first.clone())?,
        format!("SHACS_UID={uid}\nSHACS_GID={gid}\n")
    );
    assert_eq!(env_after_first.permissions().mode() & 0o777, 0o600);
    assert_eq!(first_calls, first_run_calls());
    let ids = read_lines(&setup.log("ids"))?;
    assert!(
        ids.iter().all(|line| line == &format!("{uid} {gid}")),
        "{ids:?}"
    );
    let first_text = text(&first);
    assert!(!first_text.contains("[y/N]") && !first_text.contains("Choose 1 or 2"));
    assert!(
        first_text.contains("provider codex login --headless"),
        "{first_text}"
    );
    assert!(
        first_text.contains("channels connect discord --token-stdin"),
        "{first_text}"
    );
    let env_after_second = fs::metadata(setup.env_file())?;
    assert_eq!(env_after_second.ino(), env_after_first.ino());
    assert_eq!(fs::read(setup.env_file())?, env_bytes_after_first);
    let all_calls = setup.calls()?;
    assert_eq!(all_calls[first_calls.len()..], first_calls[..]);
    assert!(setup.leftover_temp_files()?.is_empty());
    assert_eq!(real_repo_env(), real_env_before);
    Ok(())
}

#[test]
fn the_env_file_keeps_other_lines_and_its_mode() -> TestResult {
    let cases = [
        (
            "other lines",
            "# SHACS_UID=1000\nANTHROPIC_API_KEY=\nSHACS_UID=999\nOTHER=1\n",
            0o640,
        ),
        (
            "repeated lines",
            "SHACS_UID=1\nSHACS_GID=1\nSHACS_UID=2\n",
            0o600,
        ),
    ];
    for (name, before, mode) in cases {
        // Given
        let setup = Setup::new()?;
        let (uid, gid) = setup.ids();
        fs::write(setup.env_file(), before)?;
        fs::set_permissions(setup.env_file(), fs::Permissions::from_mode(mode))?;
        if let Some(user) = setup.as_user {
            std::os::unix::fs::chown(setup.env_file(), Some(user), Some(user))?;
        }

        // When
        let output = setup.run(&[])?;

        // Then
        assert!(output.status.success(), "{name}: {}", text(&output));
        let after = fs::read_to_string(setup.env_file())?;
        let expected = match name {
            "other lines" => format!(
                "# SHACS_UID=1000\nANTHROPIC_API_KEY=\nSHACS_UID={uid}\nOTHER=1\nSHACS_GID={gid}\n"
            ),
            _ => format!("SHACS_UID={uid}\nSHACS_GID={gid}\n"),
        };
        assert_eq!(after, expected, "{name}");
        assert_eq!(
            fs::metadata(setup.env_file())?.permissions().mode() & 0o777,
            mode,
            "{name}"
        );
        if name == "other lines" {
            assert!(
                text(&output).contains(&format!("Changing SHACS_UID in .env from 999 to {uid}"))
            );
        }
        assert!(setup.leftover_temp_files()?.is_empty(), "{name}");
    }

    // A replace that fails leaves the file and no temporary copy behind.
    let setup = Setup::new()?;
    fs::write(setup.env_file(), "SHACS_UID=999\n")?;
    if let Some(user) = setup.as_user {
        std::os::unix::fs::chown(setup.env_file(), Some(user), Some(user))?;
    }
    setup.write_tool("mv", "#!/bin/sh\nexit 1\n")?;
    let output = setup.run(&[])?;
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert_eq!(fs::read_to_string(setup.env_file())?, "SHACS_UID=999\n");
    assert!(setup.leftover_temp_files()?.is_empty());
    assert_eq!(setup.calls()?, ["compose version"]);
    Ok(())
}

#[test]
fn ids_exported_by_the_shell_are_replaced_and_reported() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (uid, gid) = setup.ids();

    // When
    let output = setup.run(&[("SHACS_UID", "4242"), ("SHACS_GID", "4242")])?;

    // Then
    assert!(output.status.success(), "{}", text(&output));
    let ids = read_lines(&setup.log("ids"))?;
    assert!(
        ids.iter().all(|line| line == &format!("{uid} {gid}")),
        "{ids:?}"
    );
    assert!(text(&output).contains("unset SHACS_UID SHACS_GID"));
    Ok(())
}

#[test]
fn refusals_change_nothing() -> TestResult {
    // Root: the script stops before any change or docker call.
    let root_user = Setup::new()?;
    root_user.write_tool("id", "#!/bin/sh\necho 0\n")?;
    let output = root_user.run(&[])?;
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(!root_user.data().exists() && !root_user.env_file().exists());
    assert!(root_user.calls()?.is_empty());

    // A data folder the user cannot write.
    let locked = Setup::new()?;
    fs::create_dir(locked.data())?;
    if let Some(user) = locked.as_user {
        std::os::unix::fs::chown(locked.data(), Some(user), Some(user))?;
    }
    fs::set_permissions(locked.data(), fs::Permissions::from_mode(0o555))?;
    let output = locked.run(&[])?;
    fs::set_permissions(locked.data(), fs::Permissions::from_mode(0o755))?;
    let (uid, gid) = locked.ids();
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(text(&output).contains(&format!("belongs to {uid}:{gid}")));
    assert!(text(&output).contains("sudo chown -R"));
    assert!(!locked.env_file().exists());
    assert_eq!(locked.calls()?, ["compose version"]);

    // A file where the data folder belongs.
    let file = Setup::new()?;
    fs::write(file.data(), "not a folder")?;
    let output = file.run(&[])?;
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(!file.env_file().exists());

    // No docker on PATH, and no Compose v2.
    let no_docker = Setup::new()?;
    let output = no_docker
        .command(&[], false)
        .stdin(Stdio::null())
        .output()?;
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(text(&output).contains("docker is not installed"));
    assert!(!no_docker.data().exists() && !no_docker.env_file().exists());
    let no_compose = Setup::new()?;
    let output = no_compose.run(&[("FAKE_VERSION_EXIT", "1")])?;
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(!no_compose.env_file().exists());
    Ok(())
}

#[test]
fn a_failed_required_step_stops_the_script() -> TestResult {
    let steps = [
        ("FAKE_BUILD_EXIT", "compose build"),
        ("FAKE_ONBOARD_EXIT", "compose run --rm -T shacs-cli onboard"),
        (
            "FAKE_PROVIDER_EXIT",
            "compose run --rm -T shacs-cli provider status",
        ),
        (
            "FAKE_DISCORD_EXIT",
            "compose run --rm -T shacs-cli channels connect discord --status",
        ),
        ("FAKE_PS_EXIT", "compose ps --status running --services"),
        ("FAKE_UP_EXIT", "compose up -d shacs-gateway"),
    ];
    for (variable, failing_call) in steps {
        // Given
        let setup = Setup::new()?;

        // When
        let output = setup.run(&[(variable, "1")])?;

        // Then
        assert_eq!(
            output.status.code(),
            Some(1),
            "{variable}: {}",
            text(&output)
        );
        let calls = setup.calls()?;
        assert_eq!(
            calls.last().map(String::as_str),
            Some(failing_call),
            "{calls:?}"
        );
    }
    Ok(())
}

/// The script running with a terminal as standard input.
struct Interactive {
    child: Child,
    master: OwnedFd,
    slave: OwnedFd,
    output: Receiver<String>,
    seen: String,
    /// Where in `seen` the next expected text is looked for.
    cursor: usize,
}

impl Interactive {
    fn start(setup: &Setup, environment: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let (master, slave) = open_pty()?;
        let mut child = setup
            .command(environment, true)
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (sender, output) = mpsc::channel();
        let pipes: [Box<dyn Read + Send>; 2] = [
            Box::new(child.stdout.take().ok_or("stdout")?),
            Box::new(child.stderr.take().ok_or("stderr")?),
        ];
        for mut pipe in pipes {
            let sender = sender.clone();
            std::thread::spawn(move || {
                let mut buffer = [0_u8; 512];
                while let Ok(read) = pipe.read(&mut buffer) {
                    if read == 0
                        || sender
                            .send(String::from_utf8_lossy(&buffer[..read]).into_owned())
                            .is_err()
                    {
                        break;
                    }
                }
            });
        }
        Ok(Self {
            child,
            master,
            slave,
            output,
            seen: String::new(),
            cursor: 0,
        })
    }

    /// Waits for the next `text` after the last one found.
    fn expect(&mut self, text: &str) -> TestResult {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(at) = self.seen[self.cursor..].find(text) {
                self.cursor += at + text.len();
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let chunk = self
                .output
                .recv_timeout(remaining)
                .map_err(|_| format!("no `{text}` in: {}", self.seen))?;
            self.seen.push_str(&chunk);
        }
    }

    fn answer(&mut self, question: &str, reply: &str) -> TestResult {
        self.expect(question)?;
        let typed = if reply == END_OF_INPUT {
            reply.to_owned()
        } else {
            format!("{reply}\n")
        };
        let mut master = fs::File::from(self.master.try_clone()?);
        master.write_all(typed.as_bytes())?;
        Ok(())
    }

    /// Waits until the script's output ends and it exits; returns its status
    /// and all output.
    fn finish(mut self) -> Result<(ExitStatus, String, Interactive), Box<dyn Error>> {
        let deadline = Instant::now() + WAIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(chunk) => self.seen.push_str(&chunk),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.child.kill()?;
                    return Err(format!("the script did not end: {}", self.seen).into());
                }
            }
        }
        let status = self.child.wait()?;
        let seen = self.seen.clone();
        Ok((status, seen, self))
    }

    fn echo(&self) -> Result<bool, Box<dyn Error>> {
        let mut settings = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `settings` is valid for writing one `termios`.
        if unsafe { libc::tcgetattr(self.slave.as_raw_fd(), settings.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: `tcgetattr` succeeded and filled it.
        Ok(unsafe { settings.assume_init() }.c_lflag & libc::ECHO != 0)
    }

    /// What the terminal showed of the typed input.
    fn shown(&self) -> Result<String, Box<dyn Error>> {
        let mut master = fs::File::from(self.master.try_clone()?);
        let mut shown = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => shown.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(String::from_utf8_lossy(&shown).into_owned())
    }
}

fn open_pty() -> Result<(OwnedFd, OwnedFd), Box<dyn Error>> {
    let mut master = -1;
    let mut slave = -1;
    let settings: *mut libc::termios = std::ptr::null_mut();
    let size: *mut libc::winsize = std::ptr::null_mut();
    // SAFETY: `openpty` writes two descriptors; null name, settings and size
    // pointers are allowed.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            settings,
            size,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: `openpty` succeeded, so both are open descriptors owned here.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // SAFETY: setting a status flag on an open descriptor.
    if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((master, slave))
}

const LOGIN: &str = "Choose 1 or 2 [2]: ";
const CONNECT: &str = "Connect a Discord bot now? [y/N]: ";
const TOKEN_PROMPT: &str = "Discord bot token (input hidden): ";
const USER_ID: &str = "empty allows anyone): ";
const ANYONE: &str = "Continue without a user ID? [y/N]: ";
/// Ctrl-D at the start of a line: the terminal's end of input.
const END_OF_INPUT: &str = "\u{4}";
const SIGN_IN_TODO: &str = "Sign in:";
const DISCORD_TODO: &str = "Connect Discord:";

#[test]
fn a_ready_provider_and_a_connected_bot_ask_nothing() -> TestResult {
    // Given
    let setup = Setup::new()?;

    // When
    let session = Interactive::start(
        &setup,
        &[
            ("FAKE_PROVIDER", READY_PROVIDER),
            ("FAKE_DISCORD", CONNECTED),
        ],
    )?;
    let (status, output, _) = session.finish()?;

    // Then
    assert!(status.success(), "{output}");
    assert!(
        !output.contains(LOGIN) && !output.contains(CONNECT),
        "{output}"
    );
    assert!(!output.contains("Still to do"), "{output}");
    Ok(())
}

#[test]
fn the_questions_sign_in_connect_and_restart_as_answered() -> TestResult {
    struct Case {
        name: &'static str,
        environment: &'static [(&'static str, &'static str)],
        answers: &'static [(&'static str, &'static str)],
        login: bool,
        connect: Option<&'static str>,
        restart: bool,
        sign_in_todo: bool,
        discord_todo: bool,
    }
    let cases = [
        Case {
            name: "sign in on a running gateway",
            environment: &[("FAKE_RUNNING", "1")],
            answers: &[(LOGIN, "1"), (CONNECT, "n")],
            login: true,
            connect: None,
            restart: true,
            sign_in_todo: true,
            discord_todo: true,
        },
        Case {
            name: "skip everything on a running gateway",
            environment: &[("FAKE_RUNNING", "1")],
            answers: &[(LOGIN, "2"), (CONNECT, "n")],
            login: false,
            connect: None,
            restart: false,
            sign_in_todo: true,
            discord_todo: true,
        },
        Case {
            name: "sign in with the gateway stopped, ready after",
            environment: &[("FAKE_PROVIDER_AFTER", READY_PROVIDER)],
            answers: &[(LOGIN, "1"), (CONNECT, "n")],
            login: true,
            connect: None,
            restart: false,
            sign_in_todo: false,
            discord_todo: true,
        },
        Case {
            name: "a failed sign-in goes on",
            environment: &[("FAKE_RUNNING", "1"), ("FAKE_LOGIN_EXIT", "1")],
            answers: &[(LOGIN, "1"), (CONNECT, "n")],
            login: true,
            connect: None,
            restart: false,
            sign_in_todo: true,
            discord_todo: true,
        },
        Case {
            name: "an empty user id asked again",
            environment: &[("FAKE_RUNNING", "1")],
            answers: &[
                (LOGIN, "2"),
                (CONNECT, "y"),
                (TOKEN_PROMPT, TOKEN),
                (USER_ID, ""),
                (ANYONE, "n"),
                (USER_ID, "123456"),
            ],
            login: false,
            connect: Some(
                "compose run --rm -T shacs-cli channels connect discord --token-stdin --allow-from 123456",
            ),
            restart: true,
            sign_in_todo: true,
            discord_todo: false,
        },
        Case {
            name: "an empty user id confirmed",
            environment: &[],
            answers: &[
                (LOGIN, "2"),
                (CONNECT, "y"),
                (TOKEN_PROMPT, TOKEN),
                (USER_ID, ""),
                (ANYONE, "y"),
            ],
            login: false,
            connect: Some("compose run --rm -T shacs-cli channels connect discord --token-stdin"),
            restart: false,
            sign_in_todo: true,
            discord_todo: false,
        },
        Case {
            name: "a failed connection goes on",
            environment: &[("FAKE_RUNNING", "1"), ("FAKE_CONNECT_EXIT", "1")],
            answers: &[
                (LOGIN, "2"),
                (CONNECT, "y"),
                (TOKEN_PROMPT, TOKEN),
                (USER_ID, "123456"),
            ],
            login: false,
            connect: Some(
                "compose run --rm -T shacs-cli channels connect discord --token-stdin --allow-from 123456",
            ),
            restart: false,
            sign_in_todo: true,
            discord_todo: true,
        },
        Case {
            name: "end of input at the token",
            environment: &[("FAKE_RUNNING", "1")],
            answers: &[
                (LOGIN, "2"),
                (CONNECT, "y"),
                (TOKEN_PROMPT, END_OF_INPUT),
            ],
            login: false,
            connect: None,
            restart: false,
            sign_in_todo: true,
            discord_todo: true,
        },
        Case {
            name: "end of input at the user id",
            environment: &[("FAKE_RUNNING", "1")],
            answers: &[
                (LOGIN, "2"),
                (CONNECT, "y"),
                (TOKEN_PROMPT, TOKEN),
                (USER_ID, END_OF_INPUT),
            ],
            login: false,
            connect: None,
            restart: false,
            sign_in_todo: true,
            discord_todo: true,
        },
        Case {
            name: "end of input when asked to allow anyone",
            environment: &[("FAKE_RUNNING", "1")],
            answers: &[
                (LOGIN, "2"),
                (CONNECT, "y"),
                (TOKEN_PROMPT, TOKEN),
                (USER_ID, ""),
                (ANYONE, END_OF_INPUT),
            ],
            login: false,
            connect: None,
            restart: false,
            sign_in_todo: true,
            discord_todo: true,
        },
    ];
    for case in cases {
        // Given
        let setup = Setup::new()?;
        let mut session = Interactive::start(&setup, case.environment)?;

        // When
        for (question, reply) in case.answers {
            session.answer(question, reply)?;
        }
        let (status, output, session) = session.finish()?;

        // Then
        let name = case.name;
        assert!(status.success(), "{name}: {output}");
        let calls = setup.calls()?;
        let login_call = "compose run --rm -T shacs-cli provider codex login --headless";
        assert_eq!(
            calls.iter().any(|call| call == login_call),
            case.login,
            "{name}: {calls:?}"
        );
        let connect_calls: Vec<&String> = calls
            .iter()
            .filter(|call| call.contains("--token-stdin"))
            .collect();
        match case.connect {
            Some(expected) => {
                assert_eq!(connect_calls, [expected], "{name}");
                assert_eq!(
                    fs::read_to_string(setup.log("stdin"))?,
                    format!("{TOKEN}\n")
                );
                assert!(
                    !fs::read_to_string(setup.log("env"))?.contains(TOKEN),
                    "{name}"
                );
            }
            None => assert!(connect_calls.is_empty(), "{name}: {calls:?}"),
        }
        assert!(!session.shown()?.contains(TOKEN), "{name}");
        assert_eq!(
            output.contains(SIGN_IN_TODO),
            case.sign_in_todo,
            "{name}: {output}"
        );
        assert_eq!(
            output.contains(DISCORD_TODO),
            case.discord_todo,
            "{name}: {output}"
        );
        assert!(
            calls.contains(&"compose up -d shacs-gateway".to_owned()),
            "{name}"
        );
        let restarted = calls
            .iter()
            .any(|call| call == "compose restart shacs-gateway");
        assert_eq!(restarted, case.restart, "{name}: {calls:?}");
        assert!(!calls.iter().any(|call| call.contains(TOKEN)), "{name}");
        assert!(!output.contains(TOKEN), "{name}");
        assert!(session.echo()?, "{name}");
    }
    Ok(())
}

const FAKE_STTY: &str = r#"#!/bin/sh
if [ "$1" = "$FAKE_STTY_FAIL" ] && [ ! -e "$FAKE_DOCKER_DIR/stty-failed" ]; then
  : > "$FAKE_DOCKER_DIR/stty-failed"
  exit 1
fi
exec "$REAL_STTY" "$@"
"#;

#[test]
fn a_failing_stty_drops_the_token_and_puts_echo_back() -> TestResult {
    let cases: [(&str, &[(&str, &str)]); 2] = [
        ("-echo", &[(LOGIN, "2"), (CONNECT, "y")]),
        (
            "echo",
            &[(LOGIN, "2"), (CONNECT, "y"), (TOKEN_PROMPT, TOKEN)],
        ),
    ];
    for (failing, answers) in cases {
        // Given
        let setup = Setup::new()?;
        setup.write_tool("stty", FAKE_STTY)?;
        let real_stty = setup.root.path().join("tools/stty");
        let mut session = Interactive::start(
            &setup,
            &[
                ("FAKE_RUNNING", "1"),
                ("FAKE_STTY_FAIL", failing),
                ("REAL_STTY", real_stty.to_str().ok_or("tool path")?),
            ],
        )?;

        // When
        for (question, reply) in answers {
            session.answer(question, reply)?;
        }
        let (status, output, session) = session.finish()?;

        // Then
        assert!(status.success(), "{failing}: {output}");
        assert!(setup.log("stty-failed").exists(), "{failing}");
        assert_eq!(output.contains(TOKEN_PROMPT), failing == "echo", "{output}");
        assert!(!output.contains(USER_ID), "{failing}: {output}");
        assert!(output.contains(DISCORD_TODO), "{failing}: {output}");
        let calls = setup.calls()?;
        assert!(
            !calls.iter().any(|call| call.contains("--token-stdin")),
            "{failing}: {calls:?}"
        );
        assert!(
            calls.contains(&"compose up -d shacs-gateway".to_owned()),
            "{failing}: {calls:?}"
        );
        assert!(!output.contains(TOKEN), "{failing}");
        assert!(!session.shown()?.contains(TOKEN), "{failing}");
        assert!(session.echo()?, "{failing}");
    }
    Ok(())
}

#[test]
fn a_signal_during_the_hidden_token_puts_echo_back() -> TestResult {
    for (signal, code) in [(libc::SIGINT, 130), (libc::SIGTERM, 143)] {
        // Given
        let setup = Setup::new()?;
        let mut session = Interactive::start(&setup, &[])?;
        session.answer(LOGIN, "2")?;
        session.answer(CONNECT, "y")?;
        session.expect(TOKEN_PROMPT)?;

        // When
        let pid = libc::pid_t::try_from(session.child.id())?;
        // SAFETY: sending a signal to the script this test started.
        if unsafe { libc::kill(pid, signal) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let (status, output, session) = session.finish()?;

        // Then
        assert_eq!(status.code(), Some(code), "{signal}: {output}");
        assert!(session.echo()?, "{signal}");
        assert!(status.signal().is_none(), "{signal}");
    }
    Ok(())
}
