use crate::runtime::{
    DockerContainmentSnapshot, InheritedPermissionContext, PermissionCeilingSnapshot,
    PermissionMode, PermissionRuleInput, PermissionedActionOrigin, ProcessAdapterKind,
    ProcessExecutionEnvelope, RuntimeBoundaryOrigin,
};

use super::containment_permission_external::blocked_external_surface;
use super::containment_permission_types::{
    ContainmentBoundaryRef, ContainmentComparisonOutcome, ContainmentEvidenceState,
    ContainmentPermissionError, ContainmentPermissionInput, ContainmentPermissionProof,
    ContainmentPermissionProofProjectionInput, ContainmentProofViolation,
    PermissionCeilingComparisonOutcome, PermissionCeilingProofInput, ProcessEnvelopeAdmission,
    RuntimeBoundaryKind, WorkspaceComparisonOutcome, WorkspaceScopeProof,
};

pub fn containment_permission_proof_for_process_gate(
    envelope: &ProcessExecutionEnvelope,
    permission_rules: &PermissionRuleInput,
    inherited_context: Option<&InheritedPermissionContext>,
    now_unix_ms: u64,
) -> Result<ContainmentPermissionProof, ContainmentPermissionError> {
    let child = child_boundary(envelope, permission_rules, inherited_context, now_unix_ms);
    evaluate_containment_permission(ContainmentPermissionInput {
        parent: parent_boundary(&child, inherited_context),
        child,
        policy_safety_digest: envelope
            .policy_safety_snapshot_ref
            .policy_safety_digest
            .clone(),
        process_envelope_id: envelope.envelope_id.clone(),
        now_unix_ms,
        cancelled_at_unix_ms: None,
        untrusted_metadata: None,
    })
}

pub fn evaluate_containment_permission(
    input: ContainmentPermissionInput,
) -> Result<ContainmentPermissionProof, ContainmentPermissionError> {
    if input.policy_safety_digest.0.trim().is_empty() {
        return Err(ContainmentPermissionError::MissingPolicySnapshotRef);
    }
    if input.process_envelope_id.trim().is_empty() {
        return Err(ContainmentPermissionError::MissingProcessEnvelopeRef);
    }
    let containment_outcome = compare_containment(input.child.containment_state);
    let workspace_outcome = compare_workspace(&input.child.workspace_scope);
    let ceiling_outcome = compare_ceiling(&input.child.permission_ceiling);
    let blocked = blocked_external_surface(input.child.boundary_kind);
    let cancelled = input.cancelled_at_unix_ms.is_some();
    let mut violations = violations(
        containment_outcome,
        workspace_outcome,
        ceiling_outcome,
        blocked.is_some(),
    );
    if cancelled {
        violations.push(ContainmentProofViolation::CancelledAdmissionReuse);
    }
    let admission = admission_for(
        containment_outcome,
        workspace_outcome,
        ceiling_outcome,
        blocked.is_some(),
        cancelled,
    );
    let proof_id = format!("containment-proof:{}", input.process_envelope_id);
    let diagnostics_input = ContainmentPermissionProofProjectionInput {
        proof_id: proof_id.clone(),
        envelope_id: input.process_envelope_id.clone(),
        policy_safety_digest: input.policy_safety_digest.clone(),
        parent_boundary_kind: input.parent.boundary_kind,
        child_boundary_kind: input.child.boundary_kind,
        admission,
        redacted_summary: format!(
            "boundary={:?}; admission={admission:?}",
            input.child.boundary_kind
        ),
    };
    Ok(ContainmentPermissionProof {
        proof_id,
        policy_safety_digest: input.policy_safety_digest,
        envelope_id: input.process_envelope_id,
        containment_outcome,
        workspace_outcome,
        ceiling_outcome,
        admission,
        violations,
        diagnostics_input,
        blocked_external_surface: blocked,
    })
}

fn compare_containment(state: ContainmentEvidenceState) -> ContainmentComparisonOutcome {
    match state {
        ContainmentEvidenceState::ConfirmedNonPrivileged
        | ContainmentEvidenceState::ConfirmedEquivalent => {
            ContainmentComparisonOutcome::EqualContainment
        }
        ContainmentEvidenceState::NarrowerHardened => {
            ContainmentComparisonOutcome::NarrowerContainment
        }
        ContainmentEvidenceState::NativeUnknown => ContainmentComparisonOutcome::UnknownContainment,
        ContainmentEvidenceState::EvidenceMissing => ContainmentComparisonOutcome::MissingEvidence,
        ContainmentEvidenceState::UnsafePrivileged => {
            ContainmentComparisonOutcome::UnsafeContainment
        }
        ContainmentEvidenceState::Mismatched => ContainmentComparisonOutcome::MismatchedContainment,
        ContainmentEvidenceState::Stale => ContainmentComparisonOutcome::StaleContainment,
        ContainmentEvidenceState::Malformed => ContainmentComparisonOutcome::MalformedContainment,
    }
}

