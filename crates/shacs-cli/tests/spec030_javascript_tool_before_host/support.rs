use serde_json::{json, Map};
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{save_config_to_path, Config, ConfigBundle, ConfigContext};
use shacs_projection::{HookDiagnosticKind, HookRuntimeStatus};
use shacs_providers::{GenerationSettings, LlmResponse, ProviderRequest, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum PluginLocation {
    UserData,
    Workspace,
}

#[derive(Clone, Copy)]
pub enum Activation {
    Enabled,
    Disabled,
}

pub struct Scenario<'a> {
    pub id: &'a str,
    pub location: PluginLocation,
    pub activation: Activation,
    pub extension: &'a str,
    pub source: &'a str,
}

impl<'a> Scenario<'a> {
    pub const fn enabled_user_data(id: &'a str, extension: &'a str, source: &'a str) -> Self {
        Self {
            id,
            location: PluginLocation::UserData,
            activation: Activation::Enabled,
            extension,
            source,
        }
    }
}

pub struct ScenarioResult {
    pub marker: PathBuf,
    pub status: HookRuntimeStatus,
    pub registered_handlers: u32,
    pub diagnostics: Vec<HookDiagnosticKind>,
}

pub fn run_scenario(
    root: &Path,
    responses: &Path,
    scenario: Scenario<'_>,
) -> Result<ScenarioResult, Box<dyn Error>> {
    let scenario_root = root.join(scenario.id);
    let workspace = scenario_root.join("workspace");
    let data_dir = scenario_root.join("data");
    fs::create_dir_all(&workspace)?;
    let plugin_root = match scenario.location {
        PluginLocation::UserData => data_dir.join("plugins").join(scenario.id),
        PluginLocation::Workspace => workspace.join(".shacs-bot/plugins").join(scenario.id),
    };
    fs::create_dir_all(&plugin_root)?;
    let entrypoint = format!("hook.{}", scenario.extension);
    fs::write(plugin_root.join(&entrypoint), scenario.source)?;
    fs::write(
        plugin_root.join("plugin.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "name": scenario.id,
            "version": "0.1.0",
            "surfaces": {"hooks": ["tool:before"]},
            "entrypoints": {"trustedHooks": {"tool:before": entrypoint}}
        }))?,
    )?;
    let mut config = Config::default();
    config.agents.defaults.provider = "custom".to_owned();
    config.agents.defaults.model = "test-model".to_owned();
    config.agents.defaults.max_tool_iterations = 2;
    config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    config.permissions = serde_json::from_value(json!({"mode": "accept_edits"}))?;
    match scenario.activation {
        Activation::Enabled => config.plugins.enabled.push(scenario.id.to_owned()),
        Activation::Disabled => config.plugins.disabled.push(scenario.id.to_owned()),
    }
    let config_path = data_dir.join("config.json");
    save_config_to_path(&config, &config_path)?;
    let bundle = ConfigBundle {
        config,
        context: ConfigContext {
            config_path,
            data_dir: data_dir.clone(),
            workspace: workspace.clone(),
        },
        migrations: Vec::new(),
    };
    let marker = workspace.join(format!("{}.marker", scenario.id));
    let marker_name = marker
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("scenario.marker");
    fs::write(
        responses,
        serde_json::to_vec(&vec![
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "tool-call",
                    "write_file",
                    Map::from_iter([
                        ("path".to_owned(), json!(marker_name)),
                        ("content".to_owned(), json!("executed")),
                    ]),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("done".to_owned()),
                ..LlmResponse::default()
            },
        ])?,
    )?;
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(bundle, true)?;
    adapter.complete_chat(ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role": "user", "content": scenario.id})],
            tools: Vec::new(),
            model: "test-model".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: Some("test-model".to_owned()),
        session_key: format!("js-host:{}", scenario.id),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    })?;
    let hooks = adapter.trusted_runtime_projection().hooks().clone();
    Ok(ScenarioResult {
        marker,
        status: hooks.status,
        registered_handlers: hooks.registered_handlers,
        diagnostics: hooks
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.kind)
            .collect(),
    })
}

pub struct ScopedEnv {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl ScopedEnv {
    pub fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
