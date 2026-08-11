use shacs_core::runtime::trusted_runtime::LocalSpec030ProjectionProvider;
use shacs_projection::{
    render_spec030_runtime, CredentialStatus, ProcessAdapterSupport, ResourceKind,
    ResourceLoadStatus, SandboxStatus, Spec030ProjectionProvider, Spec030RuntimeStatus,
    Spec030UnavailableReason, TraceDestination, TraceStatus,
};
use std::{error::Error, fs};

#[test]
fn local_spec030_provider_defaults_unobserved_facts_to_non_positive_states(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config_path = root.path().join("config.json");
    fs::create_dir_all(&workspace)?;
    fs::write(&config_path, "{}")?;

    let projection =
        LocalSpec030ProjectionProvider::load(Some(config_path), Some(workspace)).projection();

    assert_ne!(projection.status(), Spec030RuntimeStatus::Unavailable);
    assert!(projection
        .process_adapters()
        .iter()
        .all(|adapter| adapter.support == ProcessAdapterSupport::Unsupported));
    assert_eq!(
        projection.credential().status,
        CredentialStatus::Unavailable
    );
    assert_eq!(projection.sandbox().status, SandboxStatus::Unknown);
    Ok(())
}

#[test]
fn local_spec030_provider_discovers_live_resources_diagnostics_and_trace(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config_path = root.path().join("config.json");
    fs::create_dir_all(workspace.join("skills/review"))?;
    fs::create_dir_all(workspace.join(".shacs-bot/plugins/live"))?;
    fs::write(
        workspace.join("skills/review/SKILL.md"),
        "---\ndescription: review\n---\nreview",
    )?;
    fs::write(workspace.join("AGENTS.md"), "context")?;
    fs::write(workspace.join("prompt.md"), "prompt")?;
    fs::write(workspace.join("python_resource.py"), "VALUE = 1\n")?;
    fs::write(workspace.join("broken.mjs"), "throw new Error('broken')")?;
    fs::write(
        workspace.join(".shacs-bot/plugins/live/plugin.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "name": "live",
            "version": "0.1.0",
            "surfaces": {},
            "permissions": {},
            "entrypoints": {},
            "assets": []
        })
        .to_string(),
    )?;
    let trace = root.path().join("trace.jsonl");
    fs::write(&trace, "{\"event\":1}\n{\"event\":2}\n")?;
    fs::write(
        &config_path,
        serde_json::json!({
            "plugins": {"enabled":["live"],"trustedWorkspaces": [workspace.to_string_lossy()]},
            "trustedRuntime": {
                "resources": [
                    {"resourceRef":"prompt:configured","kind":"prompt","path":"prompt.md"},
                    {"resourceRef":"package:configured","kind":"package","path":"prompt.md","program":"/bin/sh","args":["-c","exit 0"]},
                    {"resourceRef":"skill:python","kind":"python","path":"python_resource.py","program":"python3","module":"python_resource"},
                    {"resourceRef":"extension:js","kind":"javaScript","path":"broken.mjs","program":"/bin/sh","runtime":"node"},
                    {"resourceRef":"context:missing","kind":"context","path":"missing.ctx"}
                ],
                "trace": {"enabled":true,"destination":"localOnly","path":trace.to_string_lossy()}
            }
        })
        .to_string(),
    )?;

    // When
    let projection =
        LocalSpec030ProjectionProvider::load(Some(config_path), Some(workspace)).projection();
    let rendered = render_spec030_runtime(&projection);

    // Then
    for kind in [
        ResourceKind::Skill,
        ResourceKind::Context,
        ResourceKind::Prompt,
        ResourceKind::Package,
        ResourceKind::Extension,
    ] {
        assert!(projection
            .resources()
            .iter()
            .any(|resource| resource.kind == kind));
    }
    assert!(projection.resources().iter().any(|resource| {
        resource.resource_ref == "package:configured"
            && resource.load_status == ResourceLoadStatus::Loaded
    }));
    assert!(projection.resources().iter().any(|resource| {
        resource.resource_ref == "extension:live"
            && resource.load_status == ResourceLoadStatus::Loaded
    }));
    assert!(projection.resources().iter().any(|resource| {
        resource.resource_ref == "context:missing"
            && resource.load_status == ResourceLoadStatus::ParseFailed
            && !resource.diagnostics.is_empty()
    }));
    assert_eq!(projection.disclosure().trace.status, TraceStatus::Enabled);
    let preview = projection
        .disclosure()
        .trace
        .preview
        .as_ref()
        .ok_or("trace preview missing")?;
    assert_eq!(preview.record_count, 2);
    assert_eq!(preview.destination, TraceDestination::LocalOnly);
    assert!(rendered.contains("source="));
    assert!(rendered.contains("precedence="));
    assert!(rendered.contains("path="));
    assert!(rendered.contains("reason="));
    Ok(())
}

