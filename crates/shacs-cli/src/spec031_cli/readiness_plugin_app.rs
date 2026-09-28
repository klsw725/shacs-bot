use shacs_app::app::{AppLifecycleState, AppRegistryEntry, AppRegistryStore};
use shacs_app::app_lifecycle::{AppProcessState, AppSupervisorJournal};
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
    let states = [plugins(bundle), apps(bundle)];
    let (state, code, summary) = if states.contains(&Spec031Availability::Blocked) {
        (
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "plugin or app owner reports a blocked target",
        )
    } else if states.contains(&Spec031Availability::Unavailable) {
        (
            Spec031Availability::Unavailable,
            Spec031ReasonCode::MissingExternalOwnerEvidence,
            "plugin or app owner lookup failed or required evidence is absent",
        )
    } else if states.contains(&Spec031Availability::Degraded) {
        (
            Spec031Availability::Degraded,
            Spec031ReasonCode::Degraded,
            "plugin or app owner reports a lifecycle or runtime limitation",
        )
    } else {
        (
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "plugin and app owner checks succeeded including empty collections",
        )
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

fn apps(bundle: &ConfigBundle) -> Spec031Availability {
    let store = AppRegistryStore::new(&bundle.context.data_dir);
    let Ok(Some(registry)) = store.load_existing() else {
        return Spec031Availability::Unavailable;
    };
    let journal = AppSupervisorJournal::new(bundle.context.runtime_subdir("apps"));
    let states: Vec<_> = registry
        .entries
        .values()
        .map(|entry| app(entry, &journal))
        .collect();
    if states.contains(&Spec031Availability::Blocked) {
        Spec031Availability::Blocked
    } else if states.contains(&Spec031Availability::Unavailable) {
        Spec031Availability::Unavailable
    } else if states.contains(&Spec031Availability::Degraded) {
        Spec031Availability::Degraded
    } else {
        Spec031Availability::Ready
    }
}

fn app(entry: &AppRegistryEntry, journal: &AppSupervisorJournal) -> Spec031Availability {
    let enabled = match entry.lifecycle_state {
        AppLifecycleState::Unavailable => return Spec031Availability::Blocked,
        AppLifecycleState::Uninstalling => return Spec031Availability::Degraded,
        AppLifecycleState::Enabled => true,
        AppLifecycleState::Installed | AppLifecycleState::Disabled => false,
    };
    if !entry.unavailable_reasons.is_empty() {
        return Spec031Availability::Blocked;
    }
    let Ok(replay) = journal.replay(&entry.app_id) else {
        return Spec031Availability::Unavailable;
    };
    let Some(receipt) = replay.receipts.last() else {
        return if enabled {
            Spec031Availability::Unavailable
        } else {
            Spec031Availability::Ready
        };
    };
    if receipt.app_id != entry.app_id {
        return Spec031Availability::Unavailable;
    }
    if !receipt.blockers.is_empty() {
        return Spec031Availability::Blocked;
    }
    match receipt.current_state {
        AppProcessState::Failed | AppProcessState::RecoveryNeeded => Spec031Availability::Blocked,
        AppProcessState::Starting | AppProcessState::Stopping => Spec031Availability::Degraded,
        AppProcessState::Running => {
            if receipt.completed && receipt.manifest_digest == entry.digest && enabled {
                Spec031Availability::Ready
            } else {
                Spec031Availability::Unavailable
            }
        }
        AppProcessState::Installed | AppProcessState::Stopped => {
            if enabled {
                Spec031Availability::Degraded
            } else {
                Spec031Availability::Ready
            }
        }
    }
}
