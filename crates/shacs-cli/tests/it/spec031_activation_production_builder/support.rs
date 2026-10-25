use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{
    Config, ConfigBundle, ConfigContext, ProviderConfig, TrustedResourceConfig, TrustedResourceKind,
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
use std::path::{Path, PathBuf};
use std::thread;

type ProviderServer = thread::JoinHandle<Result<(), String>>;

pub fn run() -> Result<ExecutionSnapshot, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let data_dir = root.path().canonicalize()?;
    let workspace = data_dir.join("workspace");
    std::fs::create_dir_all(&workspace)?;
    crate::spec043_leftover_apps::copy_into(&data_dir)?;
    let (bundle, app_digest) = registry_in_place(&data_dir, "demo.clock")?;
    let canonical_resource = bundle.join("README.md").canonicalize()?;
    let content_digest = format!("{:x}", Sha256::digest(std::fs::read(&canonical_resource)?));
    let context = ConfigContext {
        config_path: data_dir.join("config.json"),
        data_dir: data_dir.clone(),
        workspace: workspace.clone(),
    };
    seed_activation(&context, &canonical_resource, &content_digest, &app_digest)?;
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

/// Rewrites the copied registry's bundle paths to this data dir, as if the
/// leftover data had never moved, and returns the app's bundle and digest.
fn registry_in_place(data_dir: &Path, app_id: &str) -> Result<(PathBuf, String), Box<dyn Error>> {
    let registry_path = data_dir.join("apps/registry.json");
    let text = std::fs::read_to_string(&registry_path)?;
    let entry = &serde_json::from_str::<serde_json::Value>(&text)?["entries"][app_id];
    let suffix = format!("/apps/{app_id}.shacsapp");
    let original = entry["bundlePath"]
        .as_str()
        .and_then(|path| path.strip_suffix(&suffix))
        .ok_or("fixture bundle path")?;
    let digest = entry["digest"].as_str().ok_or("fixture digest")?.to_owned();
    std::fs::write(
        &registry_path,
        text.replace(original, &data_dir.to_string_lossy()),
    )?;
    Ok((data_dir.join(format!("apps/{app_id}.shacsapp")), digest))
}

fn seed_activation(
    context: &ConfigContext,
    source: &Path,
    content_digest: &str,
    manifest_digest: &str,
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
        status: ActivationStatus::Active,
        reason: ActivationReason::Activated,
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
