//! Spec 052 PRD 010 acceptance 1–4, 6 and 7 through the binary: `onboard`
//! stops during an interrupted config migration, `channels connect discord`
//! puts the token only in `.env.json`, stops without changes where it must,
//! hides terminal input and puts echo back, and `--status` follows the
//! runtime's Discord worker condition.

use serde_json::{json, Value};
use shacs_cli::{load_runtime_config_with_env, RuntimeConfigOptions};
use shacs_config::{config_migration_marker_path, secrets_migration_marker_path, EnvSource};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

type TestResult = Result<(), Box<dyn Error>>;
type FileBytes = BTreeMap<PathBuf, Vec<u8>>;
type Environment = &'static [(&'static str, &'static str)];

const NAME: &str = "SHACS_CHANNELS_DISCORD_TOKEN";
const TOKEN: &str = "spec052-discord-canary-4471";
const OTHER_TOKEN: &str = "spec052-discord-other-8820";
const WAIT: Duration = Duration::from_secs(60);

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Result<Self, Box<dyn Error>> {
        let home = Self {
            root: tempfile::tempdir()?,
        };
        fs::create_dir_all(home.data())?;
        fs::create_dir_all(home.workspace())?;
        Ok(home)
    }

    /// A home whose config is `config` with the workspace filled in.
    fn with_config(mut config: Value) -> Result<Self, Box<dyn Error>> {
        let home = Self::new()?;
        config["agents"] = json!({"defaults": {"workspace": home.workspace()}});
        home.write(&home.config(), &config)?;
        Ok(home)
    }

    fn data(&self) -> PathBuf {
        self.root.path().join("data")
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("workspace")
    }

    fn config(&self) -> PathBuf {
        self.data().join("config.json")
    }

    fn env_file(&self) -> PathBuf {
        self.data().join(".env.json")
    }

    fn write(&self, path: &Path, value: &Value) -> TestResult {
        fs::write(path, serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }

    fn read(&self, path: &Path) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn command(&self, args: &[&str], environment: &[(&str, &str)]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
        command
            .env_clear()
            .env("HOME", self.root.path())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .arg("--config")
            .arg(self.config())
            .args(args);
        for (name, value) in environment {
            command.env(name, value);
        }
        command
    }

    /// Runs `channels connect discord` with `args` and `stdin` piped in.
    fn connect(
        &self,
        args: &[&str],
        stdin: &str,
        environment: &[(&str, &str)],
    ) -> Result<Output, Box<dyn Error>> {
        let mut all = vec!["channels", "connect", "discord"];
        all.extend_from_slice(args);
        let mut child = self
            .command(&all, environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("stdin")?
            .write_all(stdin.as_bytes())?;
        Ok(child.wait_with_output()?)
    }

    fn status(&self, environment: &[(&str, &str)]) -> Result<Output, Box<dyn Error>> {
        Ok(self
            .command(&["channels", "connect", "discord", "--status"], environment)
            .stdin(Stdio::null())
            .output()?)
    }

    /// Every file under the data folder and the workspace, with its bytes.
    fn files(&self) -> Result<FileBytes, Box<dyn Error>> {
        let mut files = FileBytes::new();
        let mut folders = vec![self.data(), self.workspace()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(&folder)? {
                let path = entry?.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    let bytes = fs::read(&path)?;
                    files.insert(path, bytes);
                }
            }
        }
        Ok(files)
    }

    /// The token the runtime's config read resolves for Discord.
    fn runtime_token(&self) -> Result<Option<String>, Box<dyn Error>> {
        let bundle = load_runtime_config_with_env(
            RuntimeConfigOptions {
                config_path: Some(self.config()),
                workspace_override: None,
                resolve_env: true,
            },
            &NoEnvironment,
        )?;
        Ok(bundle
            .config
            .channels
            .plugins
            .get("discord")
            .and_then(|discord| discord.get("token"))
            .and_then(Value::as_str)
            .map(str::to_owned))
    }
}

struct NoEnvironment;

