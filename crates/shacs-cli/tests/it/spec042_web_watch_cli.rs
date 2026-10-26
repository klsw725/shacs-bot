use shacs_cli::{parse_cli_args, run_command};
use shacs_core::runtime::{
    confirm_web_watch, preview_web_watch, read_web_watch, ConfirmWebWatchRequest,
    EntrustedWorkReadRequest, EntrustedWorkSnapshot, NetworkGuard, ObservationNextWake,
    WebCondition, WebFetchLimits, WebPreviewRequest, WebPreviewTarget, WebWatchSnapshot,
};
use shacs_session::{Session, SessionManager};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

#[test]
fn product_web_watch_commands_refuse_local_pages_and_report_unconfigured_status(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    SessionManager::new(root.path())?.save(&Session::new("cli:owner"))?;
    let created: EntrustedWorkSnapshot = serde_json::from_str(&run(
        root.path(),
        &[
            "create",
            "watch a price",
            "--consent",
            "explicit-request",
            "--evidence-ref",
            "cli:user",
            "--instructions",
            "tell me when it is cheaper",
        ],
    )?)?;
    let work_id = created.work.work_id.as_str();
    let url = "http://127.0.0.1:9/item?token=secret";

    // When
    let preview = run(
        root.path(),
        &["web-preview", work_id, "--url", url, "--label", "가격"],
    );
    let two_conditions = run(
        root.path(),
        &[
            "web-watch",
            work_id,
            "--url",
            url,
            "--candidate",
            "token",
            "--at-most",
            "1",
            "--text-present",
            "--cadence-seconds",
            "600",
            "--expected-revision",
            "1",
        ],
    );
    let data_dir = root.path().to_str().ok_or("path")?;
    let status: WebWatchSnapshot = serde_json::from_str(&run(
        root.path(),
        &["web-status", work_id, "--data-dir", data_dir],
    )?)?;

    // Then
    let error = preview
        .err()
        .ok_or("a local page must be refused")?
        .to_string();
    assert!(error.contains("Blocked"), "{error}");
    assert!(two_conditions.is_err());
    assert!(status.plan.is_none());
    assert_eq!(status.next_wake, ObservationNextWake::NotConfigured);
    Ok(())
}

#[test]
fn session_export_shows_the_watched_url_without_query_or_fragment(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: a confirmed watch whose URL carries a secret query and a fragment.
    let root = tempfile::tempdir()?;
    let work = confirmed_watch(root.path())?;
    let plan = read_web_watch(root.path(), root.path(), &work)?
        .plan
        .ok_or("no plan")?;

    // When
    let mut exports = Vec::new();
    for format in ["json", "jsonl"] {
        exports.push(run_command(parse_cli_args(
            [
                "session",
                "export",
                "--workspace",
                root.path().to_str().ok_or("path")?,
                "--session",
                "cli:owner",
                "--format",
                format,
                "--yes",
            ]
            .map(str::to_owned),
        )?)?);
    }

    // Then
    for export in exports {
        assert!(
            !export.contains("secret-token") && !export.contains("reviews"),
            "{export}"
        );
        assert!(export.contains(&format!("\"{}\"", plan.url)), "{export}");
        assert!(export.contains(&plan.url_digest), "{export}");
    }
    Ok(())
}

#[test]
fn web_notify_needs_an_enabled_email_channel_that_can_send(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: two configs with consent and SMTP settings, one with the channel disabled.
    let root = tempfile::tempdir()?;
    let work = confirmed_watch(root.path())?;
    let config = |enabled: bool| -> Result<String, Box<dyn std::error::Error>> {
        let path = root.path().join(format!("config-{enabled}.json"));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"channels": {"email": {
                "enabled": enabled,
                "consentGranted": true,
                "smtp": {"host": "127.0.0.1", "port": 2525, "from": "bot@example.test", "security": "plain"}
            }}}))?,
        )?;
        Ok(path.to_str().ok_or("path")?.to_owned())
    };
    let (disabled, enabled) = (config(false)?, config(true)?);
    let workspace = root.path().to_str().ok_or("path")?;
    let notify = |before: &[&str], after: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let mut args = before.to_vec();
        args.extend([
            "work",
            "web-notify",
            &work.work_id,
            "--target",
            "email:user@example.test",
            "--workspace",
            workspace,
            "--session",
            "cli:owner",
        ]);
        args.extend(after);
        Ok(run_command(parse_cli_args(args)?)?)
    };

    // When: each config before the command, after it, and both with the later one winning.
    let refused = [
        notify(&[], &["--config", &disabled]),
        notify(&["--config", &disabled], &[]),
    ];
    let accepted = [
        notify(&["--config", &enabled], &[])?,
        notify(&["--config", &disabled], &["--config", &enabled])?,
    ];

    // Then
    for result in refused {
        let error = result
            .err()
            .ok_or("a disabled Email channel must be refused")?
            .to_string();
        assert!(error.contains("Email channel cannot send"), "{error}");
    }
    let revisions = accepted
        .iter()
        .map(|output| Ok(serde_json::from_str::<serde_json::Value>(output)?["revision"].clone()))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    assert_eq!(revisions, [1, 2]);
    Ok(())
}

