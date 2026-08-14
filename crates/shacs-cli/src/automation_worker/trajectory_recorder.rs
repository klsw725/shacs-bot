use super::*;
use sha2::{Digest, Sha256};
use shacs_eval::completion_boundary::EvaluatorRoute;
use shacs_eval::evaluator::{
    ConfidenceBand, EvidenceKind, EvidenceRef, ProjectionStatus, RedactionStatus,
    ReplayDatasetItem, TaskOutcomeClass, VerdictKind,
};
use std::sync::Mutex;

pub(crate) fn record_no_provider_trajectory(
    adapter: &AgentLoopChatCompletionAdapter,
    store_root: &Path,
    trajectory_id: &str,
    instruction: &str,
) -> Result<shacs_core::runtime::RecordedTrajectoryRecord, String> {
    let data_dir = adapter
        .config_path
        .parent()
        .ok_or_else(|| "automation:data_dir_unavailable".to_owned())?;
    let work_id = crate::automation_producer::enqueue_recording_heartbeat(adapter, instruction)?;
    let recording = RecordingAdapters::new(adapter);
    let mut dispatcher = crate::automation_producer::open_dispatcher(data_dir)?;
    process_due_automation(&mut dispatcher, data_dir, &recording)?;
    let snapshot = recording
        .snapshot
        .into_inner()
        .map_err(|_| "trajectory:snapshot_lock".to_owned())?
        .ok_or_else(|| "trajectory:missing_snapshot".to_owned())?;
    let (request, receipt) = recording
        .receipt
        .into_inner()
        .map_err(|_| "trajectory:receipt_lock".to_owned())?
        .ok_or_else(|| "trajectory:missing_receipt".to_owned())?;
    if request.work_id != work_id {
        return Err("trajectory:work_mismatch".to_owned());
    }
    let source = bounded_source(instruction);
    let store = shacs_core::runtime::RecordedTrajectoryStore::open(store_root)
        .map_err(|error| error.to_string())?;
    store
        .write(shacs_core::runtime::RecordedTrajectoryInput {
            trajectory_id: trajectory_id.to_owned(),
            snapshot: snapshot_with_source(snapshot, &source)?,
            sources: vec![shacs_core::runtime::RecordedSourceArtifactInput {
                source_id: "automation-instruction".to_owned(),
                bytes: source.into_bytes(),
            }],
            owner_outcome: recorded_owner_outcome(&request, &receipt)?,
            boundary_requirement: shacs_core::runtime::RecordedBoundaryRequirement::RecordedOnly,
            origin: shacs_core::runtime::RecordedTrajectoryOrigin::AutomationOwnerReceipt,
        })
        .map_err(|error| error.to_string())
}

struct RecordingAdapters<'a> {
    inner: &'a AgentLoopChatCompletionAdapter,
    snapshot: Mutex<Option<ExecutionSnapshot>>,
    receipt: Mutex<Option<(AutomationDispatchRequest, AutomationExecutionReceipt)>>,
}

impl<'a> RecordingAdapters<'a> {
    fn new(inner: &'a AgentLoopChatCompletionAdapter) -> Self {
        Self {
            inner,
            snapshot: Mutex::new(None),
            receipt: Mutex::new(None),
        }
    }
}

impl AutomationOwnerAdapters for RecordingAdapters<'_> {
    fn snapshot(&self, request: &ProviderRequest) -> Result<ExecutionSnapshot, String> {
        let snapshot = self.inner.snapshot(request)?;
        *self
            .snapshot
            .lock()
            .map_err(|_| "trajectory:snapshot_lock".to_owned())? = Some(snapshot.clone());
        Ok(snapshot)
    }

    fn hooks(&self, request: &AutomationDispatchRequest) -> AutomationHookEvaluation {
        self.inner.hooks(request)
    }

    fn execute(
        &self,
        request: AutomationDispatchRequest,
        control: shacs_core::runtime::AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        let receipt = self.inner.execute(request.clone(), control);
        if let Ok(mut captured) = self.receipt.lock() {
            *captured = Some((request, receipt.clone()));
        }
        receipt
    }
}

fn bounded_source(instruction: &str) -> String {
    const MAX_SOURCE_BYTES: usize = 16 * 1024;
    shacs_redaction::redact_string(instruction)
        .chars()
        .take(MAX_SOURCE_BYTES)
        .collect()
}