impl EnvSource for NoEnvironment {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_no_token(output: &Output) {
    let all = text(output);
    assert!(!all.contains(TOKEN) && !all.contains(OTHER_TOKEN), "{all}");
}

fn placeholder() -> String {
    format!("${{{NAME}}}")
}

#[test]
fn onboard_stops_while_a_config_migration_is_interrupted() -> TestResult {
    for marker in [config_migration_marker_path, secrets_migration_marker_path] {
        for wizard in [false, true] {
            // Given
            let home = Home::with_config(json!({}))?;
            home.write(&home.env_file(), &json!({"OTHER": "value"}))?;
            fs::write(home.data().join("auth.json"), "{}")?;
            fs::write(marker(&home.config()), "{}")?;
            let before = home.files()?;

            // When
            let mut args = vec!["onboard"];
            if wizard {
                args.push("--wizard");
            }
            let output = home.command(&args, &[]).stdin(Stdio::null()).output()?;

            // Then
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{}", text(&output));
            assert!(
                stderr.contains("config migration is interrupted"),
                "{stderr}"
            );
            assert!(
                stderr.contains("runtime config-migrate --recover"),
                "{stderr}"
            );
            assert_eq!(home.files()?, before);
        }
    }
    Ok(())
}

#[test]
fn connecting_puts_the_token_in_env_file_and_only_its_name_in_config() -> TestResult {
    // Given
    let home = Home::with_config(json!({
        "channels": {"discord": {"enabled": false, "channelIds": ["c1"]}},
        "tools": {"exec": {"timeout": 30}}
    }))?;
    home.write(&home.env_file(), &json!({"OTHER": "value"}))?;
    let other_config_before = home.read(&home.config())?["tools"].clone();

    // When
    let first = home.connect(
        &[
            "--token-stdin",
            "--allow-from",
            "111",
            "--allow-from",
            "222",
        ],
        &format!("{TOKEN}\n"),
        &[],
    )?;
    let after_first = home.files()?;
    let second = home.connect(
        &[
            "--token-stdin",
            "--allow-from",
            "111",
            "--allow-from",
            "222",
        ],
        &format!("{TOKEN}\n"),
        &[],
    )?;
    let status = home.status(&[])?;

    // Then
    assert!(first.status.success(), "{}", text(&first));
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        home.read(&home.env_file())?,
        json!({"OTHER": "value", NAME: TOKEN})
    );
    let mode = fs::metadata(home.env_file())?.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let config = home.read(&home.config())?;
    assert_eq!(
        config["channels"]["discord"],
        json!({"enabled": true, "channelIds": ["c1"], "token": placeholder(), "allowFrom": ["111", "222"]})
    );
    assert_eq!(config["tools"], other_config_before);
    assert!(String::from_utf8_lossy(&status.stdout).starts_with("discord: connected"));
    assert_eq!(home.files()?, after_first);
    assert!(String::from_utf8_lossy(&second.stdout).contains("unchanged"));
    for output in [&first, &second, &status] {
        assert_no_token(output);
    }
    assert!(!text(&first).contains("warning:"));
    assert_eq!(home.runtime_token()?.as_deref(), Some(TOKEN));
    Ok(())
}

#[test]
fn connecting_without_a_config_or_a_sender_list_creates_both_files_and_warns() -> TestResult {
    // Given
    let home = Home::new()?;

    // When
    let output = home.connect(&["--token-stdin"], TOKEN, &[])?;

    // Then
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(
        home.read(&home.config())?,
        json!({"channels": {"discord": {"enabled": true, "token": placeholder()}}})
    );
    assert_eq!(home.read(&home.env_file())?, json!({NAME: TOKEN}));
    assert!(String::from_utf8_lossy(&output.stdout).contains("warning:"));
    assert_eq!(home.runtime_token()?.as_deref(), Some(TOKEN));
    assert_no_token(&output);
    Ok(())
}

#[test]
fn connecting_stops_without_changes_where_it_must() -> TestResult {
    let discord = |fields: Value| json!({"channels": {"discord": fields}});
    let cases: Vec<(&str, Value, Environment)> = vec![
        (
            "literal token",
            discord(json!({"token": "typed-in-token"})),
            &[],
        ),
        (
            "another name",
            discord(json!({"token": "${OTHER_NAME}"})),
            &[],
        ),
        (
            "padded name",
            discord(json!({"token": format!(" {} ", placeholder())})),
            &[],
        ),
        (
            "botToken value",
            discord(json!({"botToken": "bot-token"})),
            &[],
        ),
        (
            "bot_token value",
            discord(json!({"bot_token": "bot-token"})),
            &[],
        ),
        ("environment differs", json!({}), &[(NAME, OTHER_TOKEN)]),
        ("031 marker", json!({}), &[]),
        ("048 marker", json!({}), &[]),
    ];
    for (name, config, environment) in cases {
        // Given
        let home = Home::with_config(config)?;
        home.write(&home.env_file(), &json!({"OTHER": "value"}))?;
        if name == "031 marker" {
            fs::write(config_migration_marker_path(&home.config()), "{}")?;
        }
        if name == "048 marker" {
            fs::write(secrets_migration_marker_path(&home.config()), "{}")?;
        }
        let before = home.files()?;

        // When
        let output = home.connect(&["--token-stdin", "--replace"], TOKEN, environment)?;

        // Then
        assert_eq!(output.status.code(), Some(1), "{name}: {}", text(&output));
        assert_eq!(home.files()?, before, "{name}");
        assert_no_token(&output);
    }
    Ok(())
}

