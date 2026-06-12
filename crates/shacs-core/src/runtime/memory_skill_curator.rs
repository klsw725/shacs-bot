use crate::runtime::memory::MemoryStore;
use serde_json::json;
use shacs_eval::evaluator::stable_sha256_digest;
use shacs_eval::evaluator::{
    authored_skill_can_become_active, build_bounded_memory_evidence_set, curator_proposal,
    frozen_session_search_snapshot, skill_list_disclosure, skill_reference_disclosure,
    skill_view_disclosure, AuthoredSkillLifecycle, BoundedMemoryEvidenceSetInput,
    CuratorActionProposed, CuratorProposal, CuratorTargetKind, EvaluatorKind, EvidenceKind,
    EvidenceRef, FrozenSessionSearchSnapshot, MemoryEvidenceBudget, MemoryEvidenceRequest,
    MemoryEvidenceSet, RedactionStatus, SkillDisclosureRecord,
};
use shacs_session::{Session, SessionManager};
use shacs_skills::{SkillRegistry, SkillRegistryEntry, SkillSourceKind};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum MemorySkillCuratorRuntimeError {
    Digest(serde_json::Error),
    MissingSession(String),
    MissingSkill(String),
    SkillViewNotExplicit(String),
    MissingAppManifestEvidence,
    MissingAppTaskBoundaryEvidence,
}

impl std::fmt::Display for MemorySkillCuratorRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Digest(error) => write!(formatter, "runtime evidence digest failed: {error}"),
            Self::MissingSession(session_id) => {
                write!(formatter, "session not found: {session_id}")
            }
            Self::MissingSkill(skill_name) => write!(formatter, "skill not found: {skill_name}"),
            Self::SkillViewNotExplicit(skill_name) => {
                write!(
                    formatter,
                    "skill view requires an explicit request: {skill_name}"
                )
            }
            Self::MissingAppManifestEvidence => {
                write!(formatter, "app skill evidence requires manifest evidence")
            }
            Self::MissingAppTaskBoundaryEvidence => {
                write!(
                    formatter,
                    "app skill evidence requires task boundary evidence"
                )
            }
        }
    }
}

impl std::error::Error for MemorySkillCuratorRuntimeError {}

impl From<serde_json::Error> for MemorySkillCuratorRuntimeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Digest(error)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMemoryEvidenceRequestInput {
    pub request_id: String,
    pub session_id: String,
    pub query: String,
    pub evaluator_kind: EvaluatorKind,
    pub max_result_refs: usize,
    pub cutoff: String,
    pub redaction_profile: String,
    pub caller_reason: String,
}

pub fn runtime_memory_evidence_request(
    input: RuntimeMemoryEvidenceRequestInput,
) -> MemoryEvidenceRequest {
    MemoryEvidenceRequest {
        request_id: input.request_id,
        session_id: input.session_id,
        query: input.query,
        evaluator_kind: input.evaluator_kind,
        budget: MemoryEvidenceBudget {
            max_result_refs: input.max_result_refs,
        },
        cutoff: input.cutoff,
        redaction_profile: input.redaction_profile,
        caller_reason: input.caller_reason,
        created_at_ms: now_ms(),
    }
}

pub fn build_runtime_memory_evidence(
    store: &MemoryStore,
    request: &MemoryEvidenceRequest,
) -> Result<MemoryEvidenceSet, MemorySkillCuratorRuntimeError> {
    let search_outcome = store.search_evidence_refs(
        &request.query,
        &request.cutoff,
        request.budget.max_result_refs,
    )?;

    let mut evidence = build_bounded_memory_evidence_set(BoundedMemoryEvidenceSetInput {
        evidence_id: format!("memory-evidence-{}", request.request_id),
        request_id: request.request_id.clone(),
        query: redacted_diagnostic_text(&request.query),
        source_scope: request.session_id.clone(),
        cutoff: request.cutoff.clone(),
        max_result_refs: request.budget.max_result_refs,
        created_at_ms: request.created_at_ms,
        frozen_at_ms: now_ms(),
        candidate_refs: search_outcome.candidate_refs,
        summary_ref: None,
        redaction_profile: request.redaction_profile.clone(),
        omitted_reason: search_outcome.omitted_reason.clone(),
    })?;
    if search_outcome.filtered_omitted_count > 0 {
        evidence.candidate_count += search_outcome.filtered_omitted_count;
        evidence.omitted_count += search_outcome.filtered_omitted_count;
        evidence.omitted_reason = Some(search_outcome.omitted_reason);
    }

    Ok(evidence)
}

fn redacted_diagnostic_text(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        "[redacted]".to_owned()
    }
}

pub fn freeze_session_search_snapshot(
    manager: &SessionManager,
    session_id: &str,
    query: &str,
    snapshot_id: impl Into<String>,
) -> Result<FrozenSessionSearchSnapshot, MemorySkillCuratorRuntimeError> {
    let session = manager
        .load(session_id)
        .ok_or_else(|| MemorySkillCuratorRuntimeError::MissingSession(session_id.to_owned()))?;
    freeze_session_search_snapshot_from_session(&session, query, snapshot_id)
}

