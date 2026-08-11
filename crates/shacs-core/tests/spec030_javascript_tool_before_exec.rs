use serde_json::json;
use shacs_config::{Config, ConfigContext};
use shacs_core::runtime::{
    discover_plugins, register_trusted_javascript_tool_before_handlers, AgentRunSpec, AgentRunner,
    PluginRuntimeHookAgentHook, PluginRuntimeSnapshot, TrustedToolBeforeRegistry,
};
use shacs_core::tools::{ExecTool, ToolRegistry};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "support/spec030_tool_before.rs"]
mod support;
use support::{approved_exec_context, QueueProvider};

#[test]
fn javascript_manifest_handler_allows_and_blocks_actual_exec() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let allowed_workspace = root.path().join("allowed-workspace");
    let blocked_workspace = root.path().join("blocked-workspace");
    fs::create_dir_all(&allowed_workspace)?;
    fs::create_dir_all(&blocked_workspace)?;

    let allowed_hook = load_hook(
        root.path().join("allowed-data"),
        &allowed_workspace,
        "function toolBefore() { return {allow: true}; }",
    )?;
    let blocked_hook = load_hook(
        root.path().join("blocked-data"),
        &blocked_workspace,
        "function toolBefore() { return {block: true, reason: 'fixture denied'}; }",
    )?;
    let allowed_marker = allowed_workspace.join("allowed.marker");
    let blocked_marker = blocked_workspace.join("blocked.marker");

    run_exec(&allowed_workspace, allowed_hook, &allowed_marker);
    run_exec(&blocked_workspace, blocked_hook, &blocked_marker);

    assert!(allowed_marker.exists());
    assert!(!blocked_marker.exists());
    Ok(())
}

fn load_hook(
    data_dir: PathBuf,
    workspace: &Path,
    source: &str,
) -> Result<Arc<PluginRuntimeHookAgentHook>, Box<dyn Error>> {
    let plugin_root = data_dir.join("plugins/javascript-hook");
    fs::create_dir_all(&plugin_root)?;
    fs::write(plugin_root.join("hook.js"), source)?;
    fs::write(
        plugin_root.join("plugin.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "name": "javascript-hook",
            "version": "0.1.0",
            "surfaces": {"hooks": ["tool:before"]},
            "entrypoints": {"trustedHooks": {"tool:before": "hook.js"}}
        }))?,
    )?;
    let mut config = Config::default();
    config.plugins.enabled.push("javascript-hook".to_owned());
    let discovery = discover_plugins(
        &config,
        &ConfigContext {
            config_path: data_dir.join("config.json"),
            data_dir,
            workspace: workspace.to_path_buf(),
        },
        &BTreeMap::<String, String>::new(),
    )?;
    let mut registry = TrustedToolBeforeRegistry::new();
    register_trusted_javascript_tool_before_handlers(&discovery.plugins, &mut registry);
    let handlers = registry.active_handlers(&discovery.plugins);
    Ok(Arc::new(
        PluginRuntimeHookAgentHook::new(PluginRuntimeSnapshot {
            plugins: Vec::new(),
            commands: Vec::new(),
            diagnostics: Vec::new(),
        })
        .with_trusted_handlers(handlers),
    ))
}

fn run_exec(workspace: &Path, hook: Arc<PluginRuntimeHookAgentHook>, marker: &Path) {
    let mut registry = ToolRegistry::new();
    registry.register(ExecTool::with_workspace(workspace));
    let marker_name = marker
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("marker must have a UTF-8 file name"));
    let command = format!("printf ran > {marker_name}");
    let client = QueueProvider::exec(&command);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "run"})],
        &registry,
        &client,
        "fake",
    );
    spec.max_iterations = 2;
    spec.agent_hook = Some(hook);
    spec.tool_context = approved_exec_context(&registry, &command);
    AgentRunner::new()
        .run(spec)
        .unwrap_or_else(|error| panic!("agent run failed: {error}"));
    assert!(client.saw_tool_message("exec-call-030", ""));
}
