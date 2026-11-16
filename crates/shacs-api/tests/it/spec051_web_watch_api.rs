use super::spec042_web_watch_api::request;
use serde_json::json;
use shacs_api::{
    api_router_with_local_mutations, ApiError, ChatCompletionAdapter, ChatCompletionInvocation,
};
use shacs_core::runtime::{
    self, EntrustedWorkConditions, EntrustedWorkConsentKind, EntrustedWorkConsentProof,
    EntrustedWorkCreateRequest, RenderedWebFailure, RenderedWebPage, RenderedWebReader,
    RenderedWebRequest, WebCheckFailure,
};
use shacs_providers::LlmResponse;
use shacs_session::{Session, SessionManager};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

struct Adapter {
    workspace: PathBuf,
    reads: AtomicUsize,
    busy: AtomicBool,
    provider_calls: AtomicUsize,
    login: bool,
}

impl RenderedWebReader for Adapter {
    fn enabled(&self) -> bool {
        true
    }
    fn saved_login_enabled(&self) -> bool {
        self.login
    }
    fn read(
        &self,
        request: &RenderedWebRequest<'_>,
    ) -> Result<RenderedWebPage, RenderedWebFailure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.busy.load(Ordering::SeqCst) {
            return Err(RenderedWebFailure {
                failure: WebCheckFailure::BrowserBusy,
                browser_reason: None,
                response: None,
            });
        }
        Ok(RenderedWebPage {
            text: "Price: 12900 USD".to_owned(),
            final_url: request.url.to_owned(),
            http_status: 200,
            retry_after: None,
            truncated: false,
        })
    }
}

impl ChatCompletionAdapter for Adapter {
    fn configured_model(&self) -> &str {
        "unused"
    }
    fn complete_chat(&self, _: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        self.provider_calls.fetch_add(1, Ordering::SeqCst);
        Err(ApiError::internal("watch must not call provider"))
    }
    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }
    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }
    fn rendered_web_reader(&self) -> Option<&dyn RenderedWebReader> {
        Some(self)
    }
    fn now_ms(&self) -> u64 {
        1_800_000_000_000
    }
}

#[tokio::test]
async fn browser_preview_confirmation_and_status_preserve_mode(
) -> Result<(), Box<dyn std::error::Error>> {
    preview_confirm_status(false).await
}

#[tokio::test]
async fn saved_login_selection_survives_preview_confirm_and_status(
) -> Result<(), Box<dyn std::error::Error>> {
    preview_confirm_status(true).await
}

async fn preview_confirm_status(saved: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    SessionManager::new(root.path())?.save(&Session::new("api:owner"))?;
    let created = runtime::create_entrusted_work(
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
                instructions: "tell me".to_owned(),
                budget: None,
                prior_decisions: Vec::new(),
                remaining_questions: Vec::new(),
            },
            linked_targets: Vec::new(),
            observed_at: "100".to_owned(),
        },
    )?;
    let adapter = Arc::new(Adapter {
        workspace: root.path().to_path_buf(),
        reads: AtomicUsize::new(0),
        busy: AtomicBool::new(false),
        provider_calls: AtomicUsize::new(0),
        login: saved,
    });
    let app = || api_router_with_local_mutations(adapter.clone());
    let path = format!(
        "/v1/sessions/api%3Aowner/entrusted-work/{}/web-watch",
        created.work.work_id
    );
    let preview_path = format!("{path}/preview");
    let url = "https://93.184.216.34/item?private-query#private-fragment";
    let invalid = request(
        app(),
        "POST",
        &preview_path,
        json!({"url":url,"label":"Price:","reading_mode":"unknown"}),
    )
    .await?;
    assert_eq!(invalid.0, 400);
    assert_eq!(adapter.reads.load(Ordering::SeqCst), 0);
    let preview = request(
        app(),
        "POST",
        &preview_path,
        json!({"url":url,"label":"Price:","reading_mode":"browser","use_saved_login":saved}),
    )
    .await?;
    assert_eq!(preview.0, 200);
    assert_eq!(preview.1["reading_mode"], "browser");
    assert_eq!(preview.1["use_saved_login"], saved);
    let candidate = preview.1["candidates"][0]["token"]
        .as_str()
        .ok_or("token")?;
    let mut confirm = json!({"url":url,"candidate":candidate,"expected_revision":1,
        "condition":{"kind":"at_most","value":"11000"},"cadence_seconds":600});
    let mismatch = request(app(), "POST", &path, confirm.clone()).await?;
    assert_eq!(mismatch.0, 400);
    assert_eq!(adapter.reads.load(Ordering::SeqCst), 1);
    confirm["reading_mode"] = json!("browser");
    if saved {
        let mismatch = request(app(), "POST", &path, confirm.clone()).await?;
        assert_eq!(mismatch.0, 400);
        assert_eq!(adapter.reads.load(Ordering::SeqCst), 1);
    }
    confirm["use_saved_login"] = json!(saved);
    let configured = request(app(), "POST", &path, confirm).await?;
    assert_eq!(configured.0, 200);
    assert_eq!(configured.1["plan"]["reading_mode"], "browser");
    assert_eq!(
        configured.1["plan"]["extraction"]["version"],
        "rendered_text_v1"
    );
    assert_eq!(configured.1["baseline"]["value"]["value"], "12900");
    assert_eq!(adapter.reads.load(Ordering::SeqCst), 2);
    adapter.busy.store(true, Ordering::SeqCst);
    let busy = request(
        app(),
        "POST",
        &preview_path,
        json!({"url":url,"label":"Price:","reading_mode":"browser"}),
    )
    .await?;
    assert_eq!(busy.0, 422);
    let status = request(app(), "GET", &path, json!({})).await?;
    assert_eq!(status.0, 200);
    assert_eq!(status.1["condition_revision"], 2);
    assert_eq!(status.1["plan"]["reading_mode"], "browser");
    assert_eq!(status.1["plan"]["use_saved_login"], saved);
    assert_eq!(status.1["baseline"]["value"]["value"], "12900");
    for response in [configured.1, status.1, busy.1] {
        let output = response.to_string();
        for secret in ["private-query", "private-fragment", "cookie", "screenshot"] {
            assert!(!output.contains(secret), "response leaked {secret}");
        }
    }
    assert_eq!(adapter.provider_calls.load(Ordering::SeqCst), 0);
    Ok(())
}