fn compare_workspace(scope: &WorkspaceScopeProof) -> WorkspaceComparisonOutcome {
    let parent = normalize_scope_ref(&scope.parent_workspace_ref);
    let child = normalize_scope_ref(&scope.child_workspace_ref);
    let (Some(parent), Some(child)) = (parent, child) else {
        return WorkspaceComparisonOutcome::MalformedScope;
    };
    if scope.narrowing_reason == "wider" {
        WorkspaceComparisonOutcome::WiderScope
    } else if scope.parent_workspace_ref == scope.child_workspace_ref
        && scope.parent_scope_digest == scope.child_scope_digest
    {
        WorkspaceComparisonOutcome::SameScope
    } else if child.is_equal_or_narrower_than(&parent) {
        WorkspaceComparisonOutcome::NarrowerScope
    } else {
        WorkspaceComparisonOutcome::WiderScope
    }
}

fn compare_ceiling(input: &PermissionCeilingProofInput) -> PermissionCeilingComparisonOutcome {
    if !input.per_action_evaluation_required || input.approved_scope_refs.is_empty() {
        return PermissionCeilingComparisonOutcome::DeferredGateBypass;
    }
    if mode_rank(input.requested_mode) > mode_rank(input.parent_mode) {
        return PermissionCeilingComparisonOutcome::ModeWidening;
    }
    if input
        .requested_capabilities
        .iter()
        .any(|capability| !input.parent_capabilities.contains(capability))
    {
        return PermissionCeilingComparisonOutcome::CapabilityWidening;
    }
    if !requested_scope_is_approved(&input.requested_scope_ref, &input.approved_scope_refs) {
        return PermissionCeilingComparisonOutcome::ScopeWidening;
    }
    if mode_rank(input.requested_mode) < mode_rank(input.parent_mode)
        || input.requested_capabilities.len() < input.parent_capabilities.len()
    {
        PermissionCeilingComparisonOutcome::NarrowerCeiling
    } else {
        PermissionCeilingComparisonOutcome::EqualCeiling
    }
}

fn admission_for(
    containment: ContainmentComparisonOutcome,
    workspace: WorkspaceComparisonOutcome,
    ceiling: PermissionCeilingComparisonOutcome,
    blocked: bool,
    cancelled: bool,
) -> ProcessEnvelopeAdmission {
    if blocked {
        return ProcessEnvelopeAdmission::BlockedExternalSurface;
    }
    if cancelled || containment == ContainmentComparisonOutcome::StaleContainment {
        return ProcessEnvelopeAdmission::RejectStale;
    }
    if containment == ContainmentComparisonOutcome::MalformedContainment
        || workspace == WorkspaceComparisonOutcome::MalformedScope
    {
        return ProcessEnvelopeAdmission::RejectMalformed;
    }
    if containment == ContainmentComparisonOutcome::MismatchedContainment
        || workspace == WorkspaceComparisonOutcome::MismatchedScopeRef
    {
        return ProcessEnvelopeAdmission::Deny;
    }
    if containment == ContainmentComparisonOutcome::UnknownContainment
        || containment == ContainmentComparisonOutcome::MissingEvidence
        || workspace == WorkspaceComparisonOutcome::UnknownScope
    {
        return ProcessEnvelopeAdmission::AskRequired;
    }
    if containment == ContainmentComparisonOutcome::UnsafeContainment
        || workspace == WorkspaceComparisonOutcome::WiderScope
        || !matches!(
            ceiling,
            PermissionCeilingComparisonOutcome::EqualCeiling
                | PermissionCeilingComparisonOutcome::NarrowerCeiling
        )
    {
        return ProcessEnvelopeAdmission::Deny;
    }
    ProcessEnvelopeAdmission::Admit
}

