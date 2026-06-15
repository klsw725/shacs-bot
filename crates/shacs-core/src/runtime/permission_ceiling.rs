use crate::runtime::{PermissionMode, PermissionedActionOrigin, SafetyCapability};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionCeilingSnapshot {
    pub parent_mode: PermissionMode,
    pub capability_ceiling: Vec<SafetyCapability>,
    pub approved_scope_refs: Vec<String>,
    pub origin: RuntimeBoundaryOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RuntimeBoundaryOrigin {
    UserTurn,
    Subagent {
        subagent_id: Option<String>,
    },
    CronWake {
        job_id: Option<String>,
        approval_ref: Option<String>,
    },
    LocalApi {
        request_id: Option<String>,
    },
    ChannelInbound {
        channel: String,
        message_id: Option<String>,
    },
    AppTask {
        app_id: Option<String>,
        task_id: Option<String>,
    },
    DeferredMcp {
        bridge_name: String,
        scope_digest: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InheritedPermissionContext {
    pub ceiling: PermissionCeilingSnapshot,
    pub requested_mode: PermissionMode,
    pub requested_capabilities: Vec<SafetyCapability>,
    pub per_action_evaluation_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryPermissionViolation {
    ModeWidening,
    CapabilityWidening,
    MissingApprovalRef,
    AppDeclarationOnly,
    DeferredGateBypass,
    StaleDecisionReuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeilingEvaluation {
    pub allowed: bool,
    pub violations: Vec<BoundaryPermissionViolation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LateResultPermissionDisposition {
    Executable,
    ClosedTurn,
    SupersededTurn,
    StaleDecisionReuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LateResultPermissionInput {
    pub turn_open: bool,
    pub active_turn_id: String,
    pub result_turn_id: String,
    pub decision_snapshot_digest: String,
    pub action_snapshot_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDeclarationPermissionInput {
    pub app_id: String,
    pub declared_capabilities: Vec<SafetyCapability>,
    pub requested_capabilities: Vec<SafetyCapability>,
}

pub fn ceiling_for_origin(
    parent_mode: PermissionMode,
    capability_ceiling: Vec<SafetyCapability>,
    approved_scope_refs: Vec<String>,
    origin: RuntimeBoundaryOrigin,
) -> PermissionCeilingSnapshot {
    PermissionCeilingSnapshot {
        parent_mode,
        capability_ceiling,
        approved_scope_refs,
        origin,
    }
}

pub fn boundary_origin_from_action(origin: &PermissionedActionOrigin) -> RuntimeBoundaryOrigin {
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

pub fn evaluate_inherited_ceiling(context: &InheritedPermissionContext) -> CeilingEvaluation {
    let mut violations = Vec::new();
    if mode_rank(context.requested_mode) > mode_rank(context.ceiling.parent_mode) {
        violations.push(BoundaryPermissionViolation::ModeWidening);
    }
    if context
        .requested_capabilities
        .iter()
        .any(|capability| !context.ceiling.capability_ceiling.contains(capability))
    {
        violations.push(BoundaryPermissionViolation::CapabilityWidening);
    }
    if matches!(
        context.ceiling.origin,
        RuntimeBoundaryOrigin::CronWake {
            approval_ref: None,
            ..
        }
    ) {
        violations.push(BoundaryPermissionViolation::MissingApprovalRef);
    }
    if matches!(
        context.ceiling.origin,
        RuntimeBoundaryOrigin::AppTask { .. }
    ) && context.ceiling.approved_scope_refs.is_empty()
    {
        violations.push(BoundaryPermissionViolation::AppDeclarationOnly);
    }
    if matches!(
        context.ceiling.origin,
        RuntimeBoundaryOrigin::DeferredMcp { .. }
    ) && (!context.per_action_evaluation_required
        || context.ceiling.approved_scope_refs.is_empty())
    {
        violations.push(BoundaryPermissionViolation::DeferredGateBypass);
    }
    CeilingEvaluation {
        allowed: violations.is_empty() && context.per_action_evaluation_required,
        violations,
    }
}

pub fn late_result_permission_disposition(
    input: &LateResultPermissionInput,
) -> LateResultPermissionDisposition {
    if !input.turn_open {
        return LateResultPermissionDisposition::ClosedTurn;
    }
    if input.active_turn_id != input.result_turn_id {
        return LateResultPermissionDisposition::SupersededTurn;
    }
    if input.decision_snapshot_digest != input.action_snapshot_digest {
        return LateResultPermissionDisposition::StaleDecisionReuse;
    }
    LateResultPermissionDisposition::Executable
}

pub fn app_declaration_grants_permission(_input: &AppDeclarationPermissionInput) -> bool {
    false
}

fn mode_rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::Plan => 0,
        PermissionMode::Default => 1,
        PermissionMode::AcceptEdits => 2,
        PermissionMode::Auto => 3,
        PermissionMode::DontAsk => 1,
        PermissionMode::BypassPermissions => 4,
    }
}
