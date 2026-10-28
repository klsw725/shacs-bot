use axum::body::{to_bytes, Body};
use serde_json::{json, Value};
use shacs_api::{api_router, api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_core::runtime::*;
use shacs_session::{Session, SessionManager};
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

struct Adapter(PathBuf);
impl ChatCompletionAdapter for Adapter {
    fn configured_model(&self) -> &str {
        "unused"
    }
    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<shacs_providers::LlmResponse, shacs_api::ApiError> {
        panic!("observation routes must not invoke provider")
    }
    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

#[tokio::test]
async fn observation_routes_require_opt_in_and_linkage_and_persist_configuration(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    manager.save(&Session::new("api:owner"))?;
    let created = create_entrusted_work(
        root.path(),
        EntrustedWorkCreateRequest {
            owner_session_id: "api:owner".to_owned(),
            goal_text: "local observation".to_owned(),
            turn_budget: 5,
            consent: EntrustedWorkConsentProof {
                kind: EntrustedWorkConsentKind::ExplicitRequest,
                evidence_ref: "api:user".to_owned(),
                recorded_at: "now".to_owned(),
            },
            conditions: EntrustedWorkConditions {
                instructions: "read local only".to_owned(),
                budget: None,
                prior_decisions: Vec::new(),
                remaining_questions: Vec::new(),
            },
            linked_targets: Vec::new(),
            observed_at: "now".to_owned(),
        },
    )?;
    let adapter = Arc::new(Adapter(root.path().to_path_buf()));
    let path = format!(
        "/v1/sessions/api%3Aowner/entrusted-work/{}/observations",
        created.work.work_id
    );
    let body = json!({"expected_revision": 1, "source": {"kind": "local_file", "relative_path": "source.txt"}, "cadence_seconds": 5});

    let denied = request(api_router(adapter.clone()), "POST", &path, body.clone()).await?;
    let configured = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        &path,
        body,
    )
    .await?;
    let restarted = request(
        api_router(Arc::new(Adapter(root.path().to_path_buf()))),
        "GET",
        &path,
        json!({}),
    )
    .await?;
    let unlinked_path = format!(
        "/v1/sessions/api%3Aother/entrusted-work/api%3Aowner/{}/observations",
        created.work.work_id
    );
    let unlinked = request(api_router(adapter), "GET", &unlinked_path, json!({})).await?;

    assert_eq!(denied.0, 403);
    assert_eq!(configured.0, 200);
    assert_eq!(configured.1["work"]["current_revision"], 2);
    assert_eq!(restarted.0, 200);
    assert_eq!(restarted.1["next_wake"]["status"], "awaiting_enqueue");
    assert!(restarted.1["current_observation"].is_null());
    assert_eq!(unlinked.0, 403);
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

impl EntrustedObservationExecutor for Adapter {
    fn prepare(&self, _: &ObservationWake) -> Result<String, ObservationBlock> {
        Ok("snapshot:api-test".to_owned())
    }
    fn observe(&self, wake: &ObservationWake) -> ObservationResult {
        use shacs_core::tools::Tool;
        let ObservationSource::LocalFile { relative_path } = &wake.plan.source;
        let result = BoundedObservationTool::new(&self.0, wake).execute(
            serde_json::Map::from_iter([("path".to_owned(), json!(relative_path))]),
        );
        serde_json::from_str(&result.into_text()).expect("bounded result")
    }
}

#[tokio::test]
async fn comparison_api_round_trips_persisted_decisions_and_rejects_stale_configuration(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    SessionManager::new(root.path())?.save(&Session::new("api:owner"))?;
    let adapter = Arc::new(Adapter(root.path().to_path_buf()));
    let app = api_router_with_local_mutations(adapter.clone());
    let created = request(app.clone(), "POST", "/v1/sessions/api%3Aowner/entrusted-work", json!({
        "goal_text":"observe value", "consent":{"kind":"explicit_request", "evidence_ref":"api:user", "recorded_at":"now"},
        "conditions":{"instructions":"observe only"}, "observed_at":"now"
    })).await?;
    assert_eq!(created.0, 201);
    let work_id = created.1["work"]["work_id"].as_str().ok_or("work")?;
    let path = format!("/v1/sessions/api%3Aowner/entrusted-work/{work_id}/observations");
    let body = json!({"expected_revision":1, "source":{"kind":"local_file", "relative_path":"source.json"}, "cadence_seconds":5,
        "comparison":{"criterion":{"kind":"integer_at_most", "value":100}, "max_age_seconds":60, "regular_summary":true}});
    let configured = request(app.clone(), "POST", &path, body.clone()).await?;
    assert_eq!(configured.0, 200);
    let first = configured.1["work"]["observation_plan"]["first_wake_at_ms"]
        .as_u64()
        .ok_or("time")?;
    let bus = MessageBus::new();
    let mut dispatcher = DurableWorkDispatcher::open(
        root.path().join("runtime/durable-events"),
        root.path().join("runtime/work-payloads"),
        bus.clone(),
        "test",
        120_000,
    )?;
    let mut runtime = ObservationRuntime {
        workspace: root.path(),
        data_dir: root.path(),
        now_ms: first,
    };
    dispatcher.enqueue_entrusted_observations(&runtime)?;
    for value in [120, 120, 110, 90] {
        std::fs::write(
            root.path().join("source.json"),
            serde_json::to_vec(
                &json!({"schema":"integer_v1", "value":value, "as_of_ms":runtime.now_ms}),
            )?,
        )?;
        dispatcher.process_entrusted_observations(&runtime, adapter.as_ref())?;
        runtime.now_ms += 5000;
    }

    let restarted = request(
        api_router(Arc::new(Adapter(root.path().to_path_buf()))),
        "GET",
        &path,
        json!({}),
    )
    .await?;

    assert_eq!(restarted.0, 200);
    let decisions = restarted.1["decisions"].as_array().ok_or("decisions")?;
    assert_eq!(
        decisions
            .iter()
            .map(|decision| decision["verdict"].as_str())
            .collect::<Vec<_>>(),
        [
            Some("baseline"),
            Some("suppress"),
            Some("suppress"),
            Some("notify_candidate")
        ]
    );
    assert_eq!(decisions[3]["previous"]["summary"]["value"], 110);
    assert_eq!(decisions[3]["current"]["summary"]["value"], 90);
    assert_eq!(decisions[1]["regular_summary_eligible"], true);
    assert_eq!(request(app, "POST", &path, body).await?.0, 409);
    assert_eq!((bus.inbound_size(), bus.outbound_size()), (0, 0));
    Ok(())
}
