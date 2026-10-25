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
        root.path(),
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
        root.path(),
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
