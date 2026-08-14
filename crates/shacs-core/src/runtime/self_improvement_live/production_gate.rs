use super::local_types::digest;
use super::{
    CurrentGateEvidence, CurrentSpec030Receipts, LocalGateSource, LocalImprovementBlock,
    LocalImprovementProposal,
};
use crate::runtime::{
    AgentHookContext, HeadlessToolBeforeInteraction, PluginRuntimeHookAgentHook, RuntimeToolCall,
    ToolBeforeConfirmRequest, ToolBeforeConfirmation, ToolBeforeInteraction,
};
use serde::Serialize;
use serde_json::json;
use shacs_projection::{
    CredentialFingerprintStatus, CredentialStatus, HookDenialReason, HookRuntimeStatus,
    ProcessAdapterKind, ProcessAdapterSupport, SandboxFallback, SandboxStatus,
    Spec030ProjectionProvider, Spec030RuntimeProjection,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct ProductionLocalGateSource {
    projection: Arc<dyn Spec030ProjectionProvider + Send + Sync>,
    hooks: PluginRuntimeHookAgentHook,
    interaction: Arc<dyn ToolBeforeInteraction>,
    generation: AtomicU64,
}

impl ProductionLocalGateSource {
    pub fn new(
        projection: Arc<dyn Spec030ProjectionProvider + Send + Sync>,
        hooks: PluginRuntimeHookAgentHook,
        interaction: Arc<dyn ToolBeforeInteraction>,
    ) -> Self {
        Self {
            projection,
            hooks: hooks.with_interaction(interaction.clone()),
            interaction,
            generation: AtomicU64::new(0),
        }
    }

    pub fn headless(projection: Arc<dyn Spec030ProjectionProvider + Send + Sync>) -> Self {
        let interaction: Arc<dyn ToolBeforeInteraction> = Arc::new(HeadlessToolBeforeInteraction);
        Self::new(
            projection,
            PluginRuntimeHookAgentHook::new(Default::default()),
            interaction,
        )
    }
}

impl LocalGateSource for ProductionLocalGateSource {
    fn current_receipts(
        &self,
        proposal: &LocalImprovementProposal,
        target_digest: &str,
    ) -> Result<CurrentSpec030Receipts, LocalImprovementBlock> {
        let call = RuntimeToolCall::new(
            format!("self-improvement:{}", proposal.proposal_id()),
            "local_artifact_replace",
            json!({"target_ref": proposal.target_ref(), "target_digest": target_digest}),
        );
        let context = AgentHookContext {
            iteration: 0,
            messages: Vec::new(),
        };
        if !self
            .hooks
            .blocked_tool_messages(&context, std::slice::from_ref(&call))
            .is_empty()
        {
            return Err(denial(&self.hooks.hook_runtime_projection()));
        }
        if proposal.confirmation_required() {
            match self.interaction.confirm(&ToolBeforeConfirmRequest {
                call_id: call.id,
                prompt: format!("Apply local artifact proposal {}?", proposal.proposal_id()),
            }) {
                ToolBeforeConfirmation::Confirmed => {}
                ToolBeforeConfirmation::Denied => {
                    return Err(LocalImprovementBlock::ConfirmationDenied)
                }
                ToolBeforeConfirmation::HeadlessDenied => {
                    return Err(LocalImprovementBlock::HeadlessConfirmationDenied)
                }
            }
        }
        let projection = self.projection.projection();
        validate_projection(&projection)?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        CurrentSpec030Receipts::try_new(
            typed_evidence(
                "hook",
                proposal,
                target_digest,
                generation,
                projection.hooks(),
            ),
            typed_evidence(
                "confirmation",
                proposal,
                target_digest,
                generation,
                &proposal.confirmation_required(),
            ),
            typed_evidence(
                "process",
                proposal,
                target_digest,
                generation,
                &projection.process_adapters(),
            ),
            typed_evidence(
                "sandbox",
                proposal,
                target_digest,
                generation,
                projection.sandbox(),
            ),
            Some(typed_evidence(
                "credential",
                proposal,
                target_digest,
                generation,
                projection.credential(),
            )),
        )
    }
}

fn typed_evidence<T: Serialize + ?Sized>(
    kind: &str,
    proposal: &LocalImprovementProposal,
    target_digest: &str,
    generation: u64,
    fact: &T,
) -> CurrentGateEvidence {
    let bytes = serde_json::to_vec(&(
        kind,
        proposal.proposal_id(),
        &proposal.snapshot().provenance_digest,
        target_digest,
        generation,
        fact,
    ))
    .unwrap_or_default();
    CurrentGateEvidence::new(
        &digest(&bytes),
        &proposal.snapshot().provenance_digest,
        target_digest,
    )
}

fn denial(projection: &shacs_projection::HookRuntimeProjection) -> LocalImprovementBlock {
    match projection.recent_denials.last().map(|denial| denial.reason) {
        Some(HookDenialReason::UserDenied) => LocalImprovementBlock::ConfirmationDenied,
        Some(HookDenialReason::HeadlessConfirmationDenied) => {
            LocalImprovementBlock::HeadlessConfirmationDenied
        }
        Some(HookDenialReason::ExtensionBlocked | HookDenialReason::HookFailed) | None => {
            LocalImprovementBlock::HookVeto
        }
    }
}

fn validate_projection(projection: &Spec030RuntimeProjection) -> Result<(), LocalImprovementBlock> {
    if projection.hooks().status == HookRuntimeStatus::Unavailable {
        return Err(LocalImprovementBlock::MissingGateEvidence);
    }
    let process = projection.process_adapters().iter().any(|adapter| {
        adapter.adapter == ProcessAdapterKind::GenericExec
            && adapter.support == ProcessAdapterSupport::Supported
            && adapter.capabilities.cwd
            && adapter.capabilities.bounded_output
    });
    let sandbox = matches!(
        projection.sandbox().status,
        SandboxStatus::Active | SandboxStatus::Disabled
    ) && projection.sandbox().fallback != SandboxFallback::ExecutionDenied;
    let credential = projection.credential().status == CredentialStatus::Resolved
        && projection.credential().fingerprint == CredentialFingerprintStatus::Current;
    if process && sandbox && credential {
        Ok(())
    } else {
        Err(LocalImprovementBlock::MissingGateEvidence)
    }
}
