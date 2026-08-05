use serde_json::json;
use shacs_config::{save_config_to_path, Config};
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SENTINEL: &str = "sk-spec031-cli-sentinel";
static PROVIDERLESS_SURFACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct ServeProcess {
    child: Child,
}

impl Drop for ServeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_config(
    root: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn std::error::Error>> {
    let config_path = root.join("config.json");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace)?;
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    config.agents.defaults.provider = "openai".to_owned();
    config.agents.defaults.model = "gpt-4o-mini".to_owned();
    config.providers.insert(
        "openai".to_owned(),
        shacs_config::ProviderConfig {
            api_key: Some(SENTINEL.to_owned()),
            api_key_ref: None,
            api_base: Some("https://example.invalid/v1".to_owned()),
            extra_headers: None,
            extra_body: None,
        },
    );
    config
        .channels
        .plugins
        .insert("telegram".to_owned(), json!({ "enabled": true }));
    save_config_to_path(&config, &config_path)?;
    Ok((config_path, workspace))
}

fn write_providerless_config(
    root: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn std::error::Error>> {
    let config_path = root.join("config.json");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace)?;
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    config.agents.defaults.provider = "openai".to_owned();
    config.agents.defaults.model = "gpt-4o-mini".to_owned();
    config.providers.insert(
        "openai".to_owned(),
        shacs_config::ProviderConfig {
            api_key: None,
            api_key_ref: None,
            api_base: Some("https://example.invalid/v1".to_owned()),
            extra_headers: None,
            extra_body: None,
        },
    );
    save_config_to_path(&config, &config_path)?;
    Ok((config_path, workspace))
}

fn run_cli(config_path: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(config_path)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "command failed {:?}\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn run_cli_with_stderr(
    config_path: &Path,
    args: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(config_path)
        .args(args)
        .output()?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?
    );
    if !output.status.success() {
        return Err(format!("command failed {args:?}\n{combined}").into());
    }
    Ok(combined)
}

fn parse_diagnostics_json(output: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let marker = "\nBundle: ";
    let json_text = output
        .split_once(marker)
        .map_or(output, |(json_text, _)| json_text);
    Ok(serde_json::from_str(json_text)?)
}

fn read_bundle_snapshot(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let output = Command::new("python3")
        .arg("-c")
        .arg(
            "import sys, zipfile; print(zipfile.ZipFile(sys.argv[1]).read('snapshot.json').decode())",
        )
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn reserve_port() -> Result<u16, Box<dyn std::error::Error>> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn start_serve(
    config_path: &Path,
    workspace: &Path,
    port: u16,
) -> Result<ServeProcess, Box<dyn std::error::Error>> {
    let port_arg = port.to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(config_path)
        .args([
            "serve",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
            "--port",
            &port_arg,
        ])
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if ureq::get(&format!("http://127.0.0.1:{port}/health"))
            .call()
            .is_ok()
        {
            return Ok(ServeProcess { child });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("serve did not become healthy".into())
}

fn readiness_from_api(port: u16) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut response = ureq::get(&format!("http://127.0.0.1:{port}/v1/readiness")).call()?;
    Ok(serde_json::from_str(
        &response.body_mut().read_to_string()?,
    )?)
}

fn diagnostics_from_api(port: u16) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut response = ureq::get(&format!("http://127.0.0.1:{port}/v1/diagnostics")).call()?;
    Ok(serde_json::from_str(
        &response.body_mut().read_to_string()?,
    )?)
}

