use serde_json::json;
use shacs_core::runtime::{
    build_plugin_surface_projection, evaluate_plugin_permission_ceiling,
    plugin_spec025_evidence_ref, plugin_spec025_release_evidence_checklist,
    plugin_surface_diagnostic, reject_plugin_replay_live_dispatch,
    required_spec025_release_evidence_buckets, BoundaryPermissionViolation, DiscoveredPlugin,
    PermissionMode, PluginBlockReason, PluginManifest, PluginManifestSource,
    PluginPermissionCeilingRequest, PluginSecretRefKind, PluginSpec025ReleaseEvidence,
    PluginSpec025ReleaseEvidenceBucket, PluginState, SafetyCapability,
};
use std::path::PathBuf;

#[test]
fn spec025_descriptor_only_enabled_plugin_surfaces_are_active_but_not_executable() {
    let plugin = enabled_plugin(
        "review",
        json!({
            "tools": ["review_comment"],
            "hooks": ["tool:before"],
            "skills": ["audit"],
            "commands": ["review"],
            "mcp": ["review_server"]
        }),
        json!({
            "tools": {"review_comment": {"description": "Review comment"}},
            "commands": {"review": {"backend": "node"}},
            "mcp": {"review_server": {"backend": "stdio"}}
        }),
    );

    let projection = build_plugin_surface_projection(&[plugin]);

    assert_eq!(projection.plugins[0].active_surface_count, 5);
    assert_eq!(projection.tools.len(), 1);
    assert_eq!(projection.hooks.len(), 1);
    assert_eq!(projection.skills.len(), 1);
    assert_eq!(projection.commands.len(), 1);
    assert_eq!(projection.mcp.len(), 1);
    assert!(projection.tools.iter().all(|tool| !tool.execution_enabled));
    assert!(projection.hooks.iter().all(|hook| !hook.execution_enabled));
    assert!(projection
        .skills
        .iter()
        .all(|skill| !skill.execution_enabled));
}

#[test]
fn spec025_non_enabled_states_contribute_no_active_surfaces() {
    let plugins = vec![
        plugin_with_state("waiting", PluginState::NotEnabled),
        plugin_with_state("disabled", PluginState::Disabled),
        plugin_with_state("blocked", PluginState::Blocked),
    ];

    let projection = build_plugin_surface_projection(&plugins);

    assert!(projection.tools.is_empty());
    assert!(projection.commands.is_empty());
    assert!(projection
        .plugins
        .iter()
        .all(|plugin| plugin.active_surface_count == 0));
    assert!(projection
        .plugins
        .iter()
        .all(|plugin| plugin.declared_surface_count > 0));
}

#[test]
fn spec025_command_and_mcp_backends_are_metadata_only() {
    let plugin = enabled_plugin(
        "backends",
        json!({"commands": ["inspect"], "mcp": ["fs"]}),
        json!({
            "commands": {"inspect": {"backend": "exec"}},
            "mcp": {"fs": {"backend": "stdio"}}
        }),
    );

    let projection = build_plugin_surface_projection(&[plugin]);

    assert_eq!(projection.commands[0].backend, "exec");
    assert!(!projection.commands[0].execution_enabled);
    assert_eq!(projection.mcp[0].backend, "stdio");
    assert!(!projection.mcp[0].execution_enabled);
}

#[test]
fn spec025_plugin_tool_search_metadata_is_deferrable_and_provider_hidden() {
    let plugin = enabled_plugin("tools", json!({"tools": ["review_comment"]}), json!({}));

    let projection = build_plugin_surface_projection(&[plugin]);

    assert!(projection.tools[0].deferrable);
    assert!(!projection.tools[0].provider_visible);
}

#[test]
fn spec025_builtin_command_conflict_is_diagnostic_only() {
    let plugin = enabled_plugin(
        "conflict",
        json!({"commands": ["status"]}),
        json!({"commands": {"status": {"backend": "node"}}}),
    );

    let projection = build_plugin_surface_projection(&[plugin]);

    assert_eq!(projection.commands.len(), 1);
    assert!(projection.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "builtin_command_conflict" && diagnostic.message.contains("status")
    }));
}

#[test]
fn spec025_plugin_skill_namespace_uses_plugin_prefix() {
    let plugin = enabled_plugin("skills", json!({"skills": ["audit"]}), json!({}));

    let projection = build_plugin_surface_projection(&[plugin]);

    assert_eq!(projection.skills[0].namespace, "plugin:skills/audit");
}

