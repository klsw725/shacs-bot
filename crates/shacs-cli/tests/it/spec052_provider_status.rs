//! Spec 052 PRD 000 acceptance 4 and 6 through the binary: `provider status`
//! judges the runtime's credential place without writing or showing secrets,
//! and a gateway without a provider credential says so once it has started
//! and keeps running. PRD 040 acceptance 4: an empty provider variable no
//! longer hides an imported key, and importing again saves the current
//! declaration with the key.

use serde_json::{json, Value};
use shacs_config::{
    config_migration_marker_path, save_auth_store_to_path, secrets_migration_marker_path,
    AuthStore, CredentialFingerprint, ProviderAuth,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;
type FileBytes = BTreeMap<PathBuf, Vec<u8>>;

const CODEX_TOKEN: &str = "spec052-codex-canary-5521";
const ANTHROPIC_KEY: &str = "spec052-anthropic-canary-7731";
const ENV_FILE_VALUE: &str = "spec052-env-file-canary-9902";
const WAIT: Duration = Duration::from_secs(60);

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new(mut defaults: Value) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let home = Self { root };
        fs::create_dir_all(home.data())?;
        defaults["workspace"] = json!(home.root.path().join("workspace"));
        home.write_config(&json!({"agents": {"defaults": defaults}}))?;
        Ok(home)
    }

    fn data(&self) -> PathBuf {
        self.root.path().join("data")
    }

    fn config(&self) -> PathBuf {
        self.data().join("config.json")
    }

    fn write_config(&self, config: &Value) -> TestResult {
        fs::write(self.config(), serde_json::to_vec_pretty(config)?)?;
        Ok(())
    }

    fn save_auth(&self, entries: &[(&str, ProviderAuth)]) -> TestResult {
        let mut auth = AuthStore::default();
        for (provider_id, entry) in entries {
            auth.providers
                .insert((*provider_id).to_owned(), entry.clone());
        }
        save_auth_store_to_path(&auth, &self.data().join("auth.json"))?;
        Ok(())
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

    fn status(&self, environment: &[(&str, &str)]) -> Result<Output, Box<dyn Error>> {
        Ok(self
            .command(&["provider", "status"], environment)
            .output()?)
    }

    fn files(&self) -> Result<FileBytes, Box<dyn Error>> {
        let mut files = FileBytes::new();
        for entry in fs::read_dir(self.data())? {
            let path = entry?.path();
            if path.is_file() {
                let bytes = fs::read(&path)?;
                files.insert(path, bytes);
            }
        }
        Ok(files)
    }
}

