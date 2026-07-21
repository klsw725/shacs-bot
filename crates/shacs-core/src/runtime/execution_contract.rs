use serde::{Deserialize, Serialize};
use shacs_utils::tool_results::ToolResultArtifactRef;

const MAX_EXECUTION_RECORDS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionScope {
    pub session_id: String,
    pub turn_id: String,
}

impl ExecutionScope {
    pub fn new(session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionIdentity {
    pub scope: ExecutionScope,
    pub effect_id: String,
    pub correlation_id: String,
    pub attempt_id: String,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
}

impl ExecutionIdentity {
    pub fn new(
        scope: ExecutionScope,
        effect_id: impl Into<String>,
        correlation_id: impl Into<String>,
    ) -> Self {
        let effect_id = effect_id.into();
        Self {
            scope,
            correlation_id: correlation_id.into(),
            attempt_id: "attempt:1".to_owned(),
            idempotency_key: effect_id.clone(),
            effect_id,
            causation_id: None,
        }
    }

    pub fn with_attempt(mut self, attempt_id: impl Into<String>) -> Self {
        self.attempt_id = attempt_id.into();
        self
    }

    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = idempotency_key.into();
        self
    }

    pub fn with_causation_id(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionDomain {
    Provider,
    Tool,
    Subagent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutcomeKind {
    Completed,
    ToolRequested,
    Failed,
    TimedOut,
    Cancelled,
    Stale,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureClass {
    Recoverable,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInterruptKind {
    AskUser,
    PermissionApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOutcomeKind {
    Completed,
    Failed { class: ToolFailureClass },
    TimedOut,
    Cancelled,
    Interrupted { interrupt: ToolInterruptKind },
    Skipped { reason: String },
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentOutcomeKind {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    Stale,
    RetryRequested,
    MergeRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "domain", content = "outcome", rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Provider(ProviderOutcomeKind),
    Tool(ToolOutcomeKind),
    Subagent(SubagentOutcomeKind),
}

impl ExecutionOutcome {
    pub fn domain(&self) -> ExecutionDomain {
        match self {
            Self::Provider(_) => ExecutionDomain::Provider,
            Self::Tool(_) => ExecutionDomain::Tool,
            Self::Subagent(_) => ExecutionDomain::Subagent,
        }
    }

    fn blocks_late_adoption(&self) -> bool {
        !matches!(
            self,
            Self::Subagent(SubagentOutcomeKind::RetryRequested)
                | Self::Provider(ProviderOutcomeKind::Failed)
                | Self::Tool(ToolOutcomeKind::Failed {
                    class: ToolFailureClass::Recoverable
                })
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingExecution {
    pub identity: ExecutionIdentity,
    pub domain: ExecutionDomain,
    pub started_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcomeFact {
    pub identity: ExecutionIdentity,
    pub outcome: ExecutionOutcome,
    pub finished_at_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<ToolResultArtifactRef>,
}

impl ExecutionOutcomeFact {
    pub fn new(
        identity: ExecutionIdentity,
        outcome: ExecutionOutcome,
        finished_at_ms: u128,
    ) -> Self {
        Self {
            identity,
            outcome,
            finished_at_ms,
            detail: None,
            artifact_ref: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_artifact_ref(mut self, artifact_ref: ToolResultArtifactRef) -> Self {
        self.artifact_ref = Some(artifact_ref);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LateResultDecision {
    Accepted,
    DuplicateIgnored { reason: String },
    DiscardedLate { reason: String },
    DiscardedStale { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedExecutionOutcome {
    pub fact: ExecutionOutcomeFact,
    pub decision: LateResultDecision,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeExecutionLedger {
    #[serde(default)]
    pub pending: Vec<PendingExecution>,
    #[serde(default)]
    pub outcomes: Vec<RecordedExecutionOutcome>,
}

impl RuntimeExecutionLedger {
    pub fn begin(&mut self, pending: PendingExecution) {
        self.pending.retain(|current| {
            current.identity.effect_id != pending.identity.effect_id
                || current.identity.attempt_id != pending.identity.attempt_id
        });
        self.pending.push(pending);
        retain_newest(&mut self.pending);
    }

    pub fn record(&mut self, fact: ExecutionOutcomeFact) -> LateResultDecision {
        let decision = self.decision_for(&fact);
        self.record_with_decision(fact, decision.clone());
        decision
    }

    pub fn record_with_decision(
        &mut self,
        fact: ExecutionOutcomeFact,
        decision: LateResultDecision,
    ) {
        if matches!(decision, LateResultDecision::Accepted) {
            self.pending.retain(|pending| {
                pending.identity.effect_id != fact.identity.effect_id
                    || pending.identity.correlation_id != fact.identity.correlation_id
            });
        }
        self.outcomes
            .push(RecordedExecutionOutcome { fact, decision });
        retain_newest(&mut self.outcomes);
    }

    fn decision_for(&self, fact: &ExecutionOutcomeFact) -> LateResultDecision {
        if let Some(pending) = self
            .pending
            .iter()
            .find(|pending| pending.identity.effect_id == fact.identity.effect_id)
        {
            if pending.identity.correlation_id != fact.identity.correlation_id
                || pending.identity.scope != fact.identity.scope
            {
                return LateResultDecision::DiscardedStale {
                    reason: "execution correlation does not match the pending effect".to_owned(),
                };
            }
        }

        if let Some(recorded) = self.outcomes.iter().rev().find(|recorded| {
            recorded.fact.identity.idempotency_key == fact.identity.idempotency_key
                && matches!(recorded.decision, LateResultDecision::Accepted)
        }) {
            if recorded.fact.identity.attempt_id == fact.identity.attempt_id {
                return LateResultDecision::DuplicateIgnored {
                    reason: "execution outcome attempt was already accepted".to_owned(),
                };
            }
            if recorded.fact.outcome.blocks_late_adoption() {
                return LateResultDecision::DiscardedLate {
                    reason: "a terminal outcome was already accepted for this effect".to_owned(),
                };
            }
        }

        LateResultDecision::Accepted
    }
}

fn retain_newest<T>(values: &mut Vec<T>) {
    if values.len() > MAX_EXECUTION_RECORDS {
        values.drain(..values.len() - MAX_EXECUTION_RECORDS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ExecutionIdentity {
        ExecutionIdentity::new(
            ExecutionScope::new("cli:test", "turn:1"),
            "provider:1",
            "turn:1",
        )
    }

    #[test]
    fn duplicate_attempt_is_ignored() {
        let mut ledger = RuntimeExecutionLedger::default();
        ledger.begin(PendingExecution {
            identity: identity(),
            domain: ExecutionDomain::Provider,
            started_at_ms: 1,
        });
        let fact = ExecutionOutcomeFact::new(
            identity(),
            ExecutionOutcome::Provider(ProviderOutcomeKind::Completed),
            2,
        );
        assert_eq!(ledger.record(fact.clone()), LateResultDecision::Accepted);
        assert!(matches!(
            ledger.record(fact),
            LateResultDecision::DuplicateIgnored { .. }
        ));
    }

    #[test]
    fn late_success_after_timeout_is_discarded() {
        let mut ledger = RuntimeExecutionLedger::default();
        let timed_out = ExecutionOutcomeFact::new(
            identity(),
            ExecutionOutcome::Provider(ProviderOutcomeKind::TimedOut),
            2,
        );
        assert_eq!(ledger.record(timed_out), LateResultDecision::Accepted);
        let completed = ExecutionOutcomeFact::new(
            identity().with_attempt("attempt:2"),
            ExecutionOutcome::Provider(ProviderOutcomeKind::Completed),
            3,
        );
        assert!(matches!(
            ledger.record(completed),
            LateResultDecision::DiscardedLate { .. }
        ));
    }

    #[test]
    fn mismatched_correlation_is_stale() {
        let mut ledger = RuntimeExecutionLedger::default();
        ledger.begin(PendingExecution {
            identity: identity(),
            domain: ExecutionDomain::Provider,
            started_at_ms: 1,
        });
        let stale = ExecutionOutcomeFact::new(
            ExecutionIdentity::new(
                ExecutionScope::new("cli:test", "turn:2"),
                "provider:1",
                "turn:2",
            ),
            ExecutionOutcome::Provider(ProviderOutcomeKind::Completed),
            2,
        );
        assert!(matches!(
            ledger.record(stale),
            LateResultDecision::DiscardedStale { .. }
        ));
        assert_eq!(ledger.pending.len(), 1);
    }

    #[test]
    fn recoverable_attempt_can_be_retried() {
        let mut ledger = RuntimeExecutionLedger::default();
        let failed = ExecutionOutcomeFact::new(
            identity(),
            ExecutionOutcome::Tool(ToolOutcomeKind::Failed {
                class: ToolFailureClass::Recoverable,
            }),
            2,
        );
        assert_eq!(ledger.record(failed), LateResultDecision::Accepted);
        let completed = ExecutionOutcomeFact::new(
            identity().with_attempt("attempt:2"),
            ExecutionOutcome::Tool(ToolOutcomeKind::Completed),
            3,
        );
        assert_eq!(ledger.record(completed), LateResultDecision::Accepted);
    }

    #[test]
    fn explicit_decisions_remain_bounded() {
        let mut ledger = RuntimeExecutionLedger::default();
        for index in 0..130 {
            let fact = ExecutionOutcomeFact::new(
                ExecutionIdentity::new(
                    ExecutionScope::new("cli:test", "turn:1"),
                    format!("subagent:{index}"),
                    format!("child:{index}"),
                ),
                ExecutionOutcome::Subagent(SubagentOutcomeKind::Stale),
                index,
            );
            ledger.record_with_decision(
                fact,
                LateResultDecision::DiscardedStale {
                    reason: "stale workflow child".to_owned(),
                },
            );
        }

        assert_eq!(ledger.outcomes.len(), MAX_EXECUTION_RECORDS);
        assert_eq!(ledger.outcomes[0].fact.identity.effect_id, "subagent:2");
    }
}