fn snapshot_with_source(
    snapshot: ExecutionSnapshot,
    source: &str,
) -> Result<ExecutionSnapshot, String> {
    let bytes = u64::try_from(source.len()).map_err(|error| error.to_string())?;
    let context = shacs_core::runtime::ContextSourceSnapshot {
        source_ref: "automation-instruction".to_owned(),
        content_digest: digest(source),
        inclusion: shacs_core::runtime::ContextInclusion::Included,
        original_bytes: bytes,
        included_bytes: bytes,
        precedence: shacs_core::runtime::ContextArtifactPriority::ExplicitInline,
        decision: shacs_core::runtime::ContextBudgetDecision::Included,
        estimated_tokens: bytes.div_ceil(4),
        included_tokens: bytes.div_ceil(4),
        reason: None,
    };
    ExecutionSnapshot::create(shacs_core::runtime::ExecutionSnapshotInput {
        snapshot_id: snapshot.snapshot_id,
        created_at_unix_ms: snapshot.created_at_unix_ms,
        config: snapshot.config,
        profiles: snapshot.profiles,
        trusted_runtime: snapshot.trusted_runtime,
        sandbox: snapshot.sandbox,
        credential: snapshot.credential,
        context_sources: vec![context],
        selected_tools: snapshot.selected_tools,
        selected_resources: snapshot.selected_resources,
        provider: snapshot.provider,
        token_budget: snapshot.token_budget,
        disclosure: snapshot.disclosure,
        replay: snapshot.replay,
    })
    .map_err(|error| error.to_string())
}

fn recorded_owner_outcome(
    request: &AutomationDispatchRequest,
    receipt: &AutomationExecutionReceipt,
) -> Result<ReplayDatasetItem, String> {
    let outcome = receipt
        .task_outcome
        .as_ref()
        .ok_or_else(|| "trajectory:missing_owner_outcome".to_owned())?;
    let actual_outcome = route_outcome(outcome.route);
    let actual_projection_status = outcome_projection(&actual_outcome);
    let actual_verdict = match receipt.job_result {
        AutomationJobResult::Succeeded { .. } => VerdictKind::Pass,
        AutomationJobResult::Pending
        | AutomationJobResult::Failed { .. }
        | AutomationJobResult::TimedOut { .. }
        | AutomationJobResult::Cancelled { .. } => VerdictKind::Fail,
    };
    Ok(ReplayDatasetItem {
        dataset_id: format!("automation-dataset-{}", request.run.run_id),
        case_id: format!("automation-case-{}", request.work_id),
        trajectory_refs: Vec::new(),
        expected_verdict: actual_verdict.clone(),
        expected_outcome: actual_outcome.clone(),
        expected_projection_status: actual_projection_status.clone(),
        expected_confidence_band: ConfidenceBand::High,
        allowed_judge_roles: Vec::new(),
        redaction_profile: "shacs-redaction-v1".to_owned(),
        tool_outcome_policies: Vec::new(),
        actual_verdict: Some(actual_verdict),
        actual_outcome: Some(actual_outcome),
        actual_projection_status: Some(actual_projection_status),
        actual_confidence_band: Some(ConfidenceBand::High),
        auxiliary_judge_routes: Vec::new(),
        diagnostics_refs: evidence_refs(outcome),
        coverage_refs: Vec::new(),
    })
}

const fn route_outcome(route: EvaluatorRoute) -> TaskOutcomeClass {
    match route {
        EvaluatorRoute::Notify => TaskOutcomeClass::Notify,
        EvaluatorRoute::Suppress => TaskOutcomeClass::Suppress,
        EvaluatorRoute::Continue => TaskOutcomeClass::ContinueTask,
        EvaluatorRoute::Escalate => TaskOutcomeClass::Escalate,
        EvaluatorRoute::Verify => TaskOutcomeClass::Verify,
        EvaluatorRoute::RollbackCandidate => TaskOutcomeClass::Rollback,
    }
}

const fn outcome_projection(outcome: &TaskOutcomeClass) -> ProjectionStatus {
    match outcome {
        TaskOutcomeClass::Notify | TaskOutcomeClass::Suppress => ProjectionStatus::Success,
        TaskOutcomeClass::ContinueTask | TaskOutcomeClass::Verify | TaskOutcomeClass::Rollback => {
            ProjectionStatus::Pending
        }
        TaskOutcomeClass::Escalate => ProjectionStatus::Blocked,
    }
}

fn evidence_refs(outcome: &shacs_core::runtime::AutomationTaskOutcomeRecord) -> Vec<EvidenceRef> {
    [
        (
            &outcome.evaluator_evidence_ref,
            EvidenceKind::EvaluatorSummary,
        ),
        (&outcome.owner_evidence_ref, EvidenceKind::TaskResult),
    ]
    .into_iter()
    .map(|(reference, kind)| EvidenceRef {
        kind,
        id: reference.clone(),
        digest: digest(reference),
        summary: "recorded production automation evidence".to_owned(),
        redaction_status: RedactionStatus::AlreadySafe,
        owner_spec: Some("033".to_owned()),
        locator: Some(reference.clone()),
        retention_hint: Some("local".to_owned()),
    })
    .collect()
}

fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}