#[test]
fn a_different_saved_token_needs_replace_and_then_only_it_changes() -> TestResult {
    // Given
    let home = Home::with_config(json!({}))?;
    home.write(
        &home.env_file(),
        &json!({"OTHER": "value", NAME: OTHER_TOKEN}),
    )?;
    let before = home.files()?;

    // When
    let kept = home.connect(&["--token-stdin"], TOKEN, &[])?;
    let kept_files = home.files()?;
    let replaced = home.connect(&["--token-stdin", "--replace"], TOKEN, &[])?;

    // Then
    assert_eq!(kept.status.code(), Some(1), "{}", text(&kept));
    assert_eq!(kept_files, before);
    assert!(replaced.status.success(), "{}", text(&replaced));
    assert_eq!(
        home.read(&home.env_file())?,
        json!({"OTHER": "value", NAME: TOKEN})
    );
    assert_eq!(
        home.read(&home.config())?["channels"]["discord"]["token"],
        json!(placeholder())
    );
    for output in [&kept, &replaced] {
        assert_no_token(output);
    }
    Ok(())
}

#[test]
fn a_token_saved_before_config_changed_still_reads_and_reconnects() -> TestResult {
    // Given: the state after .env.json was written and config was not.
    let home = Home::with_config(json!({}))?;
    home.write(&home.env_file(), &json!({NAME: TOKEN}))?;

    // When
    let token_before = home.runtime_token()?;
    let status_before = home.status(&[])?;
    let reconnect = home.connect(&["--token-stdin"], TOKEN, &[])?;
    let status_after = home.status(&[])?;

    // Then
    assert_eq!(token_before, None);
    assert!(String::from_utf8_lossy(&status_before.stdout).starts_with("discord: not connected"));
    assert!(reconnect.status.success(), "{}", text(&reconnect));
    assert!(String::from_utf8_lossy(&status_after.stdout).starts_with("discord: connected"));
    Ok(())
}

#[test]
fn status_follows_the_runtime_discord_condition() -> TestResult {
    struct StatusCase {
        name: &'static str,
        discord: Value,
        env_file: Value,
        environment: Environment,
        expected: &'static str,
    }
    let cases = [
        StatusCase {
            name: "disabled",
            discord: json!({"enabled": false, "token": placeholder()}),
            env_file: json!({NAME: TOKEN}),
            environment: &[],
            expected: "discord: not connected\nreason: channels.discord.enabled",
        },
        StatusCase {
            name: "empty token",
            discord: json!({"enabled": true, "token": ""}),
            env_file: json!({}),
            environment: &[],
            expected: "discord: not connected\nreason: channels.discord has no token",
        },
        StatusCase {
            name: "name nowhere",
            discord: json!({"enabled": true, "token": "${SPEC052_NOWHERE}"}),
            env_file: json!({}),
            environment: &[],
            expected: "discord: not connected\nreason: channels.discord.token: missing environment variable: SPEC052_NOWHERE",
        },
        StatusCase {
            name: "name in .env.json",
            discord: json!({"enabled": true, "token": placeholder()}),
            env_file: json!({NAME: TOKEN}),
            environment: &[],
            expected: "discord: connected\ntoken: channels.discord.token",
        },
        StatusCase {
            name: "environment wins",
            discord: json!({"enabled": true, "token": placeholder()}),
            env_file: json!({}),
            environment: &[(NAME, TOKEN)],
            expected: "discord: connected\ntoken: channels.discord.token",
        },
        StatusCase {
            name: "botToken before an empty token",
            discord: json!({"enabled": true, "botToken": "${BOT_NAME}", "token": ""}),
            env_file: json!({"BOT_NAME": TOKEN}),
            environment: &[],
            expected: "discord: connected\ntoken: channels.discord.botToken",
        },
        StatusCase {
            name: "empty botToken before a token",
            discord: json!({"enabled": true, "botToken": "${BOT_NAME}", "token": placeholder()}),
            env_file: json!({"BOT_NAME": "", NAME: TOKEN}),
            environment: &[],
            expected: "discord: connected\ntoken: channels.discord.token",
        },
    ];
    for case in cases {
        // Given
        let name = case.name;
        let home = Home::with_config(json!({"channels": {"discord": case.discord}}))?;
        home.write(&home.env_file(), &case.env_file)?;
        let before = home.files()?;

        // When
        let output = home.status(case.environment)?;
        fs::write(secrets_migration_marker_path(&home.config()), "{}")?;
        let with_marker = home.status(case.environment)?;

        // Then
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{name}: {}", text(&output));
        assert!(stdout.starts_with(case.expected), "{name}: {stdout}");
        assert_eq!(with_marker.stdout, output.stdout, "{name}");
        fs::remove_file(secrets_migration_marker_path(&home.config()))?;
        assert_eq!(home.files()?, before, "{name}");
        assert_no_token(&output);
    }

    let broken = Home::with_config(json!({}))?;
    fs::write(broken.env_file(), "[1]")?;
    assert_eq!(broken.status(&[])?.status.code(), Some(1));
    Ok(())
}

