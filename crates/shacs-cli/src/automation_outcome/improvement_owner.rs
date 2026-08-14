use super::owner_support::{evidence_ref, route_evidence};
use shacs_core::runtime::{
    AutomationRouteEvidence, AutomationTaskOutcomeInput, LocalImprovementStore,
};
use shacs_eval::completion_boundary::EvaluatorRoute;
use std::path::Path;

pub(super) fn record_candidate(
    workspace: &Path,
    input: &AutomationTaskOutcomeInput,
) -> Result<AutomationRouteEvidence, String> {
    let evidence = evidence_ref(EvaluatorRoute::RollbackCandidate, input);
    let proposal_id = input
        .owner_target_ref
        .as_deref()
        .ok_or_else(|| "rollback candidate lacks self-improvement owner target".to_owned())?;
    let store = LocalImprovementStore::open(workspace.join(".shacs-self-improvement/store.json"))
        .map_err(|error| error.to_string())?;
    store
        .record_rollback_candidate(proposal_id, &evidence)
        .map_err(|error| error.to_string())?;
    Ok(route_evidence(EvaluatorRoute::RollbackCandidate, evidence))
}