fn violations(
    containment: ContainmentComparisonOutcome,
    workspace: WorkspaceComparisonOutcome,
    ceiling: PermissionCeilingComparisonOutcome,
    blocked: bool,
) -> Vec<ContainmentProofViolation> {
    let mut result = containment_violations(containment);
    if workspace == WorkspaceComparisonOutcome::WiderScope {
        result.push(ContainmentProofViolation::WorkspaceWidening);
    }
    if workspace == WorkspaceComparisonOutcome::MalformedScope {
        result.push(ContainmentProofViolation::MalformedInput);
    }
    match ceiling {
        PermissionCeilingComparisonOutcome::ModeWidening => {
            result.push(ContainmentProofViolation::ModeWidening);
        }
        PermissionCeilingComparisonOutcome::CapabilityWidening => {
            result.push(ContainmentProofViolation::CapabilityWidening);
        }
        PermissionCeilingComparisonOutcome::DeferredGateBypass => {
            result.push(ContainmentProofViolation::DeferredGateBypass);
        }
        PermissionCeilingComparisonOutcome::ScopeWidening => {
            result.push(ContainmentProofViolation::WorkspaceWidening);
        }
        PermissionCeilingComparisonOutcome::EqualCeiling
        | PermissionCeilingComparisonOutcome::NarrowerCeiling => {}
    }
    if blocked {
        result.push(ContainmentProofViolation::BlockedExternalSurface);
    }
    result
}

fn containment_violations(outcome: ContainmentComparisonOutcome) -> Vec<ContainmentProofViolation> {
    match outcome {
        ContainmentComparisonOutcome::UnknownContainment
        | ContainmentComparisonOutcome::MissingEvidence => {
            vec![ContainmentProofViolation::UnknownContainment]
        }
        ContainmentComparisonOutcome::UnsafeContainment => {
            vec![ContainmentProofViolation::UnsafeContainment]
        }
        ContainmentComparisonOutcome::MismatchedContainment => {
            vec![ContainmentProofViolation::ContainmentDigestMismatch]
        }
        ContainmentComparisonOutcome::StaleContainment => {
            vec![ContainmentProofViolation::StaleEvidence]
        }
        ContainmentComparisonOutcome::MalformedContainment => {
            vec![ContainmentProofViolation::MalformedInput]
        }
        ContainmentComparisonOutcome::EqualContainment
        | ContainmentComparisonOutcome::NarrowerContainment => Vec::new(),
    }
}

fn mode_rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::Plan => 0,
        PermissionMode::Default | PermissionMode::DontAsk => 1,
        PermissionMode::AcceptEdits => 2,
        PermissionMode::Auto => 3,
        PermissionMode::BypassPermissions => 4,
    }
}

fn child_boundary(
    envelope: &ProcessExecutionEnvelope,
    permission_rules: &PermissionRuleInput,
    inherited_context: Option<&InheritedPermissionContext>,
    now_unix_ms: u64,
) -> ContainmentBoundaryRef {
    let requested_mode = envelope.action.permission_mode_snapshot.mode;
    let requested_capabilities = envelope.action.capabilities.clone();
    let ceiling = inherited_context
        .map(|context| PermissionCeilingProofInput {
            parent_mode: context.ceiling.parent_mode,
            requested_mode: context.requested_mode,
            parent_capabilities: context.ceiling.capability_ceiling.clone(),
            requested_capabilities: context.requested_capabilities.clone(),
            approved_scope_refs: context.ceiling.approved_scope_refs.clone(),
            requested_scope_ref: scope_ref(envelope),
            per_action_evaluation_required: context.per_action_evaluation_required,
        })
        .unwrap_or_else(|| PermissionCeilingProofInput {
            parent_mode: requested_mode,
            requested_mode,
            parent_capabilities: requested_capabilities.clone(),
            requested_capabilities,
            approved_scope_refs: Vec::new(),
            requested_scope_ref: scope_ref(envelope),
            per_action_evaluation_required: true,
        });
    let workspace_scope = workspace_scope_from_ceiling(envelope, &ceiling);
    ContainmentBoundaryRef {
        boundary_id: envelope.envelope_id.clone(),
        boundary_kind: boundary_kind(envelope.adapter),
        origin: boundary_origin(&envelope.action.origin),
        containment_state: containment_state(&permission_rules.containment),
        containment_digest: permission_rules.containment.digest.clone(),
        workspace_scope,
        permission_ceiling: ceiling,
        created_at_unix_ms: now_unix_ms,
    }
}

fn parent_boundary(
    child: &ContainmentBoundaryRef,
    inherited_context: Option<&InheritedPermissionContext>,
) -> ContainmentBoundaryRef {
    let mut parent = child.parent_boundary();
    if let Some(context) = inherited_context {
        parent.origin = context.ceiling.origin.clone();
        parent.permission_ceiling = parent_ceiling(&context.ceiling);
    }
    parent
}

fn parent_ceiling(ceiling: &PermissionCeilingSnapshot) -> PermissionCeilingProofInput {
    PermissionCeilingProofInput {
        parent_mode: ceiling.parent_mode,
        requested_mode: ceiling.parent_mode,
        parent_capabilities: ceiling.capability_ceiling.clone(),
        requested_capabilities: ceiling.capability_ceiling.clone(),
        approved_scope_refs: ceiling.approved_scope_refs.clone(),
        requested_scope_ref: ceiling
            .approved_scope_refs
            .first()
            .cloned()
            .unwrap_or_default(),
        per_action_evaluation_required: true,
    }
}

