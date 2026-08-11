use super::model::Spec030ReleaseArtifactError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030SurfaceAssertions {
    pub schema_version: u32,
    pub runtime_status: String,
    pub credential_status: String,
    pub sandbox_status: String,
    pub supported_process_adapter_count: u64,
    pub resource_count: u64,
    pub raw_content_possible: bool,
    pub trace_status: String,
    pub cli_api_json_parity: bool,
    pub cli_human_tui_runtime_parity: bool,
    pub tui_no_session: bool,
    pub tui_runtime_owner_facts: bool,
    pub api_schema1_status: u16,
    pub api_schema2_status: u16,
    pub feature_assertions: Spec030FeatureAssertions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030FeatureAssertions {
    pub prd000_trusted_profile: bool,
    pub prd001_active_hooks: bool,
    pub prd002_process_controls: bool,
    pub prd003_credential_lifecycle: bool,
    pub prd004_active_sandbox: bool,
    pub prd005_resource_disclosure: bool,
    pub prd006_surface_integrity: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionCapture {
    schema_version: u32,
    availability: String,
    status: String,
    profile: ProfileCapture,
    hooks: HookCapture,
    credential: CredentialCapture,
    sandbox: SandboxCapture,
    process_adapters: Vec<ProcessCapture>,
    resources: Vec<serde_json::Value>,
    disclosure: DisclosureCapture,
}

#[derive(Deserialize)]
struct StatusCapture {
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileCapture {
    availability: String,
    status: String,
    profile: String,
    execution_authority: String,
    workspace_trust: String,
    resource_trust: String,
    default_containment: String,
    optional_sandbox: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HookCapture {
    availability: String,
    status: String,
    registered_handlers: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialCapture {
    availability: String,
    status: String,
    source: String,
    refresh_serialization: String,
}

#[derive(Deserialize)]
struct SandboxCapture {
    availability: String,
    status: String,
    fallback: String,
}

#[derive(Deserialize)]
struct ProcessCapture {
    support: String,
    capabilities: ProcessCapabilitiesCapture,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessCapabilitiesCapture {
    timeout: bool,
    abort: bool,
    cwd: bool,
    bounded_output: bool,
    descendant_cleanup: bool,
    startup_readiness: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DisclosureCapture {
    raw_content_possible: bool,
    surfaces: Vec<String>,
    trace: StatusCapture,
}

#[derive(Deserialize)]
struct ApiCapture {
    schema1: ApiResponseCapture,
    schema2: ApiResponseCapture,
}

#[derive(Deserialize)]
struct ApiResponseCapture {
    status: u16,
    body: serde_json::Value,
}

pub fn parse_spec030_surface_assertions(
    root: &Path,
) -> Result<Spec030SurfaceAssertions, Spec030ReleaseArtifactError> {
    let cli_bytes = read(root, "cli.json")?;
    let cli_value = serde_json::from_slice::<serde_json::Value>(&cli_bytes)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let projection = serde_json::from_value::<ProjectionCapture>(cli_value.clone())
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let api = serde_json::from_slice::<ApiCapture>(&read(root, "api.json")?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let human = String::from_utf8(read(root, "cli.txt")?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let tui_no_session = String::from_utf8(read(root, "tui-no-session.txt")?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let tui_runtime = String::from_utf8(read(root, "tui-runtime.txt")?)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let parity = api.schema1.body == cli_value;
    let human_parity = human
        .lines()
        .filter(|line| {
            [
                "Trusted runtime:",
                "profile:",
                "credential:",
                "sandbox:",
                "disclosure:",
                "trace:",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .all(|line| tui_runtime.lines().any(|tui_line| tui_line == line));
    let no_session = tui_no_session.contains("status: no sessions")
        && !tui_runtime.contains("status: no sessions");
    let runtime_owner_facts = tui_runtime
        .contains("process: adapter=bash support=supported controlScope=controlledChild")
        && tui_runtime.contains("credential: availability=available status=resolved")
        && tui_runtime.contains("sandbox: availability=available status=active");
    let no_session_is_clean = !tui_no_session
        .contains("process: adapter=bash support=supported controlScope=controlledChild")
        && !tui_no_session.contains("credential: availability=available status=resolved")
        && !tui_no_session.contains("sandbox: availability=available status=active");
    let controlled_process = projection.process_adapters.iter().any(|adapter| {
        adapter.support == "supported"
            && adapter.capabilities.timeout
            && adapter.capabilities.abort
            && adapter.capabilities.cwd
            && adapter.capabilities.bounded_output
            && adapter.capabilities.descendant_cleanup
    });
    let startup_process = projection
        .process_adapters
        .iter()
        .any(|adapter| adapter.support == "supported" && adapter.capabilities.startup_readiness);
    let feature_assertions = Spec030FeatureAssertions {
        prd000_trusted_profile: projection.availability != "unavailable"
            && projection.status != "unavailable"
            && projection.profile.availability != "unavailable"
            && projection.profile.status == "active"
            && projection.profile.profile == "trustedLocalAgent"
            && projection.profile.execution_authority == "currentOsUser"
            && projection.profile.workspace_trust == "userAsserted"
            && projection.profile.resource_trust == "explicitOrTrustedWorkspace"
            && projection.profile.default_containment == "none"
            && projection.profile.optional_sandbox == "adapterScoped",
        prd001_active_hooks: projection.hooks.availability != "unavailable"
            && projection.hooks.status == "active"
            && projection.hooks.registered_handlers > 0,
        prd002_process_controls: controlled_process && startup_process,
        prd003_credential_lifecycle: projection.credential.availability == "available"
            && projection.credential.status == "resolved"
            && projection.credential.source == "environment"
            && projection.credential.refresh_serialization != "unavailable",
        prd004_active_sandbox: projection.sandbox.availability == "available"
            && projection.sandbox.status == "active"
            && projection.sandbox.fallback == "notApplicable",
        prd005_resource_disclosure: !projection.resources.is_empty()
            && projection.disclosure.raw_content_possible
            && projection.disclosure.trace.status != "unavailable"
            && ["session", "log", "trace", "toolOutput", "extensionData"]
                .iter()
                .all(|surface| {
                    projection
                        .disclosure
                        .surfaces
                        .iter()
                        .any(|value| value == surface)
                }),
        prd006_surface_integrity: parity
            && human_parity
            && no_session
            && runtime_owner_facts
            && no_session_is_clean,
    };
    if projection.schema_version != 1
        || api.schema1.status != 200
        || api.schema2.status != 400
        || api.schema2.body["error"]["type"] != "invalid_request_error"
        || !parity
        || !human_parity
        || !no_session
        || !runtime_owner_facts
        || !no_session_is_clean
        || !feature_assertions.prd000_trusted_profile
        || !feature_assertions.prd001_active_hooks
        || !feature_assertions.prd002_process_controls
        || !feature_assertions.prd003_credential_lifecycle
        || !feature_assertions.prd004_active_sandbox
        || !feature_assertions.prd005_resource_disclosure
        || !feature_assertions.prd006_surface_integrity
    {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(Spec030SurfaceAssertions {
        schema_version: projection.schema_version,
        runtime_status: projection.status,
        credential_status: projection.credential.status,
        sandbox_status: projection.sandbox.status,
        supported_process_adapter_count: u64::try_from(
            projection
                .process_adapters
                .iter()
                .filter(|adapter| adapter.support == "supported")
                .count(),
        )
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?,
        resource_count: u64::try_from(projection.resources.len())
            .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?,
        raw_content_possible: projection.disclosure.raw_content_possible,
        trace_status: projection.disclosure.trace.status,
        cli_api_json_parity: parity,
        cli_human_tui_runtime_parity: human_parity,
        tui_no_session: no_session,
        tui_runtime_owner_facts: runtime_owner_facts,
        api_schema1_status: api.schema1.status,
        api_schema2_status: api.schema2.status,
        feature_assertions,
    })
}

pub(super) fn fixture_surface_assertions() -> Spec030SurfaceAssertions {
    Spec030SurfaceAssertions {
        schema_version: 1,
        runtime_status: "active".to_owned(),
        credential_status: "resolved".to_owned(),
        sandbox_status: "active".to_owned(),
        supported_process_adapter_count: 2,
        resource_count: 1,
        raw_content_possible: true,
        trace_status: "disabled".to_owned(),
        cli_api_json_parity: true,
        cli_human_tui_runtime_parity: true,
        tui_no_session: true,
        tui_runtime_owner_facts: true,
        api_schema1_status: 200,
        api_schema2_status: 400,
        feature_assertions: Spec030FeatureAssertions {
            prd000_trusted_profile: true,
            prd001_active_hooks: true,
            prd002_process_controls: true,
            prd003_credential_lifecycle: true,
            prd004_active_sandbox: true,
            prd005_resource_disclosure: true,
            prd006_surface_integrity: true,
        },
    }
}

fn read(root: &Path, relative: &str) -> Result<Vec<u8>, Spec030ReleaseArtifactError> {
    std::fs::read(root.join(relative))
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)
}
