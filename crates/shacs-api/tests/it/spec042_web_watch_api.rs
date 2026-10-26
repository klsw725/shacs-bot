use axum::body::{to_bytes, Body};
use serde_json::{json, Value};
use shacs_api::{api_router, api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_core::runtime::*;
use shacs_session::{Session, SessionManager};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::{path::PathBuf, sync::Arc};
use tower::ServiceExt;

/// Test adapter: unlike the product adapter it allows the local test server.
struct Adapter(PathBuf);

impl ChatCompletionAdapter for Adapter {
    fn configured_model(&self) -> &str {
        "unused"
    }
    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<shacs_providers::LlmResponse, shacs_api::ApiError> {
        Err(shacs_api::ApiError::internal(
            "web watch routes must not invoke a provider",
        ))
    }
    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn web_watch_network_guard(&self) -> NetworkGuard {
        NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"])
    }
    fn now_ms(&self) -> u64 {
        1_800_000_000_000
    }
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

#[tokio::test]
async fn web_watch_routes_preview_confirm_and_report_masked_status(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    SessionManager::new(root.path())?.save(&Session::new("api:owner"))?;
    let created = create_entrusted_work(
        root.path(),
        EntrustedWorkCreateRequest {
            owner_session_id: "api:owner".to_owned(),
            goal_text: "watch a price".to_owned(),
            turn_budget: 5,
            consent: EntrustedWorkConsentProof {
                kind: EntrustedWorkConsentKind::ExplicitRequest,
                evidence_ref: "api:user".to_owned(),
                recorded_at: "100".to_owned(),
            },
            conditions: EntrustedWorkConditions {
                instructions: "tell me when it is cheaper".to_owned(),
                budget: None,
                prior_decisions: Vec::new(),
                remaining_questions: Vec::new(),
            },
            linked_targets: Vec::new(),
            observed_at: "100".to_owned(),
        },
    )?;
    let url = format!(
        "{}/item?token=secret#tab",
        serve_page("<p>Price: $12.99 per month</p>")?
    );
    let adapter = Arc::new(Adapter(root.path().to_path_buf()));
    let path = format!(
        "/v1/sessions/api%3Aowner/entrusted-work/{}/web-watch",
        created.work.work_id
    );
    let local = || api_router_with_local_mutations(adapter.clone());

    // When
    let denied = request(
        api_router(adapter.clone()),
        "POST",
        &format!("{path}/preview"),
        json!({"url": url, "label": "Price:"}),
    )
    .await?;
    let preview = request(
        local(),
        "POST",
        &format!("{path}/preview"),
        json!({"url": url, "label": "Price:"}),
    )
    .await?;
    let area = request(
        local(),
        "POST",
        &format!("{path}/preview"),
        json!({"url": url, "phrase": "sold out", "label": "Price:", "until": "month"}),
    )
    .await?;
    let candidate = preview.1["candidates"][0]["token"]
        .as_str()
        .ok_or("no candidate")?;
    let confirm = json!({"expected_revision": 1, "url": url, "candidate": candidate, "condition": {"kind": "at_most", "value": "10"}, "cadence_seconds": 600});
    let configured = request(local(), "POST", &path, confirm.clone()).await?;
    let stale = request(local(), "POST", &path, confirm).await?;
    let notify_path = format!("{path}/notify");
    let without_email = request(
        local(),
        "POST",
        &notify_path,
        json!({"target": "email:user@example.test"}),
    )
    .await?;
    let off = request(local(), "POST", &notify_path, json!({"off": true})).await?;
    let deadline_path = format!("{path}/deadline");
    let untargeted = request(
        local(),
        "POST",
        &deadline_path,
        json!({"expected_revision": 0, "at": "2099-01-01T09:00:00+09:00"}),
    )
    .await?;
    let cleared = request(
        local(),
        "POST",
        &deadline_path,
        json!({"expected_revision": 0, "clear": true}),
    )
    .await?;
    let status = request(api_router(adapter.clone()), "GET", &path, json!({})).await?;

    // Then
    assert_eq!(denied.0, 403);
    assert_eq!(preview.0, 200);
    assert_eq!(
        preview.1["candidates"][0]["value"],
        json!({"kind": "number", "value": "12.99"})
    );
    assert_eq!(area.0, 200);
    assert_eq!(area.1["candidates"][0]["shown_text"], "$12.99 per");
    assert_eq!(
        area.1["candidates"][0]["value"],
        json!({"kind": "text", "present": false})
    );
    assert_eq!(configured.0, 200);
    assert_eq!(configured.1["condition_revision"], 2);
    assert_eq!(stale.0, 409);
    assert_eq!(without_email.0, 400);
    assert_eq!((off.0, off.1["revision"].clone()), (200, json!(1)));
    assert_eq!(status.0, 200);
    assert_eq!(status.1["notify"]["revision"], 1);
    assert_eq!(untargeted.0, 400);
    assert_eq!((cleared.0, cleared.1["revision"].clone()), (200, json!(1)));
    assert_eq!(status.1["deadline"]["revision"], 1);
    assert_eq!(status.1["next_wake"]["status"], "awaiting_enqueue");
    assert_eq!(status.1["baseline"]["value"]["value"], "12.99");
    let output = status.1.to_string();
    assert!(output.contains("/item") && !output.contains("secret") && !output.contains("#tab"));
    Ok(())
}

async fn request(
    app: axum::Router,
    method: &str,
    path: &str,
    body: Value,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    let status = response.status().as_u16();
    let value = serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await?)?;
    Ok((status, value))
}