/// A terminal whose slave end the child reads as standard input.
struct Terminal {
    master: OwnedFd,
    slave: OwnedFd,
}

impl Terminal {
    fn open() -> Result<Self, Box<dyn Error>> {
        let mut master = -1;
        let mut slave = -1;
        let settings: *mut libc::termios = std::ptr::null_mut();
        let size: *mut libc::winsize = std::ptr::null_mut();
        // SAFETY: `openpty` writes two descriptors; null name, settings and
        // size pointers are allowed.
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
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        // SAFETY: setting a status flag on an open descriptor.
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
        Ok(Self { master, slave })
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

    fn type_text(&self, text: &str) -> TestResult {
        let mut master = fs::File::from(self.master.try_clone()?);
        master.write_all(text.as_bytes())?;
        Ok(())
    }

    /// What the terminal showed, without waiting for more.
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

/// Starts `channels connect discord --token-stdin` on the terminal and waits
/// for its prompt.
fn start_on_terminal(home: &Home, terminal: &Terminal) -> Result<Child, Box<dyn Error>> {
    let mut child = home
        .command(&["channels", "connect", "discord", "--token-stdin"], &[])
        .stdin(Stdio::from(terminal.slave.try_clone()?))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stderr = child.stderr.take().ok_or("stderr")?;
    let (sender, prompt) = mpsc::channel();
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        let mut byte = [0_u8; 1];
        while let Ok(1) = stderr.read(&mut byte) {
            seen.push(byte[0]);
            if seen.ends_with(b"Discord bot token: ") {
                let _ = sender.send(());
            }
        }
    });
    prompt
        .recv_timeout(WAIT)
        .map_err(|_| "no token prompt on standard error")?;
    Ok(child)
}

#[test]
fn terminal_input_is_hidden_and_echo_comes_back() -> TestResult {
    // Given
    let home = Home::with_config(json!({}))?;
    let terminal = Terminal::open()?;

    // When
    let mut child = start_on_terminal(&home, &terminal)?;
    terminal.type_text(&format!("{TOKEN}\n"))?;
    let status = child.wait()?;

    // Then
    assert!(status.success(), "exited {status}");
    assert!(!terminal.shown()?.contains(TOKEN));
    assert!(terminal.echo()?);
    assert_eq!(home.read(&home.env_file())?, json!({NAME: TOKEN}));
    Ok(())
}

#[test]
fn echo_comes_back_after_an_empty_token_and_after_signals() -> TestResult {
    for ending in ["empty", "SIGINT", "SIGTERM"] {
        // Given
        let home = Home::with_config(json!({}))?;
        let terminal = Terminal::open()?;

        // When
        let mut child = start_on_terminal(&home, &terminal)?;
        let signal = match ending {
            "SIGINT" => Some(libc::SIGINT),
            "SIGTERM" => Some(libc::SIGTERM),
            _ => None,
        };
        match signal {
            Some(signal) => {
                let pid = libc::pid_t::try_from(child.id())?;
                // SAFETY: sending a signal to the child this test started.
                unsafe { libc::kill(pid, signal) };
            }
            None => terminal.type_text("\n")?,
        }
        let status = child.wait()?;

        // Then
        match signal {
            Some(signal) => assert_eq!(status.signal(), Some(signal), "{ending}"),
            None => assert_eq!(status.code(), Some(1), "{ending}"),
        }
        assert!(terminal.echo()?, "{ending}");
        assert!(!home.env_file().exists(), "{ending}");
    }
    Ok(())
}