pub fn freeze_session_search_snapshot_from_session(
    session: &Session,
    query: &str,
    snapshot_id: impl Into<String>,
) -> Result<FrozenSessionSearchSnapshot, MemorySkillCuratorRuntimeError> {
    let query_lower = query.to_lowercase();
    let matched_event_refs = session
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            query.is_empty() || message.to_string().to_lowercase().contains(&query_lower)
        })
        .map(|(index, message)| {
            let digest = stable_sha256_digest(message)?;
            Ok(EvidenceRef {
                kind: EvidenceKind::SessionEvent,
                id: format!("{}:{index}", session.key),
                digest,
                summary: format!("session event {index}"),
                redaction_status: RedactionStatus::Redacted,
                owner_spec: Some("shacs-session.SessionManager".to_owned()),
                locator: Some(format!("session://{}/{}", session.key, index)),
                retention_hint: Some("audit_replay".to_owned()),
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let search_input = json!({"session_id": session.key, "query": query});
    Ok(frozen_session_search_snapshot(
        snapshot_id,
        &search_input,
        matched_event_refs,
        now_ms(),
    )?)
}

pub fn runtime_skill_list_disclosure(registry: &SkillRegistry) -> Vec<SkillDisclosureRecord> {
    registry
        .active_entries()
        .into_iter()
        .map(skill_list_record)
        .collect()
}

pub fn runtime_skill_view_disclosure(
    registry: &SkillRegistry,
    skill_name: &str,
    explicit_request: bool,
) -> Result<SkillDisclosureRecord, MemorySkillCuratorRuntimeError> {
    if !explicit_request {
        return Err(MemorySkillCuratorRuntimeError::SkillViewNotExplicit(
            skill_name.to_owned(),
        ));
    }
    let entry = registry
        .find(skill_name)
        .ok_or_else(|| MemorySkillCuratorRuntimeError::MissingSkill(skill_name.to_owned()))?;
    let list_record = skill_list_record(entry);
    let raw = entry.raw.as_deref().unwrap_or_default();
    Ok(skill_view_disclosure(&list_record, raw)?)
}

pub fn runtime_skill_reference_evidence(
    registry: &SkillRegistry,
    skill_name: &str,
) -> Result<SkillDisclosureRecord, MemorySkillCuratorRuntimeError> {
    let entry = registry
        .find(skill_name)
        .ok_or_else(|| MemorySkillCuratorRuntimeError::MissingSkill(skill_name.to_owned()))?;
    Ok(skill_reference_disclosure(&skill_list_record(entry)))
}

pub fn authored_skill_ready_for_active_registry(lifecycle: &AuthoredSkillLifecycle) -> bool {
    authored_skill_can_become_active(lifecycle)
}

pub fn runtime_curator_proposal_record(
    proposal_id: impl Into<String>,
    target_kind: CuratorTargetKind,
    target_refs: Vec<EvidenceRef>,
    reason: impl Into<String>,
    evidence_refs: Vec<EvidenceRef>,
    suggested_action: CuratorActionProposed,
    approval_ref: Option<EvidenceRef>,
) -> CuratorProposal {
    curator_proposal(
        proposal_id,
        target_kind,
        target_refs,
        reason,
        evidence_refs,
        suggested_action,
        approval_ref,
    )
}

pub fn app_provided_skill_reference_evidence(
    registry: &SkillRegistry,
    skill_name: &str,
    app_manifest_ref: Option<EvidenceRef>,
    app_task_boundary_ref: Option<EvidenceRef>,
) -> Result<SkillDisclosureRecord, MemorySkillCuratorRuntimeError> {
    let entry = registry
        .find(skill_name)
        .ok_or_else(|| MemorySkillCuratorRuntimeError::MissingSkill(skill_name.to_owned()))?;
    if entry.descriptor.source_kind == SkillSourceKind::PluginProvided && app_manifest_ref.is_none()
    {
        return Err(MemorySkillCuratorRuntimeError::MissingAppManifestEvidence);
    }
    if entry.descriptor.source_kind == SkillSourceKind::PluginProvided
        && app_task_boundary_ref.is_none()
    {
        return Err(MemorySkillCuratorRuntimeError::MissingAppTaskBoundaryEvidence);
    }
    let mut record = skill_reference_disclosure(&skill_list_record(entry));
    record.app_manifest_ref = app_manifest_ref;
    record.app_task_boundary_ref = app_task_boundary_ref;
    if let Some(evidence_ref) = record.evidence_ref.as_mut() {
        evidence_ref.owner_spec = Some("017-app-operating-environment".to_owned());
        evidence_ref.retention_hint = Some("app_task_boundary".to_owned());
    }
    Ok(record)
}

fn skill_list_record(entry: &SkillRegistryEntry) -> SkillDisclosureRecord {
    skill_list_disclosure(
        format!("skill-disclosure-{}", entry.descriptor.name),
        entry.descriptor.name.clone(),
        entry.descriptor.source_kind.label(),
        entry.status.label(),
        entry.descriptor.description.clone().unwrap_or_default(),
        entry.descriptor.body_hash.clone(),
    )
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(u128::from(u64::MAX)) as u64,
        Err(_) => 0,
    }
}
