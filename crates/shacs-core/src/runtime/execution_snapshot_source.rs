use super::execution_snapshot::{
    trusted_runtime_fact_refs, ConfigMigrationState, ConfigSnapshotRef, ContextSourceSnapshot,
    ExecutionSnapshotError, ExecutionSnapshotInput, ProfileSelectionSnapshot,
    ProviderInputSnapshot, ReplayContract, TokenBudgetSnapshot,
};
use super::{
    admit_activation_for_execution, select_token_estimator, ActivationSnapshotCandidate,
    ContextProviderHandoff, ResourceIdentitySnapshot, TokenEstimatorSelection,
};
use shacs_projection::{
    ResourceCandidateProjection, Spec030RuntimeProjection, Spec030UnavailableReason,
};
use shacs_providers::ProviderRequest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_EXECUTION_SNAPSHOT_ID: AtomicU64 = AtomicU64::new(1);
type ActivationProvider =
    Arc<dyn Fn(&ResourceCandidateProjection) -> Option<ActivationSnapshotCandidate> + Send + Sync>;

#[derive(Clone)]
pub struct LiveExecutionSnapshotSource {
    pub config: ConfigSnapshotRef,
    pub profiles: ProfileSelectionSnapshot,
    pub provider_id: String,
    pub shaping_version: String,
    pub estimator: TokenEstimatorSelection,
    pub budget_tokens: u64,
    spec030_projection: Arc<dyn Fn() -> Spec030RuntimeProjection + Send + Sync>,
    activation_provider: ActivationProvider,
}

impl Default for LiveExecutionSnapshotSource {
    fn default() -> Self {
        Self {
            config: ConfigSnapshotRef {
                source_ref: "config:unavailable".to_owned(),
                schema_version: 0,
                migration_state: ConfigMigrationState::Unavailable,
            },
            profiles: ProfileSelectionSnapshot {
                provider: None,
                trusted_runtime: None,
                context: None,
            },
            provider_id: "provider:unavailable".to_owned(),
            shaping_version: "shaping:unavailable".to_owned(),
            estimator: select_token_estimator("unknown", "unknown"),
            budget_tokens: 0,
            spec030_projection: Arc::new(|| {
                Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing)
            }),
            activation_provider: Arc::new(|_| None),
        }
    }
}

impl LiveExecutionSnapshotSource {
    pub fn with_spec030_provider(
        mut self,
        provider: Arc<dyn Fn() -> Spec030RuntimeProjection + Send + Sync>,
    ) -> Self {
        self.spec030_projection = provider;
        self
    }

    pub fn with_activation_provider(mut self, provider: ActivationProvider) -> Self {
        self.activation_provider = provider;
        self
    }

    pub fn resolve(
        &self,
        request: &ProviderRequest,
        context_sources: Vec<ContextSourceSnapshot>,
        handoff: Option<&ContextProviderHandoff>,
    ) -> Result<ExecutionSnapshotInput, ExecutionSnapshotError> {
        let projection = (self.spec030_projection)();
        let mut refs = trusted_runtime_fact_refs(&projection)?;
        attach_admitted_activation_refs(
            &mut refs.resources,
            projection.resources(),
            &self.activation_provider,
        );
        let sequence = NEXT_EXECUTION_SNAPSHOT_ID.fetch_add(1, Ordering::Relaxed);
        let created_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default();
        let estimator = handoff
            .map(|handoff| &handoff.estimator)
            .unwrap_or(&self.estimator);
        let budget_tokens = handoff
            .map(|handoff| handoff.budget_tokens as u64)
            .unwrap_or(self.budget_tokens);
        let reserved_tokens = handoff
            .map(|handoff| {
                handoff
                    .required
                    .iter()
                    .map(|evidence| evidence.estimated_tokens as u64)
                    .sum()
            })
            .unwrap_or_default();
        let used_context_tokens = handoff
            .map(|handoff| handoff.used_context_tokens as u64)
            .unwrap_or_default();
        Ok(ExecutionSnapshotInput {
            snapshot_id: format!("execution:{created_at_unix_ms}:{sequence}"),
            created_at_unix_ms,
            config: self.config.clone(),
            profiles: self.profiles.clone(),
            trusted_runtime: refs.trusted_runtime,
            sandbox: refs.sandbox,
            credential: refs.credential,
            context_sources,
            selected_tools: Vec::new(),
            selected_resources: refs.resources,
            provider: ProviderInputSnapshot {
                provider: self.provider_id.clone(),
                model: request.model.clone(),
                shaping_version: self.shaping_version.clone(),
                messages_digest: String::new(),
                tools_digest: String::new(),
            },
            token_budget: TokenBudgetSnapshot {
                tokenizer: estimator.name.clone(),
                estimator_uncertainty_percent: estimator.uncertainty_percent,
                budget_tokens,
                reserved_tokens,
                used_context_tokens,
                estimated_input_tokens: request
                    .messages
                    .iter()
                    .map(|message| estimator.estimate(&message.to_string()) as u64)
                    .sum(),
            },
            disclosure: refs.disclosure,
            replay: ReplayContract::diagnostic_only(),
        })
    }
}

fn attach_admitted_activation_refs(
    snapshots: &mut [ResourceIdentitySnapshot],
    resources: &[ResourceCandidateProjection],
    provider: &ActivationProvider,
) {
    for resource in resources {
        let Some(candidate) = provider(resource) else {
            continue;
        };
        if admit_activation_for_execution(candidate.record(), &candidate.live_facts(resource))
            .is_err()
        {
            continue;
        }
        if let Some(snapshot) = snapshots
            .iter_mut()
            .find(|snapshot| snapshot.identity == resource.resource_ref)
        {
            snapshot.activation_ref = Some(candidate.record().activation_ref().to_owned());
        }
    }
}

#[cfg(test)]
#[path = "execution_snapshot_source/tests.rs"]
mod tests;
