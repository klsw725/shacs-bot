use super::spec051_rendered_browser::{self, Rig};
use serde_json::Value;
use shacs_cli::{browser, parse_cli_args, run_command};
use shacs_session::{Session, SessionManager};
use std::error::Error;

#[cfg(feature = "spec051-test-profile")]
#[test]
fn conversation_login_reaches_production_gateway() -> Result<(), Box<dyn Error>> {
    use serde_json::json;
    use shacs_cli::spec051_gateway_fixture::Gateway;
    use shacs_core::runtime::{self, EntrustedWorkReadRequest};
    use shacs_providers::{LlmResponse, ToolCallRequest};
    use std::fs;
    use std::time::Duration;

    const NAME: &str = "spec051_web_watch_cli::conversation_login_reaches_production_gateway";
    if std::env::var("SHACS_SPEC051_CONVERSATION").as_deref() != Ok("1") {
        let root = tempfile::tempdir()?;
        let clock = root.path().join("clock");
        fs::write(&clock, "1800000000000")?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", NAME, "--nocapture"])
            .env("SHACS_SPEC051_CONVERSATION", "1")
            .env("SHACS_SPEC042_CLOCK_FILE", clock)
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let mut rig = Rig::new()?;
    rig.bundle.config.agents.defaults.unified_session = false;
    rig.bundle.config.channels.plugins.insert("email".to_owned(), json!({
        "enabled":true,"consentGranted":true,"smtp":{"host":"localhost","from":"watch@example.test"}
    }));
    fs::write(
        &rig.bundle.context.config_path,
        serde_json::to_vec(&rig.bundle.config)?,
    )?;
    browser::install_process_browser_fixture(rig.host.clone())?;
    let draft = json!({"url":"https://93.184.216.34/item","reading_mode":"browser",
        "use_saved_login":true,"value_label":"Price:","condition":"at_most",
        "condition_value":"11000","cadence_seconds":600,"notify_email":"owner@example.test"});
    let replies = vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "proposal",
                "propose_web_watch",
                draft.as_object().ok_or("draft")?.clone(),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("summary".to_owned()),
            ..LlmResponse::default()
        },
    ];
    let mut gateway = Gateway::start_with_replies(rig.bundle.clone(), replies)?;
    let summary = gateway.say("owner", "https://93.184.216.34/item Price: 11000 이하가 되면 owner@example.test로 알려줘. 브라우저로 로그인한 상태로 보기.");
    assert_eq!(summary.status, 200, "{:?}", summary.body);
    let text = summary.body.to_string();
    assert!(text.contains("로그인 사용: 예"), "{text}");
    assert!(text.contains("12900"), "{text}");
    rig.next_peer();
    let confirmation = gateway.say("owner", "응");
    assert_eq!(confirmation.status, 200, "{:?}", confirmation.body);
    let manager = SessionManager::new(&rig.bundle.context.workspace)?;
    let session = manager.load_existing("api:owner").ok_or("owner session")?;
    let work_id = session.metadata["web_watch"]["watches"][0]["work_id"]
        .as_str()
        .ok_or("confirmed watch")?
        .to_owned();
    let request = EntrustedWorkReadRequest {
        owner_session_id: "api:owner".to_owned(),
        requester_target: "api:owner".to_owned(),
        work_id,
    };
    let status = runtime::read_web_watch(
        &rig.bundle.context.workspace,
        &rig.bundle.context.data_dir,
        &request,
    )?;
    assert!(status.plan.ok_or("plan")?.use_saved_login);
    let calls = gateway.provider_calls();
    rig.next_peer();
    let (entered, release) = rig.block();
    fs::write(
        std::env::var_os("SHACS_SPEC042_CLOCK_FILE").ok_or("clock")?,
        "1800000600000",
    )?;
    entered.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(gateway.provider_calls(), calls);
    let response = gateway.external_takeover()?;
    assert!(
        response.contains("busy") || response.contains("쓰는 중"),
        "{response}"
    );
    release.send(())?;
    gateway.stop()?;
    let status = runtime::read_web_watch(
        &rig.bundle.context.workspace,
        &rig.bundle.context.data_dir,
        &request,
    )?;
    assert_eq!(status.history.len(), 1);
    assert_eq!(gateway.provider_calls(), calls);
    assert!(status.plan.ok_or("plan")?.use_saved_login);
    rig.cleaned();
    Ok(())
}

