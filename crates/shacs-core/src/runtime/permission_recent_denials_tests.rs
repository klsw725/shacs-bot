use super::*;

fn denial(id: &str) -> RecentAutoModeDenial {
    RecentAutoModeDenial {
        denial_id: id.to_owned(),
        created_at_unix_ms: 1,
        session_digest: "session".to_owned(),
        turn_digest: "turn".to_owned(),
        tool_name: "exec".to_owned(),
        capabilities: vec![SafetyCapability::ProcExec],
        target_summary: Vec::new(),
        action_digest: "action".to_owned(),
        argument_digest: "arguments".to_owned(),
        snapshot_digest: "snapshot".to_owned(),
        policy_safety_snapshot_ref: None,
        secret_ref_evidence: Vec::new(),
        decision_reason: PermissionPolicyReason::EvaluatorUncertain,
        classifier_rule: Some("[Data Exfiltration]".to_owned()),
        classifier_verdict: AutoEvaluatorVerdictKind::DenyCandidate,
        classifier_confidence: EvaluatorConfidence::High,
        classifier_scope_match: EvaluatorScopeMatch::Requested,
        retryable: true,
    }
}

fn store_with(id: &str, expires_at_unix_ms: u64) -> RecentAutoModeRetryTokenStore {
    let mut store = RecentAutoModeRetryTokenStore::default();
    store.insert(RecentAutoModeRetryToken::new(
        &denial(id),
        RuntimeToolCall::new("c1", "exec", json!({"command": "ls"})),
        ToolExecutionContext::default(),
        expires_at_unix_ms,
    ));
    store
}

#[test]
fn an_old_token_serves_auto_mode_but_not_the_one_shot_rerun() {
    // Given
    let held = denial("auto_denial_old");
    let mut rerun = store_with("auto_denial_old", 10);
    let mut retry = store_with("auto_denial_old", 10);

    // When
    let peeked = rerun.peek("auto_denial_old", 11).map(|_| ());
    let consumed = rerun
        .consume(
            "auto_denial_old",
            RecentAutoModeRetryTokenMatch::from_denial(&held),
            11,
        )
        .map(|_| ());
    let held_before = retry.holds("auto_denial_old");
    let peeked_any_age = retry.peek_any_age("auto_denial_old").map(|_| ());
    let consumed_any_age = retry
        .consume_any_age(
            "auto_denial_old",
            RecentAutoModeRetryTokenMatch::from_denial(&held),
        )
        .map(|_| ());

    // Then
    assert_eq!(peeked, Err(RecentAutoModeRetryTokenConsumeError::Expired));
    assert_eq!(consumed, Err(RecentAutoModeRetryTokenConsumeError::Expired));
    assert!(held_before);
    assert_eq!(peeked_any_age, Ok(()));
    assert_eq!(consumed_any_age, Ok(()));
    assert!(!retry.holds("auto_denial_old"));
    assert_eq!(
        retry.peek_any_age("auto_denial_old").map(|_| ()),
        Err(RecentAutoModeRetryTokenConsumeError::Consumed)
    );
}

#[test]
fn an_any_age_consume_still_checks_the_digests() {
    // Given
    let mut store = store_with("auto_denial_other", u64::MAX);
    let mut other = denial("auto_denial_other");
    other.argument_digest = "different".to_owned();

    // When
    let consumed = store
        .consume_any_age(
            "auto_denial_other",
            RecentAutoModeRetryTokenMatch::from_denial(&other),
        )
        .map(|_| ());

    // Then
    assert_eq!(
        consumed,
        Err(RecentAutoModeRetryTokenConsumeError::Mismatched)
    );
    assert!(!store.holds("auto_denial_other"));
}

#[test]
fn a_record_keeps_the_rule_name_of_a_block_summary_only() {
    // Given
    let named = "[Data Exfiltration] sends notes";
    let unnamed = "sends notes [Data Exfiltration]";

    // When / Then
    assert_eq!(blocked_call::rule(named), Some("[Data Exfiltration]"));
    assert_eq!(blocked_call::rule(unnamed), None);
}
