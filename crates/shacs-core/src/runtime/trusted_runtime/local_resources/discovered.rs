use crate::runtime::trusted_resources::{
    ResourceCandidate, ResourceDiagnostic, ResourceDiagnosticKind, ResourceLoadCheck,
};
use crate::runtime::{
    discover_context_files, discover_plugins, ContextFileDiscoveryOptions, ContextFileReadStatus,
    ContextFileSource, PluginManifestSource, PluginState,
};
use shacs_config::{ConfigBundle, ProcessEnv};
use shacs_projection::{
    ResourceActivation, ResourceKind, ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};
use shacs_skills::{
    discover_skill_registry, SkillRegistryOptions, SkillRegistryStatus, SkillSourceKind,
};

pub fn candidates(bundle: &ConfigBundle) -> Vec<ResourceCandidate> {
    let plugins = discover_plugins(&bundle.config, &bundle.context, &ProcessEnv)
        .map(|discovery| discovery.plugins)
        .unwrap_or_default();
    let mut skill_options = SkillRegistryOptions::new(bundle.context.workspace.clone());
    skill_options.user_skills_dir = Some(bundle.context.data_dir.join("skills"));
    skill_options.plugin_roots = plugins
        .iter()
        .filter(|plugin| plugin.state == PluginState::Enabled)
        .map(|plugin| plugin.root.join("skills"))
        .collect();
    skill_options.plugin_roots_enabled = true;
    let mut candidates = discover_skill_registry(skill_options)
        .map(|registry| {
            registry
                .entries
                .into_iter()
                .filter_map(|entry| {
                    let path = entry.descriptor.source_path?;
                    let (source, precedence) = skill_source(entry.descriptor.source_kind);
                    let activation = if entry.status == SkillRegistryStatus::Active {
                        plugin_resource_activation(&path, &plugins)
                            .unwrap_or(ResourceActivation::TrustedWorkspace)
                    } else {
                        ResourceActivation::Inactive
                    };
                    Some(ResourceCandidate {
                        resource_ref: format!("skill:{}", entry.descriptor.name),
                        kind: ResourceKind::Skill,
                        source,
                        precedence,
                        path,
                        activation,
                        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
                        load_check: ResourceLoadCheck::Content,
                        diagnostics: diagnostics(
                            &format!("skill:{}", entry.descriptor.name),
                            entry.diagnostics,
                        ),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    candidates.extend(plugins.into_iter().map(|plugin| {
        let source = match plugin.source {
            PluginManifestSource::UserData => ResourceSource::User,
            PluginManifestSource::WorkspaceLocal => ResourceSource::Project,
        };
        let precedence = match plugin.source {
            PluginManifestSource::UserData => ResourcePrecedence::UserAuto,
            PluginManifestSource::WorkspaceLocal => ResourcePrecedence::TrustedProjectAuto,
        };
        ResourceCandidate {
            resource_ref: format!("extension:{}", plugin.id),
            kind: ResourceKind::Extension,
            source,
            precedence,
            path: plugin.manifest_path,
            activation: match (plugin.state, plugin.source) {
                (PluginState::Enabled, PluginManifestSource::UserData) => {
                    ResourceActivation::Explicit
                }
                (PluginState::Enabled, PluginManifestSource::WorkspaceLocal) => {
                    ResourceActivation::TrustedWorkspace
                }
                (PluginState::NotEnabled | PluginState::Disabled | PluginState::Blocked, _) => {
                    ResourceActivation::Inactive
                }
            },
            trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            load_check: ResourceLoadCheck::Content,
            diagnostics: diagnostics(&format!("extension:{}", plugin.id), plugin.diagnostics),
        }
    }));
    let context = discover_context_files(
        &bundle.context.workspace,
        ContextFileDiscoveryOptions::default(),
    );
    candidates.extend(context.entries.into_iter().map(|entry| {
        let reason = entry.reason.into_iter().collect::<Vec<_>>();
        ResourceCandidate {
            resource_ref: format!("context:{}", entry.path.to_string_lossy()),
            kind: ResourceKind::Context,
            source: ResourceSource::Project,
            precedence: match entry.source {
                ContextFileSource::ConfiguredExtra => ResourcePrecedence::ProjectConfigured,
                ContextFileSource::DefaultCandidate => ResourcePrecedence::TrustedProjectAuto,
            },
            path: entry.path,
            activation: ResourceActivation::TrustedWorkspace,
            trusted_code_disclosure: TrustedCodeDisclosure::NotExecutable,
            load_check: if matches!(
                entry.status,
                ContextFileReadStatus::Included | ContextFileReadStatus::Truncated
            ) {
                ResourceLoadCheck::Content
            } else {
                ResourceLoadCheck::Unsupported {
                    reason: "context discovery rejected the candidate".to_owned(),
                }
            },
            diagnostics: diagnostics("context", reason),
        }
    }));
    candidates
}

fn plugin_resource_activation(
    path: &std::path::Path,
    plugins: &[crate::runtime::DiscoveredPlugin],
) -> Option<ResourceActivation> {
    plugins.iter().find_map(|plugin| {
        path.starts_with(plugin.root.join("skills"))
            .then_some(match plugin.source {
                PluginManifestSource::UserData => ResourceActivation::Explicit,
                PluginManifestSource::WorkspaceLocal => ResourceActivation::TrustedWorkspace,
            })
    })
}

fn diagnostics(resource_ref: &str, reasons: Vec<String>) -> Vec<ResourceDiagnostic> {
    reasons
        .into_iter()
        .map(|reason| ResourceDiagnostic {
            resource_ref: resource_ref.to_owned(),
            kind: ResourceDiagnosticKind::LoadFailed,
            path: None,
            reason,
        })
        .collect()
}

const fn skill_source(source: SkillSourceKind) -> (ResourceSource, ResourcePrecedence) {
    match source {
        SkillSourceKind::VirtualBuiltin | SkillSourceKind::MaterializedBuiltin => {
            (ResourceSource::Builtin, ResourcePrecedence::Builtin)
        }
        SkillSourceKind::UserGlobal => (ResourceSource::User, ResourcePrecedence::UserAuto),
        SkillSourceKind::WorkspaceLegacy | SkillSourceKind::WorkspaceLocal => (
            ResourceSource::Project,
            ResourcePrecedence::TrustedProjectAuto,
        ),
        SkillSourceKind::PluginProvided => (ResourceSource::Package, ResourcePrecedence::Package),
    }
}