#[test]
fn spec025_permission_ceiling_rejects_widening_and_allows_declared_scope() {
    let rejected = evaluate_plugin_permission_ceiling(PluginPermissionCeilingRequest {
        plugin_id: "review".to_owned(),
        parent_mode: PermissionMode::Default,
        capability_ceiling: vec![SafetyCapability::FsRead],
        requested_mode: PermissionMode::Auto,
        requested_capabilities: vec![SafetyCapability::FsWrite],
        approved_scope_refs: Vec::new(),
    });
    assert!(!rejected.allowed);
    assert!(rejected
        .violations
        .contains(&BoundaryPermissionViolation::ModeWidening));
    assert!(rejected
        .violations
        .contains(&BoundaryPermissionViolation::CapabilityWidening));

    let allowed = evaluate_plugin_permission_ceiling(PluginPermissionCeilingRequest {
        plugin_id: "review".to_owned(),
        parent_mode: PermissionMode::Default,
        capability_ceiling: vec![SafetyCapability::FsRead],
        requested_mode: PermissionMode::Default,
        requested_capabilities: vec![SafetyCapability::FsRead],
        approved_scope_refs: vec!["approval:review".to_owned()],
    });
    assert!(allowed.allowed);
    assert!(allowed.violations.is_empty());
}

#[test]
fn spec025_secret_refs_expose_names_and_presence_without_raw_values() {
    let mut plugin = enabled_plugin("secrets", json!({}), json!({}));
    if let Some(manifest) = plugin.manifest.as_mut() {
        manifest.requires_env = vec!["OPENAI_API_KEY".to_owned()];
        manifest.requires_config = vec!["BOT_TOKEN".to_owned()];
    }
    plugin.missing_config = vec!["BOT_TOKEN".to_owned()];

    let projection = build_plugin_surface_projection(&[plugin]);
    let json = serde_json::to_string(&projection).unwrap_or_else(|error| error.to_string());

    assert!(projection.plugins[0].secret_refs.iter().any(|secret| {
        secret.kind == PluginSecretRefKind::Env && secret.name == "OPENAI_API_KEY" && secret.present
    }));
    assert!(projection.plugins[0].secret_refs.iter().any(|secret| {
        secret.kind == PluginSecretRefKind::Config && secret.name == "BOT_TOKEN" && !secret.present
    }));
    assert!(!json.contains("sk-secret-token"));
    assert!(!json.contains("raw-bot-token"));
}

#[test]
fn spec025_surface_diagnostics_and_replay_rejection_are_redacted() {
    let diagnostic = plugin_surface_diagnostic(
        "review",
        "secret",
        "OPENAI_API_KEY=sk-secret-token should not leak",
    );
    let replay = reject_plugin_replay_live_dispatch(
        "review",
        "Authorization: Bearer shacs_secret_123456789",
    );

    assert!(!diagnostic.message.contains("sk-secret-token"));
    assert!(!replay.reason.contains("shacs_secret_123456789"));
    assert!(!replay.accepted);
}

#[test]
fn spec025_release_evidence_checklist_requires_all_buckets() {
    let mut evidence = PluginSpec025ReleaseEvidence {
        buckets: required_spec025_release_evidence_buckets(),
        evidence_refs: vec![plugin_spec025_evidence_ref(
            "spec025-surfaces",
            "OPENAI_API_KEY=sk-secret-token covered",
        )],
    };

    let complete = plugin_spec025_release_evidence_checklist(&evidence);
    assert!(complete.complete);
    assert!(complete.evidence_refs[0].summary.contains("[REDACTED]"));

    evidence
        .buckets
        .retain(|bucket| *bucket != PluginSpec025ReleaseEvidenceBucket::ReplayRejection);
    let incomplete = plugin_spec025_release_evidence_checklist(&evidence);
    assert!(!incomplete.complete);
    assert!(incomplete
        .missing_buckets
        .contains(&PluginSpec025ReleaseEvidenceBucket::ReplayRejection));
}

fn plugin_with_state(id: &str, state: PluginState) -> DiscoveredPlugin {
    let mut plugin = enabled_plugin(id, json!({"tools": ["tool"]}), json!({}));
    plugin.state = state;
    if state == PluginState::Blocked {
        plugin.block_reasons = vec![PluginBlockReason::UntrustedWorkspace];
    }
    plugin
}

fn enabled_plugin(
    id: &str,
    surfaces: serde_json::Value,
    entrypoints: serde_json::Value,
) -> DiscoveredPlugin {
    DiscoveredPlugin {
        id: id.to_owned(),
        state: PluginState::Enabled,
        source: PluginManifestSource::UserData,
        root: PathBuf::from(format!("/tmp/{id}")),
        manifest_path: PathBuf::from(format!("/tmp/{id}/plugin.json")),
        digest: Some(format!("sha256:{id}")),
        manifest: Some(PluginManifest {
            schema_version: 1,
            name: id.to_owned(),
            version: "0.1.0".to_owned(),
            description: None,
            surfaces,
            requires_env: Vec::new(),
            requires_config: Vec::new(),
            permissions: json!({}),
            entrypoints,
            assets: json!({}),
        }),
        missing_env: Vec::new(),
        missing_config: Vec::new(),
        block_reasons: Vec::new(),
        diagnostics: Vec::new(),
    }
}
