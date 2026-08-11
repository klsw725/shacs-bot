use super::{
    DiscoveredPlugin, PluginState, ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler,
    ToolBeforeOrderKey, TrustedToolBeforeRegistry,
};
use boa_engine::{js_string, Context, JsValue, Source};
use deno_ast::{
    parse_program, EmitOptions, MediaType, ModuleSpecifier, ParseParams, TranspileModuleOptions,
    TranspileOptions,
};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const LOOP_ITERATION_LIMIT: u64 = 10_000;
const RECURSION_LIMIT: usize = 64;
const HANDLER_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entrypoints {
    trusted_hooks: TrustedHooks,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedHooks {
    #[serde(rename = "tool:before")]
    tool_before: String,
}

pub fn register_trusted_javascript_tool_before_handlers(
    plugins: &[DiscoveredPlugin],
    registry: &mut TrustedToolBeforeRegistry,
) {
    for plugin in plugins
        .iter()
        .filter(|plugin| plugin.state == PluginState::Enabled)
    {
        let Some(manifest) = &plugin.manifest else {
            continue;
        };
        let Ok(entrypoints) = serde_json::from_value::<Entrypoints>(manifest.entrypoints.clone())
        else {
            continue;
        };
        registry.register(
            &plugin.id,
            Arc::new(JavaScriptToolBeforeHandler::load(
                &plugin.id,
                &plugin.root,
                &entrypoints.trusted_hooks.tool_before,
            )),
        );
    }
}

struct JavaScriptToolBeforeHandler {
    hook_ref: String,
    source: Result<String, String>,
}

impl JavaScriptToolBeforeHandler {
    fn load(plugin_id: &str, root: &Path, entrypoint: &str) -> Self {
        Self {
            hook_ref: format!("{plugin_id}:tool:before:trusted-js"),
            source: load_source(root, entrypoint),
        }
    }

    fn evaluate_source(
        &self,
        context: &ToolBeforeContext<'_>,
    ) -> Result<ToolBeforeDecision, String> {
        let source = self.source.as_ref().map_err(Clone::clone)?;
        let mut engine = Context::default();
        engine
            .runtime_limits_mut()
            .set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
        engine
            .runtime_limits_mut()
            .set_recursion_limit(RECURSION_LIMIT);
        engine
            .eval(Source::from_bytes(source))
            .map_err(|error| error.to_string())?;
        let function = engine
            .global_object()
            .get(js_string!("toolBefore"), &mut engine)
            .map_err(|error| error.to_string())?
            .as_callable()
            .ok_or_else(|| "entrypoint must define callable toolBefore(context)".to_owned())?;
        let call = context.call();
        let input = JsValue::from_json(
            &json!({
                "id": call.id,
                "name": call.name,
                "arguments": call.arguments,
            }),
            &mut engine,
        )
        .map_err(|error| error.to_string())?;
        let output = function
            .call(&JsValue::undefined(), &[input], &mut engine)
            .map_err(|error| error.to_string())?
            .to_json(&mut engine)
            .map_err(|error| error.to_string())?;
        Ok(output.map_or(ToolBeforeDecision::InvalidOutput, parse_decision))
    }
}

impl ToolBeforeHandler for JavaScriptToolBeforeHandler {
    fn hook_ref(&self) -> &str {
        &self.hook_ref
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.hook_ref.clone())
    }

    fn timeout(&self) -> Duration {
        HANDLER_TIMEOUT
    }

    fn evaluate(&self, context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        self.evaluate_source(context)
            .unwrap_or_else(|error| std::panic::panic_any(error))
    }
}

fn load_source(root: &Path, entrypoint: &str) -> Result<String, String> {
    let relative = Path::new(entrypoint);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("trusted hook entrypoint must be a relative path without traversal".to_owned());
    }
    let media_type = match relative.extension().and_then(|value| value.to_str()) {
        Some("js") | Some("mjs") => MediaType::JavaScript,
        Some("ts") => MediaType::TypeScript,
        _ => return Err("trusted hook entrypoint must end in .js, .mjs, or .ts".to_owned()),
    };
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let path = fs::canonicalize(root.join(relative)).map_err(|error| error.to_string())?;
    if !path.starts_with(&canonical_root) || !path.is_file() {
        return Err("trusted hook entrypoint must be a file inside the plugin root".to_owned());
    }
    let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    match media_type {
        MediaType::JavaScript => Ok(source),
        MediaType::TypeScript => transpile_typescript(path, source),
        _ => Err("unsupported trusted hook media type".to_owned()),
    }
}

fn transpile_typescript(path: PathBuf, source: String) -> Result<String, String> {
    let specifier = ModuleSpecifier::from_file_path(path)
        .map_err(|()| "trusted hook path cannot be represented as a file URL".to_owned())?;
    parse_program(ParseParams {
        specifier,
        text: source.into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    })
    .map_err(|error| error.to_string())?
    .transpile(
        &TranspileOptions::default(),
        &TranspileModuleOptions::default(),
        &EmitOptions::default(),
    )
    .map(|output| output.into_source().text)
    .map_err(|error| error.to_string())
}

fn parse_decision(output: serde_json::Value) -> ToolBeforeDecision {
    let allow = output.get("allow").and_then(serde_json::Value::as_bool);
    let block = output.get("block").and_then(serde_json::Value::as_bool);
    let reason = output.get("reason").and_then(serde_json::Value::as_str);
    match (
        allow,
        block,
        reason,
        output.as_object().map(serde_json::Map::len),
    ) {
        (Some(true), None, None, Some(1)) => ToolBeforeDecision::Allow,
        (None, Some(true), Some(reason), Some(2)) => ToolBeforeDecision::Block {
            reason: reason.to_owned(),
        },
        _ => ToolBeforeDecision::InvalidOutput,
    }
}
