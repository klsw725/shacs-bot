use serde_json::json;
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{
    save_config_to_path, AuthStore, Config, ConfigBundle, ConfigContext, LocalAuthStore,
    ProviderAuth, RawCredential,
};
use shacs_projection::{CredentialSource, ProcessAdapterKind, ProcessTerminalOutcome};
use shacs_providers::{GenerationSettings, ProviderRequest};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn production_agent_loop_turn_runtime_override_wins_over_all_fallbacks(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let auth_path = root.path().join("auth.json");
    let mut auth = AuthStore::default();
    auth.providers.insert(
        "openai".to_owned(),
        ProviderAuth::api_key("local-fallback-distinct"),
    );
    LocalAuthStore::new(&auth_path).save(&auth)?;
    let (api_base, capture) = serve_chat_response()?;
    let config: Config = serde_json::from_value(json!({
        "agents": {"defaults": {"provider": "openai", "model": "gpt-4o"}},
        "providers": {"openai": {
            "apiKey": "literal-fallback-distinct",
            "apiBase": api_base,
            "credentialSource": {
                "schemaVersion": 1,
                "command": "printf command-fallback-distinct"
            }
        }}
    }))?;
    let adapter =
        AgentLoopChatCompletionAdapter::from_bundle(bundle(config, root.path(), workspace), false)?;

    adapter.complete_chat_with_provider_runtime_override(
        invocation("hello", "api:override"),
        RawCredential::api_key("turn-override-distinct"),
    )?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;
    let credential = adapter.trusted_runtime_projection().credential().clone();

    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer turn-override-distinct"));
    for fallback in ["local-fallback", "command-fallback", "literal-fallback"] {
        assert!(!captured.contains(fallback));
    }
    assert_eq!(credential.source, Some(CredentialSource::RuntimeOverride));
    Ok(())
}

#[test]
#[cfg(unix)]
fn production_agent_loop_stop_aborts_credential_command_and_descendant(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let started = root.path().join("credential.started");
    let child_pid = root.path().join("credential-child.pid");
    let command = format!(
        "trap '' TERM; sleep 30 & child=$!; printf %s \"$child\" > {}; printf started > {}; wait \"$child\"",
        shell_path(&child_pid),
        shell_path(&started),
    );
    let config: Config = serde_json::from_value(json!({
        "agents": {"defaults": {"provider": "openai", "model": "gpt-4o"}},
        "providers": {"openai": {
            "apiKey": "literal-must-not-run",
            "apiBase": "http://127.0.0.1:1/v1",
            "credentialSource": {
                "schemaVersion": 1,
                "localAuth": false,
                "command": command
            }
        }}
    }))?;
    let adapter = Arc::new(AgentLoopChatCompletionAdapter::from_bundle(
        bundle(config, root.path(), workspace),
        false,
    )?);
    let worker = {
        let adapter = Arc::clone(&adapter);
        thread::spawn(move || adapter.complete_chat(invocation("hello", "cli:repl-stop")))
    };
    wait_for_file(&started)?;

    adapter.complete_chat(invocation("/stop", "cli:repl-stop"))?;
    let _ = worker.join().map_err(|_| "provider worker panicked")?;
    let projection = adapter.trusted_runtime_projection();
    let process = projection
        .process_adapters()
        .iter()
        .find(|process| process.adapter == ProcessAdapterKind::CredentialCommand)
        .ok_or("credential process fact missing")?;
    let pid = std::fs::read_to_string(child_pid)?.parse::<u32>()?;

    assert!(process.capabilities.abort);
    assert!(process.capabilities.descendant_cleanup);
    assert_eq!(
        process.recent_outcomes[0].outcome,
        ProcessTerminalOutcome::Aborted
    );
    assert!(!process_is_alive(pid));
    Ok(())
}

#[test]
#[cfg(unix)]
fn repl_surface_stop_aborts_long_running_credential_command() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let started = root.path().join("repl-credential.started");
    let child_pid = root.path().join("repl-credential-child.pid");
    let command = format!(
        "trap '' TERM; sleep 30 & child=$!; printf %s \"$child\" > {}; printf started > {}; wait \"$child\"",
        shell_path(&child_pid),
        shell_path(&started),
    );
    let config: Config = serde_json::from_value(json!({
        "agents": {"defaults": {
            "provider": "openai",
            "model": "gpt-4o",
            "workspace": workspace
        }},
        "providers": {"openai": {
            "apiKey": "literal-must-not-run",
            "apiBase": "http://127.0.0.1:1/v1",
            "credentialSource": {
                "schemaVersion": 1,
                "localAuth": false,
                "command": command
            }
        }}
    }))?;
    let config_path = root.path().join("config.json");
    save_config_to_path(&config, &config_path)?;
    let mut repl = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args(["agent", "--config"])
        .arg(&config_path)
        .args(["--session", "repl-stop"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = repl.stdin.take().ok_or("REPL stdin missing")?;
    input.write_all(b"hello\n")?;
    input.flush()?;
    wait_for_file(&started)?;

    let stop_started = Instant::now();
    input.write_all(b"/stop\n")?;
    drop(input);
    let output = repl.wait_with_output()?;
    let elapsed = stop_started.elapsed();
    let pid = std::fs::read_to_string(child_pid)?.parse::<u32>()?;

    assert!(output.status.success());
    assert!(elapsed < Duration::from_secs(5));
    assert!(!process_is_alive(pid));
    Ok(())
}

fn bundle(config: Config, root: &Path, workspace: std::path::PathBuf) -> ConfigBundle {
    ConfigBundle {
        config,
        context: ConfigContext {
            config_path: root.join("config.json"),
            data_dir: root.to_path_buf(),
            workspace,
        },
        migrations: Vec::new(),
    }
}

fn invocation(content: &str, session_key: &str) -> ChatCompletionInvocation {
    ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role": "user", "content": content})],
            tools: Vec::new(),
            model: "gpt-4o".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: Some("gpt-4o".to_owned()),
        session_key: session_key.to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    }
}

fn wait_for_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err("credential command did not start".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn shell_path(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\"'\"'"))
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

type Capture = thread::JoinHandle<Result<String, String>>;

fn serve_chat_response() -> Result<(String, Capture), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let request = read_request(&mut stream)?;
        let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"ok"}}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(request)
    });
    Ok((format!("http://{address}/v1"), handle))
}

fn read_request(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 512];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        bytes.extend_from_slice(&chunk[..read]);
        if read == 0 || bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}
