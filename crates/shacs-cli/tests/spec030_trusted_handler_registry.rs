use serde_json::{json, Map};
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{Config, ConfigBundle, ConfigContext};
use shacs_core::runtime::{
    ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler, ToolBeforeOrderKey,
    TrustedToolBeforeRegistry,
};
use shacs_projection::{HookDenialReason, HookDiagnosticKind, HookRuntimeStatus};
use shacs_providers::{GenerationSettings, LlmResponse, ProviderRequest, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const EXTENSION_ID: &str = "trusted-extension";

struct RegisteredHandler {
    calls: Arc<Mutex<Vec<String>>>,
}

impl ToolBeforeHandler for RegisteredHandler {
    fn hook_ref(&self) -> &str {
        "trusted-extension:tool:before"
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(EXTENSION_ID)
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(10)
    }

    fn evaluate(&self, context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        let call_id = context.call().id.clone();
        self.calls
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(call_id.clone());
        match call_id.as_str() {
            "block" => ToolBeforeDecision::Block {
                reason: "fixture block".to_owned(),
            },
            "timeout" => {
                std::thread::sleep(Duration::from_millis(40));
                ToolBeforeDecision::Allow
            }
            "allow" | "untrusted" | "unconfigured" => ToolBeforeDecision::Allow,
            _ => ToolBeforeDecision::Allow,
        }
    }
}

#[test]
fn production_registry_activates_only_configured_trusted_workspace_handler(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    write_manifest(&workspace)?;
    let responses = root.path().join("responses.json");
    fs::write(
        &responses,
        serde_json::to_vec(&responses_for(&[
            "allow",
            "block",
            "timeout",
            "untrusted",
            "unconfigured",
        ]))?,
    )?;
    std::env::set_var("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", &responses);

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut registry = TrustedToolBeforeRegistry::new();
    registry.register(
        EXTENSION_ID,
        Arc::new(RegisteredHandler {
            calls: calls.clone(),
        }),
    );

    let active = AgentLoopChatCompletionAdapter::from_bundle_with_trusted_extensions(
        bundle(root.path(), &workspace, true, true),
        false,
        registry.clone(),
    )?;
    run_turn(&active, "allow", "active:allow")?;
    run_turn(&active, "block", "active:block")?;
    run_turn(&active, "timeout", "active:timeout")?;

    let untrusted = AgentLoopChatCompletionAdapter::from_bundle_with_trusted_extensions(
        bundle(root.path(), &workspace, true, false),
        false,
        registry.clone(),
    )?;
    run_turn(&untrusted, "untrusted", "inactive:untrusted")?;

    let unconfigured = AgentLoopChatCompletionAdapter::from_bundle_with_trusted_extensions(
        bundle(root.path(), &workspace, false, true),
        false,
        registry,
    )?;
    run_turn(&unconfigured, "unconfigured", "inactive:unconfigured")?;
    let empty_root = root.path().join("empty-registry");
    fs::create_dir_all(&empty_root)?;
    let empty = AgentLoopChatCompletionAdapter::from_bundle(
        bundle(&empty_root, &workspace, true, true),
        false,
    )?;
    std::env::remove_var("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES");

    assert_eq!(
        *calls.lock().unwrap_or_else(|error| error.into_inner()),
        ["allow", "block", "timeout"]
    );
    let hooks = active.trusted_runtime_projection().hooks().clone();
    assert_eq!(hooks.status, HookRuntimeStatus::Active);
    assert_eq!(hooks.registered_handlers, 1);
    assert!(hooks
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == HookDiagnosticKind::Timeout));
    assert!(
        hooks
            .recent_denials
            .iter()
            .any(|denial| denial.call_ref == "block"
                && denial.reason == HookDenialReason::ExtensionBlocked),
        "{hooks:?}"
    );
    assert_eq!(
        untrusted
            .trusted_runtime_projection()
            .hooks()
            .registered_handlers,
        0
    );
    assert_eq!(
        unconfigured
            .trusted_runtime_projection()
            .hooks()
            .registered_handlers,
        0
    );
    assert_eq!(
        empty
            .trusted_runtime_projection()
            .hooks()
            .registered_handlers,
        0
    );
    Ok(())
}

fn bundle(root: &Path, workspace: &Path, enabled: bool, trusted: bool) -> ConfigBundle {
    let mut config = Config::default();
    config.agents.defaults.provider = "custom".to_owned();
    config.agents.defaults.model = "test-model".to_owned();
    config.agents.defaults.max_tool_iterations = 2;
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    if enabled {
        config.plugins.enabled.push(EXTENSION_ID.to_owned());
    }
    if trusted {
        config
            .plugins
            .trusted_workspaces
            .push(workspace.to_string_lossy().to_string());
    }
    let data_dir = root.join(format!("data-{enabled}-{trusted}"));
    ConfigBundle {
        config,
        context: ConfigContext {
            config_path: data_dir.join("config.json"),
            data_dir,
            workspace: workspace.to_path_buf(),
        },
        migrations: Vec::new(),
    }
}

fn run_turn(
    adapter: &AgentLoopChatCompletionAdapter,
    content: &str,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    adapter.complete_chat(ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role": "user", "content": content})],
            tools: Vec::new(),
            model: "test-model".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: Some("test-model".to_owned()),
        session_key: session_key.to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    })?;
    Ok(())
}

fn responses_for(call_ids: &[&str]) -> Vec<LlmResponse> {
    call_ids
        .iter()
        .flat_map(|call_id| {
            [
                LlmResponse {
                    finish_reason: "tool_calls".to_owned(),
                    tool_calls: vec![ToolCallRequest::new(
                        *call_id,
                        "list_dir",
                        Map::from_iter([("path".to_owned(), json!("."))]),
                    )],
                    ..LlmResponse::default()
                },
                LlmResponse {
                    content: Some("done".to_owned()),
                    ..LlmResponse::default()
                },
            ]
        })
        .collect()
}

fn write_manifest(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let path = workspace.join(".shacs-bot/plugins/trusted/plugin.json");
    fs::create_dir_all(path.parent().ok_or("manifest parent missing")?)?;
    fs::write(
        path,
        serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "name": EXTENSION_ID,
            "version": "0.1.0",
            "surfaces": {"hooks": ["tool:before"]},
            "requiresEnv": [],
            "requiresConfig": [],
            "permissions": {},
            "entrypoints": {},
            "assets": []
        }))?,
    )?;
    Ok(())
}