fn oauth(access: &str) -> ProviderAuth {
    ProviderAuth {
        kind: "oauth".to_owned(),
        access: access.to_owned(),
        refresh: None,
        expires: None,
        account_id: None,
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn line_starting_with<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    stdout
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_default()
}

fn assert_no_secrets(output: &Output) {
    let all = text(output);
    for secret in [CODEX_TOKEN, ANTHROPIC_KEY, ENV_FILE_VALUE] {
        assert!(!all.contains(secret), "{all}");
    }
}

#[test]
fn status_reports_a_ready_codex_sign_in_and_writes_nothing() -> TestResult {
    // Given
    let home = Home::new(json!({"provider": "openai_codex", "model": "gpt-5.4"}))?;
    home.save_auth(&[("openai_codex", oauth(CODEX_TOKEN))])?;
    fs::write(
        home.data().join(".env.json"),
        serde_json::to_vec(&json!({"SHACS_UNUSED": ENV_FILE_VALUE}))?,
    )?;
    let before = home.files()?;

    // When
    let output = home.status(&[])?;

    // Then
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(stdout.lines().next(), Some("provider: ready openai_codex"));
    assert!(
        !line_starting_with(&stdout, "credential:").is_empty(),
        "{stdout}"
    );
    assert!(!stdout.contains("next:"), "{stdout}");
    assert_eq!(home.files()?, before);
    assert!(!home.data().join("auth.json.lock").exists());
    assert_no_secrets(&output);
    Ok(())
}

#[test]
fn status_reports_what_is_missing_and_what_to_do() -> TestResult {
    // Given
    let none = Home::new(json!({}))?;
    let unknown = Home::new(json!({"provider": "no-such-provider"}))?;

    // When
    let none_output = none.status(&[])?;
    let unknown_output = unknown.status(&[])?;

    // Then
    let none_stdout = String::from_utf8_lossy(&none_output.stdout);
    assert!(none_output.status.success(), "{}", text(&none_output));
    assert_eq!(none_stdout.lines().next(), Some("provider: missing none"));
    let reason = line_starting_with(&none_stdout, "reason:");
    assert!(reason.contains("no provider"), "{none_stdout}");
    assert!(!reason.contains("not a known provider"), "{none_stdout}");
    assert!(
        line_starting_with(&none_stdout, "next:").contains("shacs-bot provider codex login"),
        "{none_stdout}"
    );

    let unknown_stdout = String::from_utf8_lossy(&unknown_output.stdout);
    assert!(unknown_output.status.success(), "{}", text(&unknown_output));
    assert_eq!(
        unknown_stdout.lines().next(),
        Some("provider: missing none")
    );
    assert!(line_starting_with(&unknown_stdout, "reason:").contains("no-such-provider"));
    let unknown_next = line_starting_with(&unknown_stdout, "next:");
    assert!(
        unknown_next.contains("agents.defaults.provider"),
        "{unknown_stdout}"
    );
    assert!(!unknown_next.contains("login"), "{unknown_stdout}");
    Ok(())
}

fn import_anthropic_key(home: &Home, key: &str) -> TestResult {
    let mut child = home
        .command(
            &["provider", "anthropic", "import-key", "--token-stdin"],
            &[("ANTHROPIC_API_KEY", "")],
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("import-key stdin")?
        .write_all(format!("{key}\n").as_bytes())?;
    let output = child.wait_with_output()?;
    assert!(output.status.success(), "{}", text(&output));
    assert_no_secrets(&output);
    Ok(())
}

#[test]
fn an_empty_provider_variable_does_not_hide_an_imported_key() -> TestResult {
    // Given
    let home = Home::new(json!({}))?;
    import_anthropic_key(&home, ANTHROPIC_KEY)?;

    // When
    let output = home.status(&[("ANTHROPIC_API_KEY", "")])?;
    let gateway = started_gateway(&home, &[("ANTHROPIC_API_KEY", "")])?;
    let gateway_lines = gateway.stop(&home)?;

    // Then
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(stdout.lines().next(), Some("provider: ready anthropic"));
    assert_eq!(
        line_starting_with(&stdout, "credential:"),
        "credential: API key saved in the auth store"
    );
    assert_no_secrets(&output);
    assert!(
        !gateway_lines
            .iter()
            .any(|line| line.starts_with("provider:")),
        "{gateway_lines:?}"
    );
    Ok(())
}

#[test]
fn a_key_saved_under_another_declaration_is_used_again_once_imported_again() -> TestResult {
    // Given: a key saved before the declaration it was saved under changed.
    let home = Home::new(json!({}))?;
    let mut auth = AuthStore::default();
    auth.providers
        .insert("anthropic".to_owned(), ProviderAuth::api_key(ANTHROPIC_KEY));
    auth.set_fingerprint("anthropic", CredentialFingerprint::from_descriptor("old"));
    save_auth_store_to_path(&auth, &home.data().join("auth.json"))?;

    // When
    let stale = home.status(&[])?;
    import_anthropic_key(&home, ANTHROPIC_KEY)?;
    let imported = home.status(&[])?;
    let mut config: Value = serde_json::from_slice(&fs::read(home.config())?)?;
    config["providers"]["anthropic"]["credentialSource"] =
        json!({"schemaVersion": 1, "environment": "SHACS_SPEC052_OTHER_ANTHROPIC_KEY"});
    home.write_config(&config)?;
    let changed = home.status(&[])?;

    // Then
    let stale_stdout = String::from_utf8_lossy(&stale.stdout);
    assert_eq!(
        stale_stdout.lines().next(),
        Some("provider: missing anthropic")
    );
    let next = line_starting_with(&stale_stdout, "next:");
    assert!(
        next.contains("restore providers.anthropic.credentialSource")
            && next.contains("provider anthropic import-key --token-stdin"),
        "{stale_stdout}"
    );
    let imported_stdout = String::from_utf8_lossy(&imported.stdout);
    assert_eq!(
        imported_stdout.lines().next(),
        Some("provider: ready anthropic"),
        "{imported_stdout}"
    );
    let changed_stdout = String::from_utf8_lossy(&changed.stdout);
    assert_eq!(
        changed_stdout.lines().next(),
        Some("provider: missing anthropic"),
        "{changed_stdout}"
    );
    assert!(
        line_starting_with(&changed_stdout, "reason:")
            .contains("saved under a different credential setting"),
        "{changed_stdout}"
    );
    for output in [&stale, &imported, &changed] {
        assert_no_secrets(output);
    }
    Ok(())
}

#[test]
fn a_migration_marker_keeps_the_verdict_and_leaves_only_recovery_advice() -> TestResult {
    for marker in [config_migration_marker_path, secrets_migration_marker_path] {
        // Given
        let home = Home::new(json!({}))?;
        let without_marker = home.status(&[])?;
        fs::write(marker(&home.config()), "{}")?;

        // When
        let output = home.status(&[])?;

        // Then
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{}", text(&output));
        assert_eq!(
            stdout.lines().next(),
            String::from_utf8_lossy(&without_marker.stdout)
                .lines()
                .next()
        );
        let next = line_starting_with(&stdout, "next:");
        assert!(
            next.contains("runtime config-migrate --recover"),
            "{stdout}"
        );
        assert!(
            !next.contains("login") && !next.contains("import-key"),
            "{stdout}"
        );
    }
    Ok(())
}

#[test]
fn unreadable_config_env_file_or_auth_store_fail_the_status() -> TestResult {
    // Given
    let bad_config = Home::new(json!({}))?;
    fs::write(bad_config.config(), "{not json")?;
    let bad_env_file = Home::new(json!({}))?;
    fs::write(bad_env_file.data().join(".env.json"), "[1]")?;
    let bad_auth = Home::new(json!({}))?;
    fs::write(bad_auth.data().join("auth.json"), "{not json")?;

    // When
    let outputs = [
        bad_config.status(&[])?,
        bad_env_file.status(&[])?,
        bad_auth.status(&[])?,
    ];

    // Then
    for output in outputs {
        assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    }
    Ok(())
}

/// A gateway on a free port whose standard error arrives line by line.
struct Gateway {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Gateway {
    fn start(home: &Home, environment: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port().to_string();
        drop(listener);
        let mut child = home
            .command(&["run", "--websocket-port", &port], environment)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child.stderr.take().ok_or("gateway stderr")?;
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            lines,
            seen: Vec::new(),
        })
    }

    /// Waits for a line starting with `prefix` after the channel plan.
    fn wait_for(&mut self, prefix: &str) -> Result<String, Box<dyn Error>> {
        let deadline = Instant::now() + WAIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(remaining)
                .map_err(|_| format!("no `{prefix}` line: {:?}", self.seen))?;
            self.seen.push(line.clone());
            let after_plan = self.seen.iter().any(|seen| seen == "Channel runtime plan");
            if after_plan && line.starts_with(prefix) {
                return Ok(line);
            }
        }
    }

    fn running(&mut self) -> Result<bool, Box<dyn Error>> {
        Ok(self.child.try_wait()?.is_none())
    }

    /// Stops it with `runtime stop`, waits until its standard error closes
    /// and it exits, and returns all of its standard error.
    fn stop(mut self, home: &Home) -> Result<Vec<String>, Box<dyn Error>> {
        let stop = home.command(&["runtime", "stop"], &[]).output()?;
        assert!(stop.status.success(), "{}", text(&stop));
        let deadline = Instant::now() + WAIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(remaining) {
                Ok(line) => self.seen.push(line),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    self.child.kill()?;
                    return Err(format!("gateway did not stop: {:?}", self.seen).into());
                }
            }
        }
        let status = self.child.wait()?;
        assert!(status.success(), "gateway exited {status}: {:?}", self.seen);
        Ok(self.seen)
    }
}

