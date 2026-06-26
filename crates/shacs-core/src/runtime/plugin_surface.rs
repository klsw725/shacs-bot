use crate::runtime::{
    evaluate_inherited_ceiling, BoundaryPermissionViolation, DiscoveredPlugin,
    InheritedPermissionContext, PermissionCeilingSnapshot, PermissionMode, PluginState,
    RuntimeBoundaryOrigin, SafetyCapability,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_eval::evaluator::{EvidenceKind, EvidenceRef, RedactionStatus};
use shacs_redaction::redact_string;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSurfaceProjection {
    pub plugins: Vec<PluginDescriptor>,
    pub tools: Vec<PluginToolDescriptor>,
    pub hooks: Vec<PluginHookDescriptor>,
    pub skills: Vec<PluginSkillDescriptor>,
    pub commands: Vec<PluginCommandDescriptor>,
    pub mcp: Vec<PluginMcpDescriptor>,
    pub diagnostics: Vec<PluginSurfaceDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub id: String,
    pub state: String,
    pub source: String,
    pub active_surface_count: usize,
    pub declared_surface_count: usize,
    pub secret_refs: Vec<PluginSecretRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginToolDescriptor {
    pub plugin_id: String,
    pub name: String,
    pub description: Option<String>,
    pub execution_enabled: bool,
    pub deferrable: bool,
    pub provider_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHookDescriptor {
    pub plugin_id: String,
    pub event: String,
    pub execution_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSkillDescriptor {
    pub plugin_id: String,
    pub name: String,
    pub namespace: String,
    pub execution_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCommandDescriptor {
    pub plugin_id: String,
    pub name: String,
    pub backend: String,
    pub execution_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMcpDescriptor {
    pub plugin_id: String,
    pub name: String,
    pub backend: String,
    pub execution_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSecretRefKind {
    Env,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSecretRef {
    pub kind: PluginSecretRefKind,
    pub name: String,
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSurfaceDiagnostic {
    pub plugin_id: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissionCeilingRequest {
    pub plugin_id: String,
    pub parent_mode: PermissionMode,
    pub capability_ceiling: Vec<SafetyCapability>,
    pub requested_mode: PermissionMode,
    pub requested_capabilities: Vec<SafetyCapability>,
    pub approved_scope_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissionCeilingDecision {
    pub plugin_id: String,
    pub allowed: bool,
    pub violations: Vec<BoundaryPermissionViolation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginReplayRejection {
    pub plugin_id: String,
    pub accepted: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSpec025ReleaseEvidenceBucket {
    DiscoveryManifestGate,
    DescriptorOnlySurfaces,
    HookPolicyValidation,
    PermissionCeiling,
    SecretRedaction,
    ReplayRejection,
    ReleaseEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginSpec025ReleaseEvidence {
    pub buckets: Vec<PluginSpec025ReleaseEvidenceBucket>,
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginSpec025ReleaseEvidenceChecklist {
    pub required_buckets: Vec<PluginSpec025ReleaseEvidenceBucket>,
    pub missing_buckets: Vec<PluginSpec025ReleaseEvidenceBucket>,
    pub complete: bool,
    pub evidence_refs: Vec<EvidenceRef>,
}

pub fn build_plugin_surface_projection(plugins: &[DiscoveredPlugin]) -> PluginSurfaceProjection {
    let mut projection = PluginSurfaceProjection {
        plugins: Vec::new(),
        tools: Vec::new(),
        hooks: Vec::new(),
        skills: Vec::new(),
        commands: Vec::new(),
        mcp: Vec::new(),
        diagnostics: Vec::new(),
    };

    for plugin in plugins {
        let declared = plugin
            .manifest
            .as_ref()
            .map(|manifest| declared_surface_count(&manifest.surfaces))
            .unwrap_or_default();
        let enabled = plugin.state == PluginState::Enabled;
        let secret_refs = plugin_secret_refs(plugin);

        if let Some(manifest) = &plugin.manifest {
            if enabled {
                projection.tools.extend(tool_descriptors(plugin, manifest));
                projection.hooks.extend(hook_descriptors(plugin, manifest));
                projection
                    .skills
                    .extend(skill_descriptors(plugin, manifest));
                projection
                    .commands
                    .extend(command_descriptors(plugin, manifest));
                projection.mcp.extend(mcp_descriptors(plugin, manifest));
                projection
                    .diagnostics
                    .extend(command_conflict_diagnostics(plugin, manifest));
            } else if declared > 0 {
                projection.diagnostics.push(diagnostic(
                    &plugin.id,
                    "inactive_surfaces",
                    &format!(
                        "plugin {} is {}; declared surfaces are retained as summaries only",
                        plugin.id,
                        plugin.state.as_str()
                    ),
                ));
            }
        }

        projection.plugins.push(PluginDescriptor {
            id: plugin.id.clone(),
            state: plugin.state.as_str().to_owned(),
            source: plugin.source.as_str().to_owned(),
            active_surface_count: if enabled {
                active_surface_count(&projection, &plugin.id)
            } else {
                0
            },
            declared_surface_count: declared,
            secret_refs,
        });
    }

    projection
}

pub fn evaluate_plugin_permission_ceiling(
    request: PluginPermissionCeilingRequest,
) -> PluginPermissionCeilingDecision {
    let evaluation = evaluate_inherited_ceiling(&InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: request.parent_mode,
            capability_ceiling: request.capability_ceiling,
            approved_scope_refs: request.approved_scope_refs,
            origin: RuntimeBoundaryOrigin::AppTask {
                app_id: Some(request.plugin_id.clone()),
                task_id: None,
            },
        },
        requested_mode: request.requested_mode,
        requested_capabilities: request.requested_capabilities,
        per_action_evaluation_required: true,
    });
    PluginPermissionCeilingDecision {
        plugin_id: request.plugin_id,
        allowed: evaluation.allowed,
        violations: evaluation.violations,
    }
}

pub fn reject_plugin_replay_live_dispatch(plugin_id: &str, reason: &str) -> PluginReplayRejection {
    PluginReplayRejection {
        plugin_id: plugin_id.to_owned(),
        accepted: false,
        reason: redact_string(reason),
    }
}

pub fn required_spec025_release_evidence_buckets() -> Vec<PluginSpec025ReleaseEvidenceBucket> {
    vec![
        PluginSpec025ReleaseEvidenceBucket::DiscoveryManifestGate,
        PluginSpec025ReleaseEvidenceBucket::DescriptorOnlySurfaces,
        PluginSpec025ReleaseEvidenceBucket::HookPolicyValidation,
        PluginSpec025ReleaseEvidenceBucket::PermissionCeiling,
        PluginSpec025ReleaseEvidenceBucket::SecretRedaction,
        PluginSpec025ReleaseEvidenceBucket::ReplayRejection,
        PluginSpec025ReleaseEvidenceBucket::ReleaseEvidence,
    ]
}

pub fn plugin_spec025_release_evidence_checklist(
    evidence: &PluginSpec025ReleaseEvidence,
) -> PluginSpec025ReleaseEvidenceChecklist {
    let required_buckets = required_spec025_release_evidence_buckets();
    let missing_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| !evidence.buckets.contains(bucket))
        .collect::<Vec<_>>();
    PluginSpec025ReleaseEvidenceChecklist {
        required_buckets,
        complete: missing_buckets.is_empty(),
        missing_buckets,
        evidence_refs: evidence.evidence_refs.clone(),
    }
}

pub fn plugin_surface_diagnostic(
    plugin_id: &str,
    code: &str,
    detail: &str,
) -> PluginSurfaceDiagnostic {
    diagnostic(plugin_id, code, detail)
}

pub fn plugin_spec025_evidence_ref(id: &str, summary: &str) -> EvidenceRef {
    let redacted_summary = redact_string(summary);
    EvidenceRef {
        kind: EvidenceKind::DiagnosticRecord,
        id: id.to_owned(),
        digest: format!("sha256:{}", sha256_hex(redacted_summary.as_bytes())),
        summary: redacted_summary,
        redaction_status: RedactionStatus::Redacted,
        owner_spec: Some("spec025".to_owned()),
        locator: None,
        retention_hint: Some("release-evidence".to_owned()),
    }
}

fn tool_descriptors(
    plugin: &DiscoveredPlugin,
    manifest: &crate::runtime::PluginManifest,
) -> Vec<PluginToolDescriptor> {
    names_from_surface(&manifest.surfaces, "tools")
        .into_iter()
        .map(|name| PluginToolDescriptor {
            description: description_for(&manifest.entrypoints, "tools", &name),
            plugin_id: plugin.id.clone(),
            name,
            execution_enabled: false,
            deferrable: true,
            provider_visible: false,
        })
        .collect()
}

fn hook_descriptors(
    plugin: &DiscoveredPlugin,
    manifest: &crate::runtime::PluginManifest,
) -> Vec<PluginHookDescriptor> {
    names_from_surface(&manifest.surfaces, "hooks")
        .into_iter()
        .map(|event| PluginHookDescriptor {
            plugin_id: plugin.id.clone(),
            event,
            execution_enabled: false,
        })
        .collect()
}

fn skill_descriptors(
    plugin: &DiscoveredPlugin,
    manifest: &crate::runtime::PluginManifest,
) -> Vec<PluginSkillDescriptor> {
    names_from_surface(&manifest.surfaces, "skills")
        .into_iter()
        .map(|name| PluginSkillDescriptor {
            namespace: format!("plugin:{}/{}", plugin.id, name),
            plugin_id: plugin.id.clone(),
            name,
            execution_enabled: false,
        })
        .collect()
}

fn command_descriptors(
    plugin: &DiscoveredPlugin,
    manifest: &crate::runtime::PluginManifest,
) -> Vec<PluginCommandDescriptor> {
    names_from_surface(&manifest.surfaces, "commands")
        .into_iter()
        .map(|name| PluginCommandDescriptor {
            backend: backend_for(&manifest.entrypoints, "commands", &name),
            plugin_id: plugin.id.clone(),
            name,
            execution_enabled: false,
        })
        .collect()
}

fn mcp_descriptors(
    plugin: &DiscoveredPlugin,
    manifest: &crate::runtime::PluginManifest,
) -> Vec<PluginMcpDescriptor> {
    names_from_surface(&manifest.surfaces, "mcp")
        .into_iter()
        .map(|name| PluginMcpDescriptor {
            backend: backend_for(&manifest.entrypoints, "mcp", &name),
            plugin_id: plugin.id.clone(),
            name,
            execution_enabled: false,
        })
        .collect()
}

fn command_conflict_diagnostics(
    plugin: &DiscoveredPlugin,
    manifest: &crate::runtime::PluginManifest,
) -> Vec<PluginSurfaceDiagnostic> {
    names_from_surface(&manifest.surfaces, "commands")
        .into_iter()
        .filter(|name| builtin_command_names().contains(&name.as_str()))
        .map(|name| {
            diagnostic(
                &plugin.id,
                "builtin_command_conflict",
                &format!("plugin command {name} conflicts with a builtin command"),
            )
        })
        .collect()
}

fn plugin_secret_refs(plugin: &DiscoveredPlugin) -> Vec<PluginSecretRef> {
    let mut refs = Vec::new();
    if let Some(manifest) = &plugin.manifest {
        refs.extend(manifest.requires_env.iter().map(|name| PluginSecretRef {
            kind: PluginSecretRefKind::Env,
            name: name.clone(),
            present: !plugin.missing_env.contains(name),
        }));
        refs.extend(manifest.requires_config.iter().map(|name| PluginSecretRef {
            kind: PluginSecretRefKind::Config,
            name: name.clone(),
            present: !plugin.missing_config.contains(name),
        }));
    }
    refs
}

fn names_from_surface(surfaces: &Value, key: &str) -> Vec<String> {
    surfaces.get(key).map(names_from_value).unwrap_or_default()
}

fn names_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
        Value::Object(object) => object.keys().cloned().collect(),
        Value::String(name) if !name.trim().is_empty() => vec![name.trim().to_owned()],
        _ => Vec::new(),
    }
}

fn description_for(entrypoints: &Value, kind: &str, name: &str) -> Option<String> {
    entrypoints
        .get(kind)
        .and_then(|value| value.get(name))
        .and_then(|value| value.get("description"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn backend_for(entrypoints: &Value, kind: &str, name: &str) -> String {
    entrypoints
        .get(kind)
        .and_then(|value| value.get(name))
        .and_then(|value| value.get("backend").or_else(|| value.get("command")))
        .and_then(Value::as_str)
        .unwrap_or("descriptor")
        .to_owned()
}

fn declared_surface_count(surfaces: &Value) -> usize {
    ["tools", "hooks", "skills", "commands", "mcp"]
        .into_iter()
        .map(|key| names_from_surface(surfaces, key).len())
        .sum()
}

fn active_surface_count(projection: &PluginSurfaceProjection, plugin_id: &str) -> usize {
    projection
        .tools
        .iter()
        .filter(|item| item.plugin_id == plugin_id)
        .count()
        + projection
            .hooks
            .iter()
            .filter(|item| item.plugin_id == plugin_id)
            .count()
        + projection
            .skills
            .iter()
            .filter(|item| item.plugin_id == plugin_id)
            .count()
        + projection
            .commands
            .iter()
            .filter(|item| item.plugin_id == plugin_id)
            .count()
        + projection
            .mcp
            .iter()
            .filter(|item| item.plugin_id == plugin_id)
            .count()
}

fn builtin_command_names() -> &'static [&'static str] {
    &["status", "stop", "restart", "help"]
}

fn diagnostic(plugin_id: &str, code: &str, detail: &str) -> PluginSurfaceDiagnostic {
    PluginSurfaceDiagnostic {
        plugin_id: plugin_id.to_owned(),
        code: code.to_owned(),
        message: redact_string(detail),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
