#[path = "execution_snapshot_types.rs"]
mod types;

pub use types::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_projection::{DataSurface, ProcessAdapterKind, SandboxStatus, Spec030RuntimeProjection};
use shacs_providers::ProviderRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSnapshotInput {
    pub snapshot_id: String,
    pub created_at_unix_ms: u64,
    pub config: ConfigSnapshotRef,
    pub profiles: ProfileSelectionSnapshot,
    pub trusted_runtime: TrustedRuntimeFactRef,
    pub sandbox: Vec<AdapterSandboxRef>,
    pub credential: CredentialSnapshotRef,
    pub context_sources: Vec<ContextSourceSnapshot>,
    pub selected_tools: Vec<SelectedIdentitySnapshot>,
    pub selected_resources: Vec<ResourceIdentitySnapshot>,
    pub provider: ProviderInputSnapshot,
    pub token_budget: TokenBudgetSnapshot,
    pub disclosure: DataDisclosureWarning,
    pub replay: ReplayContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSnapshot {
    pub schema_id: String,
    pub snapshot_id: String,
    pub created_at_unix_ms: u64,
    pub config: ConfigSnapshotRef,
    pub profiles: ProfileSelectionSnapshot,
    pub trusted_runtime: TrustedRuntimeFactRef,
    pub sandbox: Vec<AdapterSandboxRef>,
    pub credential: CredentialSnapshotRef,
    pub context_sources: Vec<ContextSourceSnapshot>,
    pub selected_tools: Vec<SelectedIdentitySnapshot>,
    pub selected_resources: Vec<ResourceIdentitySnapshot>,
    pub provider: ProviderInputSnapshot,
    pub token_budget: TokenBudgetSnapshot,
    pub disclosure: DataDisclosureWarning,
    pub replay: ReplayContract,
    pub provenance_digest: String,
}

impl ExecutionSnapshot {
    pub fn create(mut input: ExecutionSnapshotInput) -> Result<Self, ExecutionSnapshotError> {
        validate_input(&input)?;
        input
            .sandbox
            .sort_by_key(|reference| adapter_rank(reference.adapter));
        input
            .context_sources
            .sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
        input
            .selected_tools
            .sort_by(|left, right| left.identity.cmp(&right.identity));
        input
            .selected_resources
            .sort_by(|left, right| left.identity.cmp(&right.identity));
        input
            .disclosure
            .surfaces
            .sort_by_key(|surface| surface_rank(*surface));
        input.disclosure.surfaces.dedup();
        let mut snapshot = Self {
            schema_id: EXECUTION_SNAPSHOT_SCHEMA_V1.to_owned(),
            snapshot_id: input.snapshot_id,
            created_at_unix_ms: input.created_at_unix_ms,
            config: input.config,
            profiles: input.profiles,
            trusted_runtime: input.trusted_runtime,
            sandbox: input.sandbox,
            credential: input.credential,
            context_sources: input.context_sources,
            selected_tools: input.selected_tools,
            selected_resources: input.selected_resources,
            provider: input.provider,
            token_budget: input.token_budget,
            disclosure: input.disclosure,
            replay: input.replay,
            provenance_digest: String::new(),
        };
        snapshot.provenance_digest = digest_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    pub fn parse_json(input: &str) -> Result<Self, ExecutionSnapshotError> {
        let snapshot: Self = serde_json::from_str(input)
            .map_err(|error| ExecutionSnapshotError::Malformed(error.to_string()))?;
        if snapshot.schema_id != EXECUTION_SNAPSHOT_SCHEMA_V1 {
            return Err(ExecutionSnapshotError::UnknownSchema(snapshot.schema_id));
        }
        snapshot.validate_provenance()?;
        Ok(snapshot)
    }

    pub fn validate_provenance(&self) -> Result<(), ExecutionSnapshotError> {
        if self.replay != ReplayContract::diagnostic_only() {
            return Err(ExecutionSnapshotError::InvalidReplayContract);
        }
        if digest_snapshot(self)? == self.provenance_digest {
            Ok(())
        } else {
            Err(ExecutionSnapshotError::ProvenanceMismatch)
        }
    }

    pub fn semantic_compatibility_digest(&self) -> Result<String, ExecutionSnapshotError> {
        let mut value = serde_json::to_value(self)
            .map_err(|error| ExecutionSnapshotError::Malformed(error.to_string()))?;
        if let Value::Object(object) = &mut value {
            object.remove("snapshot_id");
            object.remove("created_at_unix_ms");
            object.remove("provenance_digest");
        }
        digest_value(&value)
    }
}

impl ExecutionSnapshotInput {
    pub fn attach_activation_refs(&mut self, activations: &[super::ActivationRecord]) {
        for resource in &mut self.selected_resources {
            resource.activation_ref = activations
                .iter()
                .find(|activation| activation.resource_ref() == resource.identity)
                .map(|activation| activation.activation_ref().to_owned());
        }
    }
}

pub struct ProviderExecutionHandoff {
    snapshot: ExecutionSnapshot,
    request: ProviderRequest,
}

impl ProviderExecutionHandoff {
    pub fn freeze(
        mut input: ExecutionSnapshotInput,
        request: ProviderRequest,
    ) -> Result<Self, ExecutionSnapshotError> {
        input.provider.model.clone_from(&request.model);
        input.provider.messages_digest = digest_value(&Value::Array(request.messages.clone()))?;
        input.provider.tools_digest = digest_value(&Value::Array(request.tools.clone()))?;
        input.selected_tools = request.tools.iter().filter_map(tool_identity).collect();
        Ok(Self {
            snapshot: ExecutionSnapshot::create(input)?,
            request,
        })
    }

    pub const fn snapshot(&self) -> &ExecutionSnapshot {
        &self.snapshot
    }

    pub fn into_request(self) -> ProviderRequest {
        self.request
    }
}

pub fn trusted_runtime_fact_refs(
    projection: &Spec030RuntimeProjection,
) -> Result<Spec030ExecutionRefs, ExecutionSnapshotError> {
    let value = serde_json::to_value(projection)
        .map_err(|error| ExecutionSnapshotError::Malformed(error.to_string()))?;
    let projection_digest = digest_value(&value)?;
    let profile_ref = digest_value(
        &serde_json::to_value(projection.profile())
            .map_err(|error| ExecutionSnapshotError::Malformed(error.to_string()))?,
    )?;
    let sandbox = projection
        .process_adapters()
        .iter()
        .map(|adapter| AdapterSandboxRef {
            adapter: adapter.adapter,
            mode: match projection.sandbox().status {
                SandboxStatus::Active
                    if projection
                        .sandbox()
                        .applied_adapters
                        .contains(&adapter.adapter) =>
                {
                    SandboxMode::Active
                }
                SandboxStatus::Active | SandboxStatus::Disabled => SandboxMode::Disabled,
                SandboxStatus::Unsupported => SandboxMode::Unsupported,
                SandboxStatus::Failed => SandboxMode::Failed,
                SandboxStatus::Unknown => SandboxMode::Unknown,
            },
            fallback: projection.sandbox().fallback,
        })
        .collect();
    let credential = projection.credential();
    let resources = projection
        .resources()
        .iter()
        .map(|resource| ResourceIdentitySnapshot {
            identity: resource.resource_ref.clone(),
            content_digest: resource.content_sha256.clone(),
            activation_ref: None,
        })
        .collect();
    Ok(Spec030ExecutionRefs {
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: projection.schema_version(),
            profile_ref,
            projection_digest,
        },
        sandbox,
        credential: CredentialSnapshotRef {
            source_kind: credential.source,
            status: credential.status,
            fingerprint_status: credential.fingerprint,
        },
        resources,
        disclosure: DataDisclosureWarning {
            raw_content_possible: projection.disclosure().raw_content_possible,
            surfaces: projection.disclosure().surfaces.clone(),
        },
    })
}

fn validate_input(input: &ExecutionSnapshotInput) -> Result<(), ExecutionSnapshotError> {
    for (field, value) in [
        ("snapshot_id", input.snapshot_id.as_str()),
        ("config.source_ref", input.config.source_ref.as_str()),
        (
            "trusted_runtime.profile_ref",
            input.trusted_runtime.profile_ref.as_str(),
        ),
        ("provider.provider", input.provider.provider.as_str()),
        ("provider.model", input.provider.model.as_str()),
        (
            "provider.shaping_version",
            input.provider.shaping_version.as_str(),
        ),
    ] {
        if value.trim().is_empty() {
            return Err(ExecutionSnapshotError::MissingField(field));
        }
    }
    if input.replay != ReplayContract::diagnostic_only() {
        return Err(ExecutionSnapshotError::InvalidReplayContract);
    }
    Ok(())
}

fn digest_snapshot(snapshot: &ExecutionSnapshot) -> Result<String, ExecutionSnapshotError> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|error| ExecutionSnapshotError::Malformed(error.to_string()))?;
    if let Value::Object(object) = &mut value {
        object.remove("provenance_digest");
    }
    digest_value(&value)
}

fn digest_value(value: &Value) -> Result<String, ExecutionSnapshotError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| ExecutionSnapshotError::Malformed(error.to_string()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn tool_identity(value: &Value) -> Option<SelectedIdentitySnapshot> {
    let identity = value
        .get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)?;
    Some(SelectedIdentitySnapshot {
        identity: format!("tool:{identity}"),
        activation_ref: None,
    })
}

const fn adapter_rank(adapter: ProcessAdapterKind) -> u8 {
    match adapter {
        ProcessAdapterKind::Bash => 0,
        ProcessAdapterKind::GenericExec => 1,
        ProcessAdapterKind::CredentialCommand => 2,
        ProcessAdapterKind::PackageOperation => 3,
        ProcessAdapterKind::PythonKernel => 4,
        ProcessAdapterKind::DaemonWorker => 5,
        ProcessAdapterKind::Mcp => 6,
    }
}

const fn surface_rank(surface: DataSurface) -> u8 {
    match surface {
        DataSurface::Session => 0,
        DataSurface::Log => 1,
        DataSurface::Trace => 2,
        DataSurface::ToolOutput => 3,
        DataSurface::ExtensionData => 4,
    }
}
