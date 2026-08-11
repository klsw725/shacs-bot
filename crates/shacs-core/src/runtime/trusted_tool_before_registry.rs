use super::{
    DiscoveredPlugin, PluginState, ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler,
    ToolBeforeOrderKey,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Default, Clone)]
pub struct TrustedToolBeforeRegistry {
    handlers: BTreeMap<String, Vec<Arc<dyn ToolBeforeHandler>>>,
}

impl TrustedToolBeforeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        extension_id: impl Into<String>,
        handler: Arc<dyn ToolBeforeHandler>,
    ) {
        let extension_id = extension_id.into();
        self.handlers
            .entry(extension_id.clone())
            .or_default()
            .push(Arc::new(RegisteredToolBeforeHandler {
                extension_id,
                handler,
            }));
    }

    pub fn active_handlers(&self, plugins: &[DiscoveredPlugin]) -> Vec<Arc<dyn ToolBeforeHandler>> {
        plugins
            .iter()
            .filter(|plugin| plugin.state == PluginState::Enabled && declares_tool_before(plugin))
            .flat_map(|plugin| self.handlers.get(&plugin.id).into_iter().flatten().cloned())
            .collect()
    }
}

struct RegisteredToolBeforeHandler {
    extension_id: String,
    handler: Arc<dyn ToolBeforeHandler>,
}

impl ToolBeforeHandler for RegisteredToolBeforeHandler {
    fn hook_ref(&self) -> &str {
        self.handler.hook_ref()
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.extension_id.clone())
    }

    fn timeout(&self) -> Duration {
        self.handler.timeout()
    }

    fn evaluate(&self, context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        self.handler.evaluate(context)
    }
}

fn declares_tool_before(plugin: &DiscoveredPlugin) -> bool {
    plugin
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.surfaces.get("hooks"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|hooks| {
            hooks
                .iter()
                .any(|hook| hook.as_str() == Some("tool:before"))
        })
}