fn run_repl(
    config_path: &Path,
    workspace: &Path,
    input: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(config_path)
        .args(["agent", "--workspace", workspace.to_string_lossy().as_ref()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?
    );
    if !output.status.success() {
        return Err(format!("repl failed\n{combined}").into());
    }
    Ok(combined)
}

fn write_migration_blocker(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = root
        .join("runtime")
        .join("migration-fixtures")
        .join("v0")
        .join("event.json");
    fs::create_dir_all(path.parent().ok_or("missing migration fixture parent")?)?;
    fs::write(path, br#"{"schema_version":2,"family":"event"}"#)?;
    Ok(())
}

fn assert_spec031_projection(output: &str, family: &str, state: &str, reason: &str) {
    assert!(output.contains(&format!("Spec031 {family}:")), "{output}");
    assert!(output.contains(&format!("state={state}")), "{output}");
    assert!(output.contains("severity="), "{output}");
    assert!(output.contains(&format!("reason={reason}")), "{output}");
    assert!(output.contains("lineage=subject:"), "{output}");
    assert!(!output.contains(SENTINEL), "{output}");
}

fn assert_no_plugin_management_raw_values(output: &str, raw_values: &[String]) {
    assert!(
        output.contains("path:"),
        "missing opaque path ref\n{output}"
    );
    for raw in raw_values {
        assert!(!output.contains(raw), "raw value leaked: {raw}\n{output}");
    }
}

#[test]
fn context_refs_parse_cli_does_not_render_raw_targets() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (config_path, _workspace) = write_config(root.path())?;
    let credential_url = "https://user:pass@example.invalid/context.txt";
    let posix_path = "/tmp/shacs-parse-secret.txt";
    let windows_path = r"C:\Users\alice\secret-token.txt";
    let prompt_sentinel = "sk-context-parse-prompt-token";
    let message = format!(
        "parse @{credential_url} @{posix_path} @{windows_path} {prompt_sentinel} {SENTINEL}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(&config_path)
        .args(["context", "refs", "parse", &message])
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    if !output.status.success() {
        return Err(
            format!("context refs parse failed\nstdout:\n{stdout}\nstderr:\n{stderr}").into(),
        );
    }
    let combined = format!("{stdout}\n{stderr}");

    assert!(stdout.contains("context refs parse"), "{stdout}");
    assert!(stdout.contains("kind=Url"), "{stdout}");
    assert!(stdout.contains("kind=File"), "{stdout}");
    assert!(stdout.contains("target=context-source:"), "{stdout}");
    for raw in [
        credential_url,
        posix_path,
        windows_path,
        prompt_sentinel,
        SENTINEL,
        "user:pass",
    ] {
        assert!(
            !combined.contains(raw),
            "raw value leaked: {raw}\n{combined}"
        );
    }
    Ok(())
}

#[test]
fn context_refs_resolve_cli_does_not_render_raw_sources() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = write_config(root.path())?;
    let workspace_secret = workspace.join("secret.txt");
    let outside_secret = root.path().join("outside-secret.txt");
    let credential_url = "https://user:pass@example.invalid/context.txt";
    fs::write(&workspace_secret, "OPENAI_API_KEY=sk-context-cli-secret")?;
    fs::write(&outside_secret, "outside secret")?;

    let output = run_cli(
        &config_path,
        &[
            "context",
            "refs",
            "resolve",
            "--message",
            &format!(
                "read @secret.txt @{} @url:{}",
                outside_secret.to_string_lossy(),
                credential_url
            ),
        ],
    )?;

    assert!(output.contains("context refs resolve"), "{output}");
    assert!(output.contains("source="), "{output}");
    for raw in [
        workspace.to_string_lossy().as_ref(),
        workspace_secret.to_string_lossy().as_ref(),
        outside_secret.to_string_lossy().as_ref(),
        credential_url,
        "sk-context-cli-secret",
        SENTINEL,
    ] {
        assert!(!output.contains(raw), "raw value leaked: {raw}\n{output}");
    }
    Ok(())
}

#[test]
fn plugin_and_hook_management_cli_uses_opaque_paths_and_never_executes_surfaces(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("unique-config-raw-path.json");
    let workspace = root.path().join("unique-workspace-raw-path");
    let plugin_dir = root.path().join("plugins").join("unique-plugin-dir");
    let manifest_path = plugin_dir.join("plugin.json");
    let sentinel = workspace.join("unique-sentinel-should-not-exist");
    let credential_url = "https://user:pass@example.invalid/plugin-description";
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&plugin_dir)?;
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    config.env.insert(
        "SPEC025_CONFIG_SECRET".to_owned(),
        "RAW_PLUGIN_CONFIG_SECRET".to_owned(),
    );
    save_config_to_path(&config, &config_path)?;
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "name": "unique-plugin",
            "version": "0.1.0",
            "description": credential_url,
            "requiresEnv": [],
            "requiresConfig": ["SPEC025_CONFIG_SECRET"],
            "surfaces": {"tools": ["unique_tool"], "hooks": ["tool:before"], "commands": ["unique"]},
            "entrypoints": {
                "tools": {"unique_tool": {"command": format!("touch {}", sentinel.display())}},
                "commands": {"unique": {"backend": format!("touch {}", sentinel.display())}}
            },
            "permissions": {},
            "assets": []
        }))?,
    )?;
    let raw_values = vec![
        config_path.to_string_lossy().to_string(),
        workspace.to_string_lossy().to_string(),
        root.path().to_string_lossy().to_string(),
        plugin_dir.to_string_lossy().to_string(),
        manifest_path.to_string_lossy().to_string(),
        sentinel.to_string_lossy().to_string(),
        "RAW_PLUGIN_CONFIG_SECRET".to_owned(),
        credential_url.to_owned(),
        "user:pass".to_owned(),
        "touch ".to_owned(),
    ];

    let enable = run_cli_with_stderr(
        &config_path,
        &[
            "plugins",
            "enable",
            "unique-plugin",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;
    let list = run_cli_with_stderr(
        &config_path,
        &[
            "plugins",
            "list",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;
    let inspect = run_cli_with_stderr(
        &config_path,
        &[
            "plugins",
            "inspect",
            "unique-plugin",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;
    let doctor = run_cli_with_stderr(
        &config_path,
        &[
            "plugins",
            "doctor",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;
    let hooks = run_cli_with_stderr(
        &config_path,
        &[
            "hooks",
            "list",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;
    let hook_inspect = run_cli_with_stderr(
        &config_path,
        &[
            "hooks",
            "inspect",
            "tool:before",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;

    for output in [enable, list, inspect, doctor, hooks, hook_inspect] {
        assert_no_plugin_management_raw_values(&output, &raw_values);
    }
    assert!(!sentinel.exists());
    Ok(())
}

#[test]
fn spec031_cli_projection_surfaces_render_canonical_envelopes(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = write_config(root.path())?;
    fs::create_dir_all(root.path().join("plugins").join("blocked"))?;
    fs::write(
        root.path()
            .join("plugins")
            .join("blocked")
            .join("plugin.json"),
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "name": "blocked-plugin",
            "version": "0.1.0",
            "requiresEnv": ["SPEC031_MISSING_ENV"]
        }))?,
    )?;

    let status = run_cli(&config_path, &["status"])?;
    assert_spec031_projection(&status, "session", "ready", "included");

    let inspect = run_cli(&config_path, &["runtime", "inspect"])?;
    assert_spec031_projection(&inspect, "diagnostics", "blocked", "blocked");
    assert!(!inspect.contains("Spec031 approval:"), "{inspect}");
    assert_spec031_projection(
        &inspect,
        "app",
        "unavailable",
        "missing_external_owner_evidence",
    );
    assert_spec031_projection(
        &inspect,
        "media",
        "unavailable",
        "missing_external_owner_evidence",
    );

    let diagnostics = run_cli(&config_path, &["runtime", "diagnostics"])?;
    assert_spec031_projection(&diagnostics, "diagnostics", "ready", "included");

    let sessions = run_cli(
        &config_path,
        &[
            "sessions",
            "list",
            "--workspace",
            &workspace.to_string_lossy(),
        ],
    )?;
    assert_spec031_projection(&sessions, "session", "unavailable", "missing");

    let _created = run_cli(
        &config_path,
        &[
            "sessions",
            "create",
            "--session",
            "spec031-session",
            "--workspace",
            &workspace.to_string_lossy(),
        ],
    )?;
    let sessions = run_cli(
        &config_path,
        &[
            "sessions",
            "list",
            "--workspace",
            &workspace.to_string_lossy(),
        ],
    )?;
    assert_spec031_projection(&sessions, "session", "ready", "included");

    let channels = run_cli(
        &config_path,
        &[
            "channels",
            "status",
            "--workspace",
            &workspace.to_string_lossy(),
        ],
    )?;
    assert_spec031_projection(&channels, "progress", "blocked", "blocked");
    assert!(
        channels.contains("missing-credentials") || channels.contains("unavailable"),
        "{channels}"
    );

    let plugins = run_cli(
        &config_path,
        &[
            "plugins",
            "doctor",
            "--workspace",
            &workspace.to_string_lossy(),
        ],
    )?;
    assert_spec031_projection(&plugins, "plugin", "ready", "included");
    Ok(())
}

#[test]
fn spec031_readiness_parity_uses_runtime_inspect_owner_source_for_api_cli_and_bundle(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = write_config(root.path())?;
    let bundle_path = root.path().join("readiness-diagnostics.zip");
    let port = reserve_port()?;
    let _serve = start_serve(&config_path, &workspace, port)?;
    write_migration_blocker(root.path())?;

    let diagnostics = run_cli(
        &config_path,
        &[
            "runtime",
            "diagnostics",
            "--bundle",
            bundle_path.to_string_lossy().as_ref(),
        ],
    )?;
    let cli_projection = parse_diagnostics_json(&diagnostics)?;
    let bundle_projection = read_bundle_snapshot(&bundle_path)?;
    let api_readiness = readiness_from_api(port)?;
    let api_diagnostics = diagnostics_from_api(port)?;

    let cli_readiness = &cli_projection["runtime"]["spec031_readiness"];
    assert_eq!(api_readiness, *cli_readiness);
    assert_eq!(
        bundle_projection["runtime"]["spec031_readiness"],
        *cli_readiness
    );
    assert_eq!(
        api_diagnostics["runtime"]["spec031_readiness"],
        *cli_readiness
    );
    assert_eq!(api_readiness["envelope"]["state"], "blocked");
    assert!(api_readiness["components"]
        .as_array()
        .is_some_and(|components| {
            components.iter().any(|component| {
                component["kind"] == "storage"
                    && component["state"] == "blocked"
                    && component["reason_code"] == "blocked"
            })
        }));

    let inspect = run_cli(&config_path, &["runtime", "inspect"])?;
    assert!(
        inspect.contains(
            "remediation=\"run the documented storage or migration recovery command later\""
        ),
        "{inspect}"
    );
    assert!(!inspect.contains("future:resolve_blocker"), "{inspect}");
    Ok(())
}

#[test]
fn providerless_control_surfaces_remain_reachable_with_degraded_readiness(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = PROVIDERLESS_SURFACE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "providerless surface lock poisoned")?;
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = write_providerless_config(root.path())?;
    let port = reserve_port()?;
    let _serve = start_serve(&config_path, &workspace, port)?;

    let status = run_cli(&config_path, &["status"])?;
    assert!(status.contains("Config:"), "{status}");
    assert!(!status.contains(SENTINEL), "{status}");

    let doctor = run_cli(
        &config_path,
        &[
            "plugins",
            "doctor",
            "--workspace",
            workspace.to_string_lossy().as_ref(),
        ],
    )?;
    assert!(doctor.contains("Plugin doctor"), "{doctor}");

    let mut health = ureq::get(&format!("http://127.0.0.1:{port}/health")).call()?;
    assert_eq!(health.body_mut().read_to_string()?, r#"{"status":"ok"}"#);

    let readiness = readiness_from_api(port)?;
    assert_ne!(readiness["envelope"]["state"], "ready");
    assert!(
        readiness["components"]
            .as_array()
            .is_some_and(|components| {
                components.iter().any(|component| {
                    component["kind"] == "provider_auth" && component["state"] != "ready"
                })
            }),
        "{readiness}"
    );
    Ok(())
}

#[test]
fn providerless_repl_priority_slash_command_does_not_construct_provider(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = PROVIDERLESS_SURFACE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "providerless surface lock poisoned")?;
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = write_providerless_config(root.path())?;

    let output = run_repl(&config_path, &workspace, "/status\n")?;

    assert!(output.contains("kind=command"), "{output}");
    assert!(output.contains("Command: Status"), "{output}");
    assert!(!output.contains("API key"), "{output}");
    Ok(())
}