#[test]
fn local_spec030_provider_invalidates_auto_skill_without_workspace_trust(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("skills/auto"))?;
    fs::write(
        workspace.join("skills/auto/SKILL.md"),
        "---\ndescription: auto\n---\nauto",
    )?;
    let config_path = root.path().join("config.json");
    fs::write(&config_path, "{}")?;

    // When
    let projection =
        LocalSpec030ProjectionProvider::load(Some(config_path), Some(workspace)).projection();

    // Then
    let skill = projection
        .resources()
        .iter()
        .find(|resource| resource.resource_ref == "skill:auto")
        .ok_or("auto skill missing")?;
    assert_eq!(skill.load_status, ResourceLoadStatus::Rejected);
    assert_eq!(
        skill.activation,
        shacs_projection::ResourceActivation::Inactive
    );
    assert!(!skill.diagnostics.is_empty());
    Ok(())
}

#[test]
fn configured_remote_trace_requires_exporter_and_endpoint_evidence_for_enabled(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        serde_json::json!({"trustedRuntime":{"trace":{
            "enabled":true,
            "destination":"configuredRemote"
        }}})
        .to_string(),
    )?;

    // When
    let projection =
        LocalSpec030ProjectionProvider::load(Some(config_path), Some(workspace)).projection();

    // Then
    assert_eq!(projection.disclosure().trace.status, TraceStatus::Preview);
    let preview = projection
        .disclosure()
        .trace
        .preview
        .as_ref()
        .ok_or("remote trace preview missing")?;
    assert_eq!(preview.destination, TraceDestination::ConfiguredRemote);
    assert_eq!(preview.exporter, None);
    assert_eq!(preview.endpoint_summary, None);
    Ok(())
}

#[test]
fn configured_remote_trace_is_enabled_with_exporter_and_endpoint_evidence(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        serde_json::json!({"trustedRuntime":{"trace":{
            "enabled":true,
            "destination":"configuredRemote",
            "exporter":"otlp",
            "endpointSummary":"https://collector.example"
        }}})
        .to_string(),
    )?;

    // When
    let projection =
        LocalSpec030ProjectionProvider::load(Some(config_path), Some(workspace)).projection();

    // Then
    assert_eq!(projection.disclosure().trace.status, TraceStatus::Enabled);
    let preview = projection
        .disclosure()
        .trace
        .preview
        .as_ref()
        .ok_or("remote trace preview missing")?;
    assert_eq!(preview.exporter.as_deref(), Some("otlp"));
    assert_eq!(
        preview.endpoint_summary.as_deref(),
        Some("https://collector.example")
    );
    Ok(())
}

#[test]
fn local_spec030_provider_reports_owner_unavailable_for_malformed_config(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config_path = root.path().join("config.json");
    fs::create_dir_all(&workspace)?;
    fs::write(&config_path, "{")?;

    let projection =
        LocalSpec030ProjectionProvider::load(Some(config_path), Some(workspace)).projection();

    assert_eq!(projection.status(), Spec030RuntimeStatus::Unavailable);
    assert_eq!(
        projection.unavailable_reason(),
        Some(Spec030UnavailableReason::OwnerUnavailable)
    );
    Ok(())
}
