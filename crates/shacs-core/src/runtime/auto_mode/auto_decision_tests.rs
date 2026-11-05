use super::*;
use crate::runtime::{
    decide_permission, normalize_runtime_tool_call, ApprovalCorrelation, AutoEvaluatorVerdict,
    AutoEvaluatorVerdictKind, AutoModeEscalation, EvaluatorConfidence, EvaluatorScopeMatch,
    PermissionModeSnapshot, PermissionedActionInput, PermissionedActionOrigin, RuleDiagnostics,
    RuntimeToolCall, StaticRuleDecision,
};
use crate::tools::ToolRegistry;
use serde_json::json;

fn input(
    tool: &str,
    mode: PermissionMode,
    materials: PermissionMaterials,
    interactive: bool,
) -> PermissionPolicyInput {
    let action = normalize_runtime_tool_call(
        &ToolRegistry::new(),
        &RuntimeToolCall::new("call_1", tool, json!({"path": "a", "command": "ls"})),
        PermissionedActionInput {
            session_id: "cli:direct".to_owned(),
            turn_id: "turn_1".to_owned(),
            origin: PermissionedActionOrigin::UserTurn,
            permission_mode_snapshot: PermissionModeSnapshot {
                mode,
                source: None,
                scope_ref: None,
            },
            containment_snapshot: None,
            intent_snapshot: None,
            permission_environment: None,
        },
    );
    PermissionPolicyInput {
        action,
        static_rule_decision: StaticRuleDecision {
            kind: StaticRuleDecisionKind::AllowCandidate,
            reason: StaticRuleReason::NoStaticMatch,
            diagnostics: RuleDiagnostics {
                rule_version: "test".to_owned(),
                matched_rules: Vec::new(),
                protected_targets: Vec::new(),
                containment_warning: None,
                unknown_classification: false,
            },
        },
        evaluator: None,
        approval: None,
        inherited_context: None,
        remembered_rules: Vec::new(),
        remembered_store_unavailable: false,
        interactive,
        materials: Some(materials),
    }
}

fn outcome(input: PermissionPolicyInput) -> (PermissionPolicyDecisionKind, PermissionPolicyReason) {
    let decision = decide_permission(input);
    (decision.kind, decision.reason)
}

fn approved(mut input: PermissionPolicyInput) -> PermissionPolicyInput {
    input.approval = Some(ApprovalCorrelation::approved("approval_1".to_owned()));
    if let Some(materials) = input.materials.as_mut() {
        materials.this_call_approved = true;
    }
    input
}

use PermissionPolicyDecisionKind::{Allow, Ask, Deny};

#[test]
fn auto_mode_follows_the_decision_table_order() {
    let auto = PermissionMode::Auto;
    let secret = PermissionMaterials {
        secret_read: true,
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(input(
            "read_file",
            auto,
            PermissionMaterials::default(),
            true
        )),
        (Allow, PermissionPolicyReason::WorkspaceRead)
    );
    assert_eq!(
        outcome(input("read_file", auto, secret.clone(), true)),
        (Ask, PermissionPolicyReason::SecretFileRead)
    );
    assert_eq!(
        outcome(input("read_file", auto, secret.clone(), false)),
        (Deny, PermissionPolicyReason::SecretFileRead)
    );
    assert_eq!(
        outcome(approved(input("read_file", auto, secret, true))).0,
        Allow
    );
    let write_secret = PermissionMaterials {
        secret_write: true,
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(approved(input("write_file", auto, write_secret, true))),
        (Deny, PermissionPolicyReason::SecretFileWrite)
    );
    let critical_with_rule = PermissionMaterials {
        critical_removal: Some("/".to_owned()),
        auto_allow: Some(AutoAllow::Rule {
            rule: "exec(rm *)".to_owned(),
        }),
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(input("exec", auto, critical_with_rule, true)),
        (Ask, PermissionPolicyReason::CriticalPathRemoval)
    );
    let refused = PermissionMaterials {
        preflight_refusal: Some("Error: blocked".to_owned()),
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(approved(input("exec", auto, refused, true))),
        (Deny, PermissionPolicyReason::ToolRefusal)
    );
}

#[test]
fn auto_mode_sends_the_rest_to_the_classifier() {
    // Given
    let mut reviewed = input(
        "exec",
        PermissionMode::Auto,
        PermissionMaterials::default(),
        true,
    );

    // When
    let unreviewed = outcome(reviewed.clone());
    reviewed.evaluator = Some(AutoEvaluatorVerdict {
        verdict: AutoEvaluatorVerdictKind::AllowCandidate,
        confidence: EvaluatorConfidence::High,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: "requested".to_owned(),
        evidence_refs: Vec::new(),
        expires_at_unix_ms: u64::MAX,
        evaluator_ref: Some("auto-mode-classifier".to_owned()),
        prompt_injection_signals: Vec::new(),
    });

    // Then
    assert_eq!(
        unreviewed,
        (Deny, PermissionPolicyReason::EvaluatorUnavailable)
    );
    assert_eq!(
        outcome(reviewed),
        (Allow, PermissionPolicyReason::EvaluatorAllow)
    );
}

