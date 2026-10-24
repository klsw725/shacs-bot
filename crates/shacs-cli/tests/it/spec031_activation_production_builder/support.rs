use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{
    Config, ConfigBundle, ConfigContext, ProviderConfig, TrustedResourceConfig, TrustedResourceKind,
};
use shacs_core::app::{
    AppId, AppLifecycleState, AppRegistry, AppRegistryEntry, AppRegistryStore, AppResourceSummary,
};
use shacs_core::runtime::{
    ActivationReason, ActivationRecord, ActivationRecordInput, ActivationSource, ActivationStatus,
    ActivationStore, ExecutionSnapshot, WorkspaceTrustRef,
};
use shacs_providers::{GenerationSettings, ProviderRequest};
use shacs_session::SessionManager;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

type ProviderServer = thread::JoinHandle<Result<(), String>>;

pub struct Scenario {
    status: Option<ActivationStatus>,
    record_manifest_digest: &'static str,
}

impl Scenario {
    pub const fn active() -> Self {
        Self {
            status: Some(ActivationStatus::Active),
            record_manifest_digest: "sha256:app-manifest-a",
        }
    }
    pub const fn missing() -> Self {
        Self {
            status: None,
            record_manifest_digest: "sha256:app-manifest-a",
        }
    }
    pub const fn status(status: ActivationStatus) -> Self {
        Self {
            status: Some(status),
            record_manifest_digest: "sha256:app-manifest-a",
        }
    }
    pub const fn digest_mismatch() -> Self {
        Self {
            status: Some(ActivationStatus::Active),
            record_manifest_digest: "sha256:app-manifest-b",
        }
    }
}

pub fn run(scenario: Scenario) -> Result<ExecutionSnapshot, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let app_root = root.path().join("apps/formatter.shacsapp");
    std::fs::create_dir_all(&workspace)?;
    std::fs::create_dir_all(&app_root)?;
    let resource_path = app_root.join("formatter.py");
    std::fs::write(&resource_path, b"print('formatter')")?;
    let canonical_resource = std::fs::canonicalize(&resource_path)?;
    let content_digest = format!("{:x}", Sha256::digest(std::fs::read(&canonical_resource)?));
    let app_digest = "sha256:app-manifest-a";
    seed_app(root.path(), &app_root, app_digest)?;
    let context = ConfigContext {
        config_path: root.path().join("config.json"),
        data_dir: root.path().to_path_buf(),
        workspace: workspace.clone(),
    };
    if let Some(status) = scenario.status {
        seed_activation(
            &context,
            &canonical_resource,
            &content_digest,
            scenario.record_manifest_digest,
            status,
        )?;
    }
    let (api_base, server) = serve_chat_response()?;
    let config = config(&canonical_resource, &api_base);
    std::fs::write(&context.config_path, serde_json::to_vec(&config)?)?;
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(
        ConfigBundle {
            config,
            context,
            migrations: Vec::new(),
        },
        false,
    )?;
    adapter.complete_chat(invocation())?;
    server.join().map_err(|_| "provider thread panicked")??;
    persisted_snapshot(&workspace)
}

fn seed_app(
    data_dir: &std::path::Path,
    app_root: &std::path::Path,
    digest: &str,
) -> Result<(), Box<dyn Error>> {
    let app_id = AppId::parse("formatter")?;
    let mut registry = AppRegistry::default();
    registry.entries.insert(
        app_id.clone(),
        AppRegistryEntry {
            app_id,
            version: "1.0.0".to_owned(),
            digest: digest.to_owned(),
            bundle_path: app_root.to_path_buf(),
            lifecycle_state: AppLifecycleState::Enabled,
            permission_requests: Vec::new(),
            secret_requests: Vec::new(),
            resource_summaries: vec![AppResourceSummary {
                kind: "entry".to_owned(),
                path: "formatter.py".to_owned(),
                size_bytes: 18,
                sha256: String::new(),
            }],
            grant_reference: None,
            unavailable_reasons: Vec::new(),
            process_snapshots: Vec::new(),
            installed_at_unix_ms: 31_100,
        },
    );
    AppRegistryStore::new(data_dir).save(&registry)?;
    Ok(())
}

fn seed_activation(
    context: &ConfigContext,
    source: &std::path::Path,
    content_digest: &str,
    manifest_digest: &str,
    status: ActivationStatus,
) -> Result<(), Box<dyn Error>> {
    let owner = WorkspaceTrustRef::new(context.workspace_permission_id()?.as_str());
    ActivationStore::new(
        context
            .runtime_subdir("snapshots")
            .join("activation-records.json"),
    )
    .put(ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:formatter:v1".to_owned(),
        source: ActivationSource::App,
        workspace_trust_ref: owner,
        resource_ref: "resource:formatter".to_owned(),
        source_identity: format!("source:app:{}", source.to_string_lossy()),
        content_digest: content_digest.to_owned(),
        dependency_manifest_digest: manifest_digest.to_owned(),
        status,
        reason: match status {
            ActivationStatus::Active => ActivationReason::Activated,
            ActivationStatus::Disabled => ActivationReason::UserDisabled,
            ActivationStatus::Revoked => ActivationReason::UserRevoked,
            ActivationStatus::Stale => ActivationReason::ContentDigestMismatch,
            ActivationStatus::Removed => ActivationReason::SourceRemoved,
        },
        recorded_at_unix_ms: 31_100,
    }))?;
    Ok(())
}

fn config(resource_path: &std::path::Path, api_base: &str) -> Config {
    let mut config = Config::default();
    config.agents.defaults.provider = "openai".to_owned();
    config.agents.defaults.model = "gpt-4o".to_owned();
    config.providers.insert(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("test-key".to_owned()),
            api_base: Some(api_base.to_owned()),
            ..ProviderConfig::default()
        },
    );
    config
        .trusted_runtime
        .resources
        .push(TrustedResourceConfig {
            resource_ref: "resource:formatter".to_owned(),
            kind: TrustedResourceKind::Python,
            path: resource_path.to_string_lossy().into_owned(),
            program: Some("python3".to_owned()),
            args: Vec::new(),
            module: Some("json".to_owned()),
            runtime: None,
        });
    config
}

fn invocation() -> ChatCompletionInvocation {
    ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role":"user","content":"hello"})],
            tools: Vec::new(),
            model: "gpt-4o".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: Some("gpt-4o".to_owned()),
        session_key: "api:activation".to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    }
}

fn persisted_snapshot(workspace: &std::path::Path) -> Result<ExecutionSnapshot, Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let key = manager
        .list_sessions()?
        .into_iter()
        .next()
        .ok_or("session missing")?
        .key;
    let value = manager
        .get_or_create(&key)
        .metadata
        .get("spec031_execution_snapshot")
        .cloned()
        .ok_or("snapshot missing")?;
    Ok(ExecutionSnapshot::parse_json(&value.to_string())?)
}

fn serve_chat_response() -> Result<(String, ProviderServer), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        read_request(&mut stream)?;
        let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"ok"}}]}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|error| error.to_string())
    });
    Ok((format!("http://{address}/v1"), handle))
}

fn read_request(stream: &mut TcpStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut bytes = [0_u8; 4096];
    stream
        .read(&mut bytes)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