fn started_gateway(home: &Home, environment: &[(&str, &str)]) -> Result<Gateway, Box<dyn Error>> {
    let mut gateway = Gateway::start(home, environment)?;
    gateway.wait_for("WebSocket:")?;
    Ok(gateway)
}

#[test]
fn a_gateway_without_a_provider_credential_says_so_and_keeps_running() -> TestResult {
    // Given
    let home = Home::new(json!({}))?;
    let mut gateway = Gateway::start(&home, &[])?;

    // When
    let hint = gateway.wait_for("provider:")?;
    let running = gateway.running()?;
    let lines = gateway.stop(&home)?;

    // Then
    assert!(hint.starts_with("provider: missing none ("), "{hint}");
    assert!(
        hint.contains("next: ") && hint.contains("shacs-bot provider codex login"),
        "{hint}"
    );
    assert!(running);
    let hints = lines
        .iter()
        .filter(|line| line.starts_with("provider:"))
        .count();
    assert_eq!(hints, 1, "{lines:?}");
    Ok(())
}

#[test]
fn a_gateway_with_a_provider_credential_adds_no_provider_line() -> TestResult {
    // Given
    let home = Home::new(json!({"provider": "custom", "model": "test-model"}))?;
    let mut config: Value = serde_json::from_slice(&fs::read(home.config())?)?;
    config["providers"] =
        json!({"custom": {"apiKey": "sk-fixture", "apiBase": "http://127.0.0.1:1/v1"}});
    home.write_config(&config)?;
    let gateway = started_gateway(&home, &[])?;

    // When
    let lines = gateway.stop(&home)?;

    // Then
    assert!(
        !lines.iter().any(|line| line.starts_with("provider:")),
        "{lines:?}"
    );
    Ok(())
}

#[test]
fn a_gateway_with_an_unreadable_auth_store_warns_and_keeps_running() -> TestResult {
    // Given
    let home = Home::new(json!({}))?;
    fs::write(home.data().join("auth.json"), "{not json")?;
    let mut gateway = Gateway::start(&home, &[])?;

    // When
    let hint = gateway.wait_for("provider:")?;
    let running = gateway.running()?;
    gateway.stop(&home)?;

    // Then
    assert!(hint.starts_with("provider: not checked"), "{hint}");
    assert!(running);
    Ok(())
}