fn verdict(
    kind: AutoEvaluatorVerdictKind,
    confidence: EvaluatorConfidence,
) -> AutoEvaluatorVerdict {
    AutoEvaluatorVerdict {
        verdict: kind,
        confidence,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: "[Data Exfiltration] sends notes".to_owned(),
        evidence_refs: Vec::new(),
        expires_at_unix_ms: u64::MAX,
        evaluator_ref: Some("auto-mode-classifier".to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

#[test]
fn row_sixteen_refuses_what_it_does_not_allow_and_asks_once_escalated() {
    let high = EvaluatorConfidence::High;
    let mut signalled = verdict(AutoEvaluatorVerdictKind::AllowCandidate, high);
    signalled.prompt_injection_signals = vec![crate::runtime::PromptInjectionSignal {
        source_ref: "web".to_owned(),
        reason: "instructions".to_owned(),
        confidence: high,
    }];
    let cases = [
        (None, PermissionPolicyReason::EvaluatorUnavailable),
        (
            Some(verdict(AutoEvaluatorVerdictKind::DenyCandidate, high)),
            PermissionPolicyReason::EvaluatorUncertain,
        ),
        (
            Some(verdict(
                AutoEvaluatorVerdictKind::Timeout,
                EvaluatorConfidence::Unknown,
            )),
            PermissionPolicyReason::EvaluatorUncertain,
        ),
        (
            Some(verdict(AutoEvaluatorVerdictKind::AskUser, high)),
            PermissionPolicyReason::EvaluatorUncertain,
        ),
        (
            Some(verdict(
                AutoEvaluatorVerdictKind::AllowCandidate,
                EvaluatorConfidence::Medium,
            )),
            PermissionPolicyReason::EvaluatorUncertain,
        ),
        (Some(signalled), PermissionPolicyReason::EvaluatorUncertain),
    ];
    for (given, reason) in cases {
        for interactive in [true, false] {
            for escalation in [None, Some(AutoModeEscalation::Consecutive)] {
                // Given
                let materials = PermissionMaterials {
                    auto_mode_escalation: escalation,
                    ..PermissionMaterials::default()
                };
                let mut call = input("exec", PermissionMode::Auto, materials, interactive);
                call.evaluator = given.clone();

                // When
                let decided = outcome(call);

                // Then
                let kind = if escalation.is_some() && interactive {
                    Ask
                } else {
                    Deny
                };
                assert_eq!(decided, (kind, reason.clone()), "{given:?} {escalation:?}");
            }
        }
    }
}

#[test]
fn an_escalation_changes_nothing_but_row_sixteen() {
    // Given
    let escalated = |materials: PermissionMaterials| PermissionMaterials {
        auto_mode_escalation: Some(AutoModeEscalation::Total),
        ..materials
    };
    let mut allowed = input(
        "exec",
        PermissionMode::Auto,
        escalated(PermissionMaterials::default()),
        true,
    );
    allowed.evaluator = Some(verdict(
        AutoEvaluatorVerdictKind::AllowCandidate,
        EvaluatorConfidence::High,
    ));
    let read = input(
        "read_file",
        PermissionMode::Auto,
        escalated(PermissionMaterials::default()),
        false,
    );
    let denied = input(
        "exec",
        PermissionMode::Auto,
        escalated(PermissionMaterials {
            deny_rule: Some("exec(ls)".to_owned()),
            ..PermissionMaterials::default()
        }),
        true,
    );

    // When / Then
    assert_eq!(
        outcome(allowed),
        (Allow, PermissionPolicyReason::EvaluatorAllow)
    );
    assert_eq!(
        outcome(read),
        (Allow, PermissionPolicyReason::WorkspaceRead)
    );
    assert_eq!(
        outcome(denied),
        (Deny, PermissionPolicyReason::UserRuleDeny)
    );
}

#[test]
fn other_modes_gain_deny_rules_secret_reads_critical_paths_and_ask_rules() {
    let secret = PermissionMaterials {
        secret_read: true,
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(input(
            "read_file",
            PermissionMode::Default,
            secret.clone(),
            true
        )),
        (Ask, PermissionPolicyReason::SecretFileRead)
    );
    assert_eq!(
        outcome(input(
            "read_file",
            PermissionMode::DontAsk,
            secret.clone(),
            true
        )),
        (Deny, PermissionPolicyReason::SecretFileRead)
    );
    assert_eq!(
        outcome(approved(input(
            "read_file",
            PermissionMode::Default,
            secret,
            true
        ))),
        (Allow, PermissionPolicyReason::ApprovalAccepted)
    );
    let denied = PermissionMaterials {
        deny_rule: Some("exec(ls)".to_owned()),
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(input(
            "exec",
            PermissionMode::BypassPermissions,
            denied,
            true
        )),
        (Deny, PermissionPolicyReason::UserRuleDeny)
    );
    let asked = PermissionMaterials {
        ask_rule: Some("read(a)".to_owned()),
        ..PermissionMaterials::default()
    };
    assert_eq!(
        outcome(input("read_file", PermissionMode::Plan, asked, true)),
        (Ask, PermissionPolicyReason::UserRuleAsk)
    );
}
