//! Spec 052 PRD 030 acceptance 4–7 through the binary: a command that writes
//! the data folder stops with the folder, its owner, the running user and a
//! fix when it cannot write there; read commands and writable folders run as
//! before.

use std::error::Error;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;

const WAIT: Duration = Duration::from_secs(60);
const NON_ROOT: u32 = 1001;

/// The checked commands run here. `provider codex login` and `memory model
/// install` are only in the unit list table: without the check they would
/// reach the network.
const CHECKED: [&[&str]; 29] = [
    &["onboard"],
    &["onboard", "--wizard"],
    &["run"],
    &["serve"],
    &["api", "serve"],
    &["gateway"],
    &["web"],
    &["ask", "hi"],
    &["agent"],
    &["runtime", "start"],
    &["runtime", "stop"],
    &["runtime", "restart"],
    &["runtime", "recover"],
    &["runtime", "update", "--target-version", "1.0.0"],
    &["runtime", "migrate", "--apply"],
    &["runtime", "migrate", "--resume"],
    &["runtime", "config-migrate", "--apply"],
    &["runtime", "config-migrate", "--recover"],
    &["provider", "codex", "import-token", "--token-stdin"],
    &["provider", "copilot", "import-token", "--token-stdin"],
    &["provider", "anthropic", "import-key", "--token-stdin"],
    &["channels", "connect", "discord", "--token-stdin"],
    &["plugins", "enable", "demo"],
    &["plugins", "disable", "demo"],
    &["permissions", "revoke", "abc"],
    &["permissions", "auto-mode", "reset"],
    &["permissions", "auto-mode", "setup"],
    &["memory", "index"],
    &[
        "tasks",
        "--json",
        "--owner",
        "recovery",
        "--action",
        "recover",
        "--locator",
        "runtime:recovery",
    ],
];

/// A temporary home. The binary runs as the test's user, or as 1001 from a
/// copy inside the folder when the test runs as root (root passes every
/// permission check).
struct Home {
    root: tempfile::TempDir,
    as_user: Option<u32>,
    binary: PathBuf,
}

impl Home {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("home"))?;
        // SAFETY: `geteuid` only reads this process's effective user id.
        let as_user = (unsafe { libc::geteuid() } == 0).then_some(NON_ROOT);
        let binary = match as_user {
            Some(user) => {
                let copy = root.path().join("shacs-bot");
                fs::copy(env!("CARGO_BIN_EXE_shacs-bot"), &copy)?;
                chown_tree(root.path(), user)?;
                copy
            }
            None => PathBuf::from(env!("CARGO_BIN_EXE_shacs-bot")),
        };
        Ok(Self {
            root,
            as_user,
            binary,
        })
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn data(&self) -> PathBuf {
        self.home().join(".shacs-bot")
    }

    /// The user and group the binary runs as.
    fn ids(&self) -> (u32, u32) {
        match self.as_user {
            Some(user) => (user, user),
            // SAFETY: `getuid` and `getgid` only read this process's ids.
            None => unsafe { (libc::getuid(), libc::getgid()) },
        }
    }

    fn make_dir(&self, path: &Path) -> TestResult {
        fs::create_dir_all(path)?;
        if let Some(user) = self.as_user {
            chown_tree(path, user)?;
        }
        Ok(())
    }

    fn lock(&self, path: &Path) -> TestResult {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555))?;
        Ok(())
    }

    fn run(&self, args: &[&str]) -> Result<Finished, Box<dyn Error>> {
        let mut command = Command::new(&self.binary);
        command
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", "/usr/bin:/bin")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(user) = self.as_user {
            command.uid(user).gid(user);
        }
        finish(command.spawn()?)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        unlock_tree(self.root.path());
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

/// Gives every folder back its write bit so the temporary folder can go.
fn unlock_tree(path: &Path) {
    let is_dir = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir());
    if !is_dir {
        return;
    }
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            unlock_tree(&entry.path());
        }
    }
}

