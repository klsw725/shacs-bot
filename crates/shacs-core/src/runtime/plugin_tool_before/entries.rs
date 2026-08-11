use super::*;

pub(super) enum HandlerEntry<'a> {
    Command {
        key: ToolBeforeOrderKey,
        plugin: &'a PluginRuntimePlugin,
        hook: &'a PluginRuntimeHook,
    },
    Trusted {
        key: ToolBeforeOrderKey,
        handler: &'a Arc<dyn ToolBeforeHandler>,
    },
}

impl HandlerEntry<'_> {
    pub(super) const fn key(&self) -> &ToolBeforeOrderKey {
        match self {
            Self::Command { key, .. } | Self::Trusted { key, .. } => key,
        }
    }

    pub(super) fn hook_ref(&self) -> &str {
        match self {
            Self::Command { hook, .. } => &hook.plugin_id,
            Self::Trusted { handler, .. } => handler.hook_ref(),
        }
    }
}

pub(super) fn command_handlers(
    snapshot: &PluginRuntimeSnapshot,
) -> impl Iterator<Item = (&PluginRuntimePlugin, &PluginRuntimeHook)> {
    snapshot.plugins.iter().flat_map(|plugin| {
        plugin
            .hooks
            .iter()
            .filter(|hook| hook.event == PluginHookEvent::ToolBefore)
            .map(move |hook| (plugin, hook))
    })
}
