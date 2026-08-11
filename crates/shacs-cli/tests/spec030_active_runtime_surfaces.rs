use serde_json::Value;
use shacs_api::{serve_api_listener, ApiError, ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, ProcessAdapterRegistration, SandboxObservation,
    Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialStatus, CredentialStatusProjection,
    HookRuntimeProjection, HookRuntimeStatus, ProcessAdapterCapabilities, ProcessAdapterKind,
    ProcessControlReason, RefreshSerializationStatus, SandboxFilesystemPolicy,
    SandboxNetworkPolicy, Spec030Availability, Spec030ProjectionProvider, Spec030RuntimeProjection,
};
use shacs_providers::LlmResponse;
use std::error::Error;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Clone)]
struct LiveAdapter(LocalSpec030ProjectionProvider);

impl ChatCompletionAdapter for LiveAdapter {
    fn configured_model(&self) -> &str {
        "spec030-live"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        self.0.projection()
    }
}

struct ApiProcess {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ApiProcess {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn separate_cli_and_tui_processes_observe_updated_active_runtime_facts(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let config = root.path().join("config.json");
    std::fs::write(
        &config,
        serde_json::json!({"api":{"host":"127.0.0.1","port":port}}).to_string(),
    )?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let server = start_api(listener, facts.clone())?;

    facts.update_hooks(HookRuntimeProjection {
        availability: Spec030Availability::Available,
        status: HookRuntimeStatus::Active,
        registered_handlers: 3,
        diagnostics: Vec::new(),
        recent_denials: Vec::new(),
    })?;
    facts.register_process_adapter(ProcessAdapterRegistration {
        adapter: ProcessAdapterKind::Bash,
        capabilities: ProcessAdapterCapabilities {
            timeout: true,
            abort: true,
            cwd: true,
            env: true,
            bounded_output: true,
            descendant_cleanup: true,
            startup_readiness: false,
            generation_fencing: false,
        },
        reason: ProcessControlReason::ControlledChildObservedNoRollback,
    })?;
    facts.update_sandbox(SandboxObservation::Active {
        applied_adapters: vec![ProcessAdapterKind::Bash],
        filesystem_policy: SandboxFilesystemPolicy::Applied,
        network_policy: SandboxNetworkPolicy::Applied,
    })?;
    facts.update_credential(CredentialStatusProjection {
        availability: Spec030Availability::Degraded,
        status: CredentialStatus::Missing,
        source: None,
        fingerprint: CredentialFingerprintStatus::Unavailable,
        refresh_serialization: RefreshSerializationStatus::Inactive,
    })?;

    let cli = run_cli(&config, &workspace)?;
    let tui = run_tui(&config, &workspace)?;

    assert_eq!(cli["hooks"]["registeredHandlers"], 3);
    assert_eq!(cli["sandbox"]["status"], "active");
    assert_eq!(cli["credential"]["status"], "missing");
    assert!(tui.contains("registeredHandlers=3"));
    assert!(tui.contains("sandbox: availability=available status=active"));
    assert!(tui.contains("credential: availability=degraded status=missing"));
    drop(server);
    Ok(())
}

fn start_api(listener: TcpListener, facts: Spec030FactStore) -> Result<ApiProcess, Box<dyn Error>> {
    listener.set_nonblocking(true)?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|error| panic!("test runtime failed: {error}"));
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener)
                .unwrap_or_else(|error| panic!("test listener failed: {error}"));
            let adapter = Arc::new(LiveAdapter(LocalSpec030ProjectionProvider::new(facts)));
            serve_api_listener(listener, adapter, async {
                let _ = stopped.await;
            })
            .await
            .unwrap_or_else(|error| panic!("test API failed: {error}"));
        });
    });
    Ok(ApiProcess {
        stop: Some(stop),
        thread: Some(thread),
    })
}

fn run_cli(config: &Path, workspace: &Path) -> Result<Value, Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args(["runtime", "trusted-runtime", "--format", "json", "--config"])
        .arg(config)
        .arg("--workspace")
        .arg(workspace)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn run_tui(config: &Path, workspace: &Path) -> Result<String, Box<dyn Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace manifest parent missing")?
        .join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(manifest)
        .args(["-p", "shacs-tui", "--", "--once", "--config"])
        .arg(config)
        .arg("--workspace")
        .arg(workspace)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}