#[test]
fn web_deadline_is_set_checked_and_cleared_from_the_command_line(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: a confirmed watch with an email target.
    let root = tempfile::tempdir()?;
    let work = confirmed_watch(root.path())?;
    let config = root.path().join("config.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({"channels": {"email": {
            "enabled": true,
            "consentGranted": true,
            "smtp": {"host": "127.0.0.1", "port": 2525, "from": "bot@example.test", "security": "plain"}
        }}}))?,
    )?;
    let config = config.to_str().ok_or("path")?;
    run(
        root.path(),
        &[
            "web-notify",
            &work.work_id,
            "--target",
            "email:user@example.test",
            "--config",
            config,
        ],
    )?;
    let deadline = |arguments: &[&str]| {
        let mut all = vec!["web-deadline", work.work_id.as_str()];
        all.extend_from_slice(arguments);
        run(root.path(), &all)
    };

    // When
    let set = deadline(&[
        "--at",
        "2099-01-01T09:00:00+09:00",
        "--remind-before-seconds",
        "3600",
        "--expected-deadline-revision",
        "0",
    ])?;
    let stale = deadline(&[
        "--at",
        "2099-01-02T09:00:00+09:00",
        "--expected-deadline-revision",
        "0",
    ]);
    let no_offset = deadline(&[
        "--at",
        "2099-01-02T09:00:00",
        "--expected-deadline-revision",
        "1",
    ]);
    let cleared = deadline(&["--clear", "--expected-deadline-revision", "1"])?;

    // Then
    let set: serde_json::Value = serde_json::from_str(&set)?;
    assert_eq!(set["revision"], 1);
    assert_eq!(set["deadline"]["at_text"], "2099-01-01T09:00:00+09:00");
    assert_eq!(
        set["deadline"]["remind_at_ms"].as_u64(),
        set["deadline"]["at_ms"]
            .as_u64()
            .map(|at_ms| at_ms - 3_600_000)
    );
    assert!(stale.is_err() && no_offset.is_err());
    let cleared: serde_json::Value = serde_json::from_str(&cleared)?;
    assert_eq!(cleared["revision"], 2);
    assert!(cleared["deadline"].is_null());
    Ok(())
}

/// A confirmed watch on a local page whose URL carries a secret query and a fragment.
fn confirmed_watch(root: &Path) -> Result<EntrustedWorkReadRequest, Box<dyn std::error::Error>> {
    SessionManager::new(root)?.save(&Session::new("cli:owner"))?;
    let created: EntrustedWorkSnapshot = serde_json::from_str(&run(
        root,
        &[
            "create",
            "watch a price",
            "--consent",
            "explicit-request",
            "--evidence-ref",
            "cli:user",
            "--instructions",
            "tell me when it is cheaper",
        ],
    )?)?;
    let url = format!(
        "{}/item?token=secret-token#reviews",
        serve_page("<p>Price: $12.99 per month</p>")?
    );
    let work = EntrustedWorkReadRequest {
        owner_session_id: "cli:owner".to_owned(),
        requester_target: "cli:owner".to_owned(),
        work_id: created.work.work_id,
    };
    let guard = NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]);
    let limits = WebFetchLimits::PRODUCTION;
    let preview = preview_web_watch(
        root,
        &WebPreviewRequest {
            work: work.clone(),
            url: url.clone(),
            target: WebPreviewTarget::Number {
                label: "Price:".to_owned(),
                decimal_comma: false,
            },
        },
        &guard,
        &limits,
    )?;
    let candidate_token = preview
        .candidates
        .first()
        .ok_or("no candidate")?
        .token
        .clone();
    confirm_web_watch(
        root,
        ConfirmWebWatchRequest {
            work: work.clone(),
            expected_revision: 1,
            url,
            candidate_token,
            condition: WebCondition::AtMost {
                value: "10".to_owned(),
            },
            cadence_seconds: 600,
            now_ms: 1_800_000_000_000,
        },
        &guard,
        &limits,
    )?;
    Ok(work)
}

fn serve_page(body: &'static str) -> Result<String, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok(base)
}

fn run(workspace: &Path, action: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["work".to_owned()];
    args.extend(action.iter().map(|value| (*value).to_owned()));
    args.extend([
        "--workspace".to_owned(),
        workspace.display().to_string(),
        "--session".to_owned(),
        "cli:owner".to_owned(),
    ]);
    Ok(run_command(parse_cli_args(args)?)?)
}
