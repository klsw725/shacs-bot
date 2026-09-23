use super::readiness_owner_support::{Fixture, TestResult};
use shacs_api::{serve_api_listener, ChatCompletionAdapter};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{load_config, LoadOptions};
use shacs_core::runtime::trusted_runtime::{
    ProcessAdapterRegistration, SandboxInactiveFallback, SandboxInactiveStatus, SandboxObservation,
    TraceDisclosureUpdate,
};
use shacs_projection::*;
use std::{net::TcpListener, sync::Arc, thread::JoinHandle};

struct Server {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            assert!(thread.join().is_ok());
        }
    }
}

fn start(
    listener: TcpListener,
    adapter: Arc<AgentLoopChatCompletionAdapter>,
) -> TestResult<Server> {
    listener.set_nonblocking(true)?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            serve_api_listener(listener, adapter, async {
                let _ = stopped.await;
            })
            .await
            .expect("API");
        });
    });
    Ok(Server {
        stop: Some(stop),
        thread: Some(thread),
    })
}

pub fn runtime_owner_parity() -> TestResult {
    let mut fixture = Fixture::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    fixture.port = listener.local_addr()?.port();
    fixture.config.api.port = fixture.port;
    fixture.config.plugins.trusted_workspaces =
        vec![fixture.config.agents.defaults.workspace.clone()];
    fixture.save_config()?;
    let bundle = load_config(LoadOptions {
        config_path: Some(fixture.root.join("config.json")),
        workspace_override: Some(fixture.root.join("workspace")),
        resolve_env: false,
        write_back_migrations: false,
    })?;
    let adapter = Arc::new(AgentLoopChatCompletionAdapter::from_bundle(bundle, false)?);
    let facts = adapter.spec030_fact_store();
    let direct = adapter.readiness_projection().ok_or("direct readiness")?;
    assert_eq!(
        direct["trusted_runtime"],
        serde_json::to_value(adapter.trusted_runtime_projection())?
    );
    assert_eq!(component(&direct, "runtime_controls")?["state"], "unknown");
    let _server = start(listener, adapter.clone())?;
    let initial = fixture.capture("runtime-initial-unknown")?;
    assert_eq!(component(&initial, "runtime_controls")?["state"], "unknown");
    facts.record_daemon_started()?;
    facts.update_trace(TraceDisclosureUpdate {
        raw_content_possible: true,
        surfaces: vec![
            DataSurface::Session,
            DataSurface::Trace,
            DataSurface::ToolOutput,
        ],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Enabled,
            preview: Some(TracePreviewProjection {
                record_count: 7,
                approximate_bytes: 351,
                destination: TraceDestination::ConfiguredRemote,
                exporter: Some("controlled-exporter".to_owned()),
                endpoint_summary: Some("https://trace.invalid".to_owned()),
            }),
        },
    })?;
    facts.update_resources(vec![ResourceCandidateProjection {
        resource_ref: "resource:controlled".to_owned(),
        kind: ResourceKind::Skill,
        source: ResourceSource::Explicit,
        precedence: ResourcePrecedence::Explicit,
        canonical_path: "resource:sha256:controlled".to_owned(),
        content_sha256: Some("a".repeat(64)),
        collision: ResourceCollisionStatus::None,
        load_status: ResourceLoadStatus::Loaded,
        activation: ResourceActivation::Explicit,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        diagnostics: vec![],
    }])?;
    for (label, sandbox, expected) in [
        ("runtime-disabled", SandboxObservation::Disabled, "degraded"),
        (
            "runtime-unsupported",
            SandboxObservation::Unsupported,
            "degraded",
        ),
        ("runtime-denied", SandboxObservation::Failed, "blocked"),
        (
            "runtime-fallback",
            SandboxObservation::Inactive {
                status: SandboxInactiveStatus::Failed,
                fallback: SandboxInactiveFallback::TrustedNativeFallback,
            },
            "degraded",
        ),
        (
            "runtime-active",
            SandboxObservation::Active {
                applied_adapters: vec![ProcessAdapterKind::GenericExec],
                filesystem_policy: SandboxFilesystemPolicy::Applied,
                network_policy: SandboxNetworkPolicy::Applied,
            },
            "ready",
        ),
    ] {
        facts.update_sandbox(sandbox)?;
        let expected_owner = serde_json::to_value(adapter.trusted_runtime_projection())?;
        let readiness = fixture.capture(label)?;
        assert_eq!(
            component(&readiness, "runtime_controls")?["state"],
            expected
        );
        assert_eq!(
            component(&readiness, "resource_disclosure")?["state"],
            "ready"
        );
        assert_eq!(readiness["trusted_runtime"], expected_owner);
        assert_eq!(
            readiness["trusted_runtime"]["disclosure"]["rawContentPossible"],
            true
        );
        assert_eq!(
            readiness["trusted_runtime"]["disclosure"]["trace"]["preview"]["destination"],
            "configuredRemote"
        );
    }
    facts.register_process_adapter(ProcessAdapterRegistration {
        adapter: ProcessAdapterKind::GenericExec,
        capabilities: ProcessAdapterCapabilities {
            timeout: true,
            abort: true,
            cwd: true,
            env: true,
            bounded_output: true,
            descendant_cleanup: false,
            startup_readiness: false,
            generation_fencing: false,
        },
        reason: ProcessControlReason::ControlledChildObservedNoRollback,
    })?;
    let limited = fixture.capture("runtime-limited-adapter")?;
    assert_eq!(
        component(&limited, "runtime_controls")?["state"],
        "degraded"
    );
    fixture.record("runtime-scope.json", &serde_json::to_vec_pretty(&serde_json::json!({
        "scope": "controlled owner observations through production adapter, real HTTP/CLI/bundle; not Linux execution proof",
        "api_direct_before_server": true, "actual_sandbox_execution": false,
        "initial_unknown_preserved": true, "containment_condition_unchanged": true
    }))?)?;
    Ok(())
}

fn component<'a>(
    readiness: &'a serde_json::Value,
    kind: &str,
) -> TestResult<&'a serde_json::Value> {
    readiness["components"]
        .as_array()
        .ok_or("components")?
        .iter()
        .find(|component| component["kind"] == kind)
        .ok_or_else(|| "missing component".into())
}