fn workspace_scope_from_ceiling(
    envelope: &ProcessExecutionEnvelope,
    ceiling: &PermissionCeilingProofInput,
) -> WorkspaceScopeProof {
    let child_ref = scope_ref(envelope);
    let parent_ref = ceiling
        .approved_scope_refs
        .first()
        .cloned()
        .unwrap_or_default();
    WorkspaceScopeProof::from_parent_child(&parent_ref, &child_ref)
}

fn boundary_kind(adapter: ProcessAdapterKind) -> RuntimeBoundaryKind {
    match adapter {
        ProcessAdapterKind::ExecTool => RuntimeBoundaryKind::ExecTool,
        ProcessAdapterKind::PluginHook => RuntimeBoundaryKind::PluginHook,
        ProcessAdapterKind::PluginTool => RuntimeBoundaryKind::PluginTool,
        ProcessAdapterKind::PluginCommand => RuntimeBoundaryKind::PluginCommand,
        ProcessAdapterKind::McpStdio => RuntimeBoundaryKind::McpStdio,
    }
}

fn boundary_origin(origin: &PermissionedActionOrigin) -> RuntimeBoundaryOrigin {
    match origin {
        PermissionedActionOrigin::UserTurn => RuntimeBoundaryOrigin::UserTurn,
        PermissionedActionOrigin::Subagent { subagent_id } => RuntimeBoundaryOrigin::Subagent {
            subagent_id: subagent_id.clone(),
        },
        PermissionedActionOrigin::CronWake { job_id } => RuntimeBoundaryOrigin::CronWake {
            job_id: job_id.clone(),
            approval_ref: None,
        },
        PermissionedActionOrigin::AppTask { app_id, task_id } => RuntimeBoundaryOrigin::AppTask {
            app_id: app_id.clone(),
            task_id: task_id.clone(),
        },
        PermissionedActionOrigin::LocalApi { request_id } => RuntimeBoundaryOrigin::LocalApi {
            request_id: request_id.clone(),
        },
        PermissionedActionOrigin::ChannelInbound {
            channel,
            message_id,
        } => RuntimeBoundaryOrigin::ChannelInbound {
            channel: channel.clone(),
            message_id: message_id.clone(),
        },
        PermissionedActionOrigin::DeferredBridge {
            bridge_name,
            scope_digest,
            ..
        } => RuntimeBoundaryOrigin::DeferredMcp {
            bridge_name: bridge_name.clone(),
            scope_digest: scope_digest.clone(),
        },
    }
}

fn containment_state(snapshot: &DockerContainmentSnapshot) -> ContainmentEvidenceState {
    if snapshot.confirmed_non_privileged() {
        ContainmentEvidenceState::ConfirmedNonPrivileged
    } else if snapshot.is_unknown() {
        ContainmentEvidenceState::NativeUnknown
    } else if snapshot.privileged == Some(true) || snapshot.root_user == Some(true) {
        ContainmentEvidenceState::UnsafePrivileged
    } else {
        ContainmentEvidenceState::Mismatched
    }
}

fn scope_ref(envelope: &ProcessExecutionEnvelope) -> String {
    envelope
        .action
        .permission_mode_snapshot
        .scope_ref
        .clone()
        .unwrap_or_else(|| "workspace".to_owned())
}

fn requested_scope_is_approved(requested: &str, approved_refs: &[String]) -> bool {
    let Some(requested_ref) = normalize_scope_ref(requested) else {
        return false;
    };
    approved_refs.iter().any(|approved| {
        normalize_scope_ref(approved)
            .is_some_and(|approved_ref| requested_ref.is_equal_or_narrower_than(&approved_ref))
    })
}

struct NormalizedScopeRef<'a> {
    segments: Vec<&'a str>,
}

impl NormalizedScopeRef<'_> {
    fn is_equal_or_narrower_than(&self, parent: &Self) -> bool {
        self.segments.len() >= parent.segments.len()
            && self
                .segments
                .iter()
                .zip(parent.segments.iter())
                .all(|(child, parent)| child == parent)
    }
}

fn normalize_scope_ref(input: &str) -> Option<NormalizedScopeRef<'_>> {
    if input.is_empty()
        || input.trim() != input
        || input.starts_with('/')
        || input.contains("/Users/")
        || input.contains('\\')
        || input.chars().any(char::is_control)
    {
        return None;
    }
    let mut segments = Vec::new();
    for segment in input.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        segments.push(segment);
    }
    Some(NormalizedScopeRef { segments })
}
