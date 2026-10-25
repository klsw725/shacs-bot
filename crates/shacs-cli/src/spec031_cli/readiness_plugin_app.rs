use shacs_config::{ConfigBundle, ProcessEnv};
use shacs_core::runtime::{
    build_plugin_runtime_snapshot, build_plugin_surface_projection, discover_plugins,
    plugin_runtime_tools, PluginState,
};
use shacs_projection::{
    Spec031Availability, Spec031ReadinessComponentKind, Spec031ReadinessObservation,
    Spec031ReasonCode,
};
use std::{fs, io};

pub(crate) fn observe(bundle: &ConfigBundle) -> Result<Spec031ReadinessObservation, String> {
    let (state, code, summary) = match plugins(bundle) {
        Spec031Availability::Blocked => (
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "plugin owner reports a blocked target",
        ),
        Spec031Availability::Unavailable => (
            Spec031Availability::Unavailable,
            Spec031ReasonCode::MissingExternalOwnerEvidence,
            "plugin owner lookup failed or required evidence is absent",
        ),
        Spec031Availability::Degraded => (
            Spec031Availability::Degraded,
            Spec031ReasonCode::Degraded,
            "plugin owner reports a runtime limitation",
        ),
        Spec031Availability::Ready | Spec031Availability::Unknown => (
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "plugin owner checks succeeded including empty collections",
        ),
    };
    super::readiness_observation::observation(
        Spec031ReadinessComponentKind::PluginApp,
        state,
        code,
        summary,
    )
}

fn plugins(bundle: &ConfigBundle) -> Spec031Availability {
    let mut available = false;
    for root in [
        bundle.context.data_dir.join("plugins"),
        bundle.context.workspace.join(".shacs-bot/plugins"),
    ] {
        match fs::symlink_metadata(root) {
            Ok(_) => available = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Spec031Availability::Unavailable,
        }
    }
    if !available {
        return Spec031Availability::Unavailable;
    }
    let Ok(discovery) = discover_plugins(&bundle.config, &bundle.context, &ProcessEnv) else {
        return Spec031Availability::Unavailable;
    };
    if discovery
        .plugins
        .iter()
        .any(|plugin| plugin.state == PluginState::Blocked)
    {
        return Spec031Availability::Blocked;
    }
    if bundle
        .config
        .plugins
        .enabled
        .iter()
        .filter(|name| !bundle.config.plugins.disabled.contains(name))
        .any(|name| !discovery.plugins.iter().any(|plugin| &plugin.id == name))
    {
        return Spec031Availability::Unavailable;
    }
    let runtime = build_plugin_runtime_snapshot(&discovery.plugins);
    let mut diagnostics = runtime.diagnostics;
    let _tools = plugin_runtime_tools(&discovery.plugins, &mut diagnostics);
    let surfaces = build_plugin_surface_projection(&discovery.plugins);
    if !diagnostics.is_empty()
        || surfaces.diagnostics.iter().any(|diagnostic| {
            discovery.plugins.iter().any(|plugin| {
                plugin.id == diagnostic.plugin_id && plugin.state == PluginState::Enabled
            })
        })
    {
        Spec031Availability::Degraded
    } else {
        Spec031Availability::Ready
    }
}