struct Finished {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

/// Waits until both outputs close; a command still running after `WAIT`
/// (one the check let through) is killed and fails the test.
fn finish(mut child: Child) -> Result<Finished, Box<dyn Error>> {
    let (sender, receiver) = mpsc::channel();
    let pipes: [(bool, Box<dyn Read + Send>); 2] = [
        (false, Box::new(child.stdout.take().ok_or("stdout")?)),
        (true, Box::new(child.stderr.take().ok_or("stderr")?)),
    ];
    for (is_stderr, mut pipe) in pipes {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            let _ = sender.send((is_stderr, text));
        });
    }
    let deadline = Instant::now() + WAIT;
    let (mut stdout, mut stderr) = (String::new(), String::new());
    for _ in 0..2 {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((true, text)) => stderr = text,
            Ok((false, text)) => stdout = text,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("the command did not finish: {stderr}").into());
            }
        }
    }
    Ok(Finished {
        status: child.wait()?,
        stdout,
        stderr,
    })
}

#[test]
fn checked_commands_explain_an_unwritable_data_folder() -> TestResult {
    // Given
    let home = Home::new()?;
    home.make_dir(&home.data())?;
    home.lock(&home.data())?;
    let (uid, gid) = home.ids();
    let data = home.data().display().to_string();
    let first_line = format!(
        "cannot write to the data folder {data}: it belongs to {uid}:{gid} (mode 0555) and this \
         command runs as {uid}:{gid}"
    );
    let fix = format!("fix: chmod u+rwx {data}");

    for args in CHECKED {
        // When
        let finished = home.run(args)?;

        // Then
        let stderr = &finished.stderr;
        assert_eq!(finished.status.code(), Some(1), "{args:?}: {stderr}");
        assert_eq!(
            stderr.lines().next(),
            Some(first_line.as_str()),
            "{args:?}: {stderr}"
        );
        assert!(stderr.contains(&fix), "{args:?}: {stderr}");
        assert!(
            !stderr.contains("os error 13") && !finished.stdout.contains("os error 13"),
            "{args:?}: {stderr}"
        );
    }

    // Read commands are not checked and run as before.
    for args in [
        &["provider", "status"][..],
        &["channels", "connect", "discord", "--status"],
    ] {
        let finished = home.run(args)?;
        assert!(finished.status.success(), "{args:?}: {}", finished.stderr);
    }
    Ok(())
}

#[test]
fn another_folder_gets_no_command_that_changes_its_owner() -> TestResult {
    // Given
    let home = Home::new()?;
    let other = home.root.path().join("other");
    home.make_dir(&other)?;
    home.lock(&other)?;
    let (uid, gid) = home.ids();
    let config = other.join("config.json");

    // When
    let finished = home.run(&["onboard", "--config", &config.display().to_string()])?;

    // Then
    let stderr = &finished.stderr;
    assert_eq!(finished.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with(&format!(
            "cannot write to the data folder {}: it belongs to {uid}:{gid}",
            other.display()
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "fix: make {} writable for {uid}:{gid}, or use --config",
            other.display()
        )),
        "{stderr}"
    );
    assert!(
        !stderr.contains("chown") && !stderr.contains("sudo"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn a_missing_data_folder_under_an_unwritable_home_is_explained() -> TestResult {
    // Given
    let home = Home::new()?;
    home.lock(&home.home())?;
    let (uid, gid) = home.ids();
    let data = home.data().display().to_string();

    // When
    let finished = home.run(&["onboard"])?;

    // Then
    let stderr = &finished.stderr;
    assert_eq!(finished.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.lines().next(),
        Some(
            format!(
                "cannot create the data folder {data}: {} belongs to {uid}:{gid} (mode 0555) and \
                 this command runs as {uid}:{gid}",
                home.home().display()
            )
            .as_str()
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "fix: sudo mkdir -p {data} && sudo chown {uid}:{gid} {data} && chmod u+rwx {data}"
        )),
        "{stderr}"
    );
    assert!(!home.data().exists());
    Ok(())
}

#[test]
fn a_writable_data_folder_runs_as_before() -> TestResult {
    // Given
    let home = Home::new()?;

    // When
    let finished = home.run(&["onboard"])?;

    // Then
    assert!(finished.status.success(), "{}", finished.stderr);
    assert!(home.data().join("config.json").is_file());
    Ok(())
}