#[cfg(not(feature = "spec051-test-profile"))]
#[test]
fn spec051_test_profile_does_not_change_production_guard() -> Result<(), Box<dyn Error>> {
    use shacs_api::ChatCompletionAdapter;
    const NAME: &str =
        "spec051_web_watch_cli::spec051_test_profile_does_not_change_production_guard";
    if std::env::var("SHACS_SPEC051_GUARD_PROBE").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", NAME, "--nocapture"])
            .env("SHACS_SPEC051_GUARD_PROBE", "1")
            .env("SHACS_SPEC051_ALLOW_TEST_PAGE", "1")
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let rig = Rig::new()?;
    let adapter = shacs_cli::AgentLoopChatCompletionAdapter::from_bundle(rig.bundle.clone(), true)?;
    let guard = adapter.web_watch_network_guard();
    assert!(guard
        .validate_url_target("http://172.30.250.2:8000/rendered")
        .is_err());
    assert!(guard.validate_url_target("http://127.0.0.1/").is_err());
    Ok(())
}

#[test]
fn gateway_runs_the_confirmed_login_choice() -> Result<(), Box<dyn Error>> {
    if spec051_rendered_browser::isolated(
        "spec051_web_watch_cli::gateway_runs_the_confirmed_login_choice",
    )? {
        return Ok(());
    }
    spec051_rendered_browser::gateway_run(true)
}

#[test]
fn browser_preview_confirmation_and_status_use_the_selected_reader() -> Result<(), Box<dyn Error>> {
    if spec051_rendered_browser::isolated(
        "spec051_web_watch_cli::browser_preview_confirmation_and_status_use_the_selected_reader",
    )? {
        return Ok(());
    }
    cli_flow(false)
}

#[test]
fn saved_login_cli_requires_matching_preview() -> Result<(), Box<dyn Error>> {
    if spec051_rendered_browser::isolated(
        "spec051_web_watch_cli::saved_login_cli_requires_matching_preview",
    )? {
        return Ok(());
    }
    cli_flow(true)
}

fn cli_flow(saved: bool) -> Result<(), Box<dyn Error>> {
    let rig = Rig::new()?;
    browser::install_process_browser_fixture(rig.host.clone())?;
    let workspace = rig.bundle.context.workspace.to_str().ok_or("workspace")?;
    let config = rig.bundle.context.config_path.to_str().ok_or("config")?;
    let data = rig.bundle.context.data_dir.to_str().ok_or("data")?;
    SessionManager::new(&rig.bundle.context.workspace)?.save(&Session::new("cli:owner"))?;
    let run = |args: &[&str]| -> Result<String, Box<dyn Error>> {
        let mut line = vec!["--config", config, "work"];
        line.extend_from_slice(args);
        line.extend(["--workspace", workspace, "--session", "cli:owner"]);
        Ok(run_command(parse_cli_args(line)?)?)
    };
    let created: Value = serde_json::from_str(&run(&[
        "create",
        "watch a price",
        "--consent",
        "explicit-request",
        "--evidence-ref",
        "cli:user",
        "--instructions",
        "tell me",
    ])?)?;
    let id = created["work"]["work_id"].as_str().ok_or("work id")?;
    let url = "https://93.184.216.34/item?secret-query#private-fragment";
    let mut preview_args = vec![
        "web-preview",
        id,
        "--url",
        url,
        "--label",
        "Price:",
        "--reading-mode",
        "browser",
    ];
    if saved {
        preview_args.push("--use-saved-login");
    }
    let preview: Value = serde_json::from_str(&run(&preview_args)?)?;
    assert_eq!(preview["reading_mode"], "browser");
    assert_eq!(preview["use_saved_login"], saved);
    assert_eq!(preview["candidates"][0]["value"]["value"], "12900");
    assert_eq!(
        preview["candidates"][0]["extraction"]["version"],
        "rendered_text_v1"
    );
    let token = preview["candidates"][0]["token"].as_str().ok_or("token")?;
    let confirm = |mode| {
        let mut arguments = vec![
            "web-watch",
            id,
            "--url",
            url,
            "--candidate",
            token,
            "--at-most",
            "11000",
            "--cadence-seconds",
            "600",
            "--expected-revision",
            "1",
            "--reading-mode",
            mode,
        ];
        if saved {
            arguments.push("--use-saved-login");
        }
        run(&arguments)
    };
    assert!(confirm("fetch").is_err());
    rig.next_peer();
    let confirmed: Value = serde_json::from_str(&confirm("browser")?)?;
    assert_eq!(confirmed["condition_revision"], 2);
    let status: Value = serde_json::from_str(&run(&["web-status", id, "--data-dir", data])?)?;
    assert_eq!(status["plan"]["reading_mode"], "browser");
    assert_eq!(status["plan"]["use_saved_login"], saved);
    assert_eq!(status["baseline"]["value"]["value"], "12900");
    let output = status.to_string();
    assert!(!output.contains("secret-query") && !output.contains("private-fragment"));
    assert!(!output.contains("cookie") && !output.contains("screenshot"));
    assert!(run(&[
        "web-preview",
        id,
        "--url",
        url,
        "--label",
        "Price:",
        "--reading-mode",
        "unknown"
    ])
    .is_err());
    Ok(())
}
