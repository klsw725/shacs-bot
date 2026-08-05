use crate::runtime::{
    build_plugin_surface_projection, DiscoveredPlugin, PluginState, PluginSurfaceDiagnostic,
};
use sha2::{Digest, Sha256};
use shacs_projection::{
    spec031_extension_catalog, spec031_extension_diagnostic, Spec031ExtensionCatalogProjection,
    Spec031ExtensionDiagnostic, Spec031ExtensionDiagnosticSeverity, Spec031ExtensionEnabledState,
    Spec031ExtensionProjection, Spec031ExtensionReadiness, Spec031ExtensionReason,
    Spec031ExtensionSurfaceKind, Spec031ExtensionSurfaceProjection,
};

pub fn build_spec031_extension_projection(
    plugins: &[DiscoveredPlugin],
) -> Spec031ExtensionCatalogProjection {
    let surface_projection = build_plugin_surface_projection(plugins);
    let extensions = plugins
        .iter()
        .map(|plugin| {
            let surface_diagnostics = surface_projection
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.plugin_id == plugin.id)
                .cloned()
                .collect::<Vec<_>>();
            extension_projection(plugin, &surface_diagnostics)
        })
        .collect();
    spec031_extension_catalog(extensions)
}

fn extension_projection(
    plugin: &DiscoveredPlugin,
    surface_diagnostics: &[PluginSurfaceDiagnostic],
) -> Spec031ExtensionProjection {
    let diagnostics = plugin_diagnostics(plugin, surface_diagnostics);
    let (readiness, reason) = extension_readiness(plugin.state, diagnostics.is_empty());
    Spec031ExtensionProjection {
        extension_ref: extension_ref(plugin),
        label: plugin.id.clone(),
        owner_source: plugin.source.as_str().to_owned(),
        enabled_state: enabled_state(plugin.state),
        readiness,
        reason,
        diagnostics,
        surfaces: extension_surfaces(plugin),
    }
}

fn extension_readiness(
    state: PluginState,
    diagnostics_empty: bool,
) -> (Spec031ExtensionReadiness, Spec031ExtensionReason) {
    match state {
        PluginState::Enabled if diagnostics_empty => (
            Spec031ExtensionReadiness::Ready,
            Spec031ExtensionReason::Ready,
        ),
        PluginState::Enabled => (
            Spec031ExtensionReadiness::Degraded,
            Spec031ExtensionReason::Degraded,
        ),
        PluginState::Blocked => (
            Spec031ExtensionReadiness::Blocked,
            Spec031ExtensionReason::Blocked,
        ),
        PluginState::Disabled | PluginState::NotEnabled => (
            Spec031ExtensionReadiness::Unavailable,
            Spec031ExtensionReason::Unavailable,
        ),
    }
}

fn enabled_state(state: PluginState) -> Spec031ExtensionEnabledState {
    match state {
        PluginState::Enabled | PluginState::Blocked => Spec031ExtensionEnabledState::Enabled,
        PluginState::Disabled => Spec031ExtensionEnabledState::Disabled,
        PluginState::NotEnabled => Spec031ExtensionEnabledState::NotEnabled,
    }
}

fn plugin_diagnostics(
    plugin: &DiscoveredPlugin,
    surface_diagnostics: &[PluginSurfaceDiagnostic],
) -> Vec<Spec031ExtensionDiagnostic> {
    let mut diagnostics = plugin
        .block_reasons
        .iter()
        .map(|reason| {
            spec031_extension_diagnostic(
                Spec031ExtensionDiagnosticSeverity::Error,
                reason.as_str(),
                reason.as_str(),
            )
        })
        .chain(plugin.diagnostics.iter().map(|message| {
            spec031_extension_diagnostic(
                Spec031ExtensionDiagnosticSeverity::Error,
                "discovery_diagnostic",
                message,
            )
        }))
        .chain(surface_diagnostics.iter().map(|diagnostic| {
            spec031_extension_diagnostic(
                Spec031ExtensionDiagnosticSeverity::Warning,
                &diagnostic.code,
                &diagnostic.message,
            )
        }))
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then(left.message.cmp(&right.message))
    });
    diagnostics
}

fn extension_surfaces(plugin: &DiscoveredPlugin) -> Vec<Spec031ExtensionSurfaceProjection> {
    let Some(manifest) = plugin.manifest.as_ref() else {
        return Vec::new();
    };
    let mut surfaces = Vec::new();
    push_surfaces(
        &mut surfaces,
        Spec031ExtensionSurfaceKind::Tool,
        &manifest.surfaces,
        "tools",
    );
    push_surfaces(
        &mut surfaces,
        Spec031ExtensionSurfaceKind::Hook,
        &manifest.surfaces,
        "hooks",
    );
    push_surfaces(
        &mut surfaces,
        Spec031ExtensionSurfaceKind::Skill,
        &manifest.surfaces,
        "skills",
    );
    push_surfaces(
        &mut surfaces,
        Spec031ExtensionSurfaceKind::Command,
        &manifest.surfaces,
        "commands",
    );
    push_surfaces(
        &mut surfaces,
        Spec031ExtensionSurfaceKind::Mcp,
        &manifest.surfaces,
        "mcp",
    );
    surfaces.sort_by(|left, right| left.name.cmp(&right.name));
    surfaces
}

fn push_surfaces(
    surfaces: &mut Vec<Spec031ExtensionSurfaceProjection>,
    kind: Spec031ExtensionSurfaceKind,
    manifest_surfaces: &serde_json::Value,
    key: &str,
) {
    for name in names_from_value(manifest_surfaces.get(key)) {
        surfaces.push(Spec031ExtensionSurfaceProjection {
            kind,
            name,
            execution_enabled: false,
        });
    }
}

fn names_from_value(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(serde_json::Value::Object(object)) => object.keys().cloned().collect(),
        Some(serde_json::Value::String(name)) if !name.trim().is_empty() => {
            vec![name.trim().to_owned()]
        }
        Some(_) | None => Vec::new(),
    }
}

fn extension_ref(plugin: &DiscoveredPlugin) -> String {
    let stable = plugin.digest.as_deref().unwrap_or(plugin.id.as_str());
    let mut hasher = Sha256::new();
    hasher.update(plugin.source.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(stable.as_bytes());
    format!("ext_sha256:{}", sha256_hex(&hasher.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
