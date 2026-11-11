use crate::runtime::{
    ProcessAdapterKind, ProcessExecutionEnvelope, ProcessExecutionEnvelopeInput, SafetyCapability,
};
use shacs_config::UserLocalBrowserEnable;

/// A file-backed opt-in bound to the complete immutable generation envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserStartEvidence {
    enable: UserLocalBrowserEnable,
    envelope: ProcessExecutionEnvelope,
}

impl BrowserStartEvidence {
    pub fn bind(enable: UserLocalBrowserEnable, envelope: &ProcessExecutionEnvelope) -> Self {
        Self {
            enable,
            envelope: envelope.clone(),
        }
    }

    pub(crate) fn admits(&self, envelope: &ProcessExecutionEnvelope) -> bool {
        let reconstructed =
            ProcessExecutionEnvelope::try_from_input(ProcessExecutionEnvelopeInput {
                identity: envelope.identity.clone(),
                adapter: envelope.adapter,
                action: envelope.action.clone(),
                required_secret_ref_count: envelope.secret_ref_evidence.len(),
                redacted_command: envelope.redacted_command.clone(),
            });
        self.enable.is_current()
            && reconstructed.as_ref().is_ok_and(|value| value == envelope)
            && &self.envelope == envelope
            && envelope.adapter == ProcessAdapterKind::Browser
            && envelope.action.capabilities == [SafetyCapability::ProcExec]
            && envelope.identity.session_id == envelope.action.session_id
            && envelope.identity.turn_id == envelope.action.turn_id
            && envelope.action.policy_safety_snapshot_ref.as_ref()
                == Some(&envelope.policy_safety_snapshot_ref)
    }
}
