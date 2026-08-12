use super::*;
use crate::runtime::select_token_estimator;
use crate::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use crate::runtime::{ContextProviderHandoff, RequiredBudgetEvidence, RequiredContextKind};
use serde_json::json;
use shacs_projection::Spec030ProjectionProvider;

#[test]
fn provider_message_estimate_uses_selected_estimator_matrix() -> Result<(), ExecutionSnapshotError>
{
    let request = ProviderRequest {
        messages: vec![json!({"role": "user", "content": "abcdefghijkl"})],
        tools: Vec::new(),
        model: "matrix-model".to_owned(),
        settings: Default::default(),
        tool_choice: None,
    };
    let serialized = request.messages[0].to_string();

    for provider_id in ["anthropic", "openai", "google", "custom"] {
        let estimator = select_token_estimator(provider_id, &request.model);
        let projection = LocalSpec030ProjectionProvider::new(Spec030FactStore::new(
            WorkspaceTrustObservation::Trusted,
        ));
        let mut source = LiveExecutionSnapshotSource::default()
            .with_spec030_provider(Arc::new(move || projection.projection()));
        source.provider_id = provider_id.to_owned();
        source.estimator = estimator.clone();

        let snapshot = source.resolve(&request, Vec::new(), None)?;

        assert_eq!(snapshot.token_budget.tokenizer, estimator.name);
        assert_eq!(
            snapshot.token_budget.estimator_uncertainty_percent,
            estimator.uncertainty_percent
        );
        assert_eq!(
            snapshot.token_budget.estimated_input_tokens,
            estimator.estimate(&serialized) as u64,
            "provider {provider_id} did not use its selected estimator"
        );
    }
    Ok(())
}

#[test]
fn snapshot_budget_uses_handoff_estimator_and_evidence() -> Result<(), ExecutionSnapshotError> {
    let request = ProviderRequest {
        messages: vec![json!({"role": "user", "content": "abcdefghijkl"})],
        tools: Vec::new(),
        model: "claude-test".to_owned(),
        settings: Default::default(),
        tool_choice: None,
    };
    let estimator = select_token_estimator("anthropic", &request.model);
    let handoff = ContextProviderHandoff {
        blocks: Vec::new(),
        evidence: Vec::new(),
        used_context_tokens: 17,
        budget_tokens: 4096,
        estimator: estimator.clone(),
        required: vec![
            RequiredBudgetEvidence {
                kind: RequiredContextKind::ActiveUserMessage,
                estimated_tokens: 11,
                overflow_tokens: 0,
            },
            RequiredBudgetEvidence {
                kind: RequiredContextKind::RuntimeInstructions,
                estimated_tokens: 23,
                overflow_tokens: 0,
            },
        ],
        required_overflow_tokens: 0,
    };
    let projection = LocalSpec030ProjectionProvider::new(Spec030FactStore::new(
        WorkspaceTrustObservation::Trusted,
    ));
    let source = LiveExecutionSnapshotSource::default()
        .with_spec030_provider(Arc::new(move || projection.projection()));

    let snapshot = source.resolve(&request, Vec::new(), Some(&handoff))?;

    assert_eq!(snapshot.token_budget.tokenizer, estimator.name);
    assert_eq!(snapshot.token_budget.estimator_uncertainty_percent, 20);
    assert_eq!(snapshot.token_budget.budget_tokens, 4096);
    assert_eq!(snapshot.token_budget.reserved_tokens, 34);
    assert_eq!(snapshot.token_budget.used_context_tokens, 17);
    Ok(())
}
