# PRD 003. auto evaluator and mode policy

## 구현 상태

Status: Implemented.

구현 증거는 `crates/shacs-core/src/runtime/permission_policy.rs`, `crates/shacs-core/src/runtime/permission_rules.rs`, `crates/shacs-core/src/runtime/tool_execution.rs`, `crates/shacs-core/src/runtime/tool_search.rs`, `crates/shacs-core/tests/permission_policy.rs`, `crates/shacs-core/tests/runtime.rs`다. 대표 테스트는 `evaluator_uncertainty_and_prompt_injection_never_allow`, `runtime_denies_direct_proc_exec_without_executing_tool`, `bridge_denies_deferred_proc_exec_without_executing_underlying_tool`다.

이 closure는 evaluator verdict를 권한 확정자가 아닌 advisory signal로 소비하는 policy table과 runtime handoff gate를 닫는다. Evaluator provider routing이나 provider-specific trace 구현을 주장하지 않는다.

## 목표

이 문서는 permission mode, static rule, protected target, auto evaluator verdict를 합성해 최종 `allow | ask | deny` decision을 만드는 runtime policy 기준을 정의한다.

목표는 evaluator를 권한 확정자가 아니라 advisory signal로 제한하면서, `auto` mode에서만 안전하게 prompt fatigue를 줄이는 것이다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. PRD 000의 mode snapshot을 소비한다.
3. PRD 001의 `PermissionedAction`과 digest를 소비한다.
4. PRD 002의 capability, protected target, containment decision을 소비한다.
5. evaluator envelope와 ledger 언어는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`를 따른다.
6. orchestrator final authority는 `docs/specs/007-main-orchestrator-policy/SPEC.md`를 따른다.

## Dependency Cut

1. 018은 evaluator output 형식과 ledger evidence 언어를 제공한다.
2. 022는 evaluator verdict를 permission decision으로 합성하는 rule을 소유한다.
3. 007은 최종 실행 허용 권한이 runtime policy에 있음을 소유한다.
4. 이 PRD는 approval request 저장과 user prompt rendering을 구현하지 않는다.
5. 이 PRD는 audit persistence backend를 구현하지 않는다.

## 범위

1. Runtime decision pipeline.
2. Auto evaluator input/output contract.
3. Permission mode baseline decision table.
4. Evaluator failure fallback.
5. Prompt injection suspicion handling.
6. `allow | ask | deny` final decision shape.
7. Tool runtime handoff rule.

## 범위 제외

1. Evaluator provider 선택과 model routing 세부 구현.
2. UI prompt rendering.
3. Approval cache persistence.
4. Audit storage physical backend.
5. 원격 운영 콘솔과 다중 사용자 권한 관리.

## 구현 요구사항

1. Runtime policy는 static deny, protected target, permission mode baseline, explicit allow/ask rule, evaluator verdict, approval state를 순서대로 합성해야 한다.
2. `deny`는 항상 `allow`보다 우선해야 한다.
3. Evaluator는 `allow_candidate`, `ask_user`, `deny_candidate`, `insufficient_context` 중 하나를 반환해야 한다.
4. Evaluator output은 confidence, scope match, risk summary, evidence refs, expiration을 포함해야 한다.
5. Evaluator `allow_candidate`는 최종 allow가 아니다.
6. Runtime policy는 protected target, denied mode, stale snapshot, insufficient context를 evaluator allow보다 우선해야 한다.
7. Evaluator timeout, parse failure, low confidence, prompt injection suspicion은 자동 allow로 이어지면 안 된다.
8. `Plan` mode는 write, exec, delivery, schedule을 deny 또는 ask가 아니라 실행 불가로 접어야 한다.
9. `Default` mode는 low-risk read와 명시 allow rule 외 side effect를 ask로 보낸다.
10. `AcceptEdits` mode는 workspace 일반 file edit/write만 allow 후보로 삼는다.
11. `Auto` mode는 evaluator와 rule이 모두 허용할 때만 allow를 만든다.
12. `DontAsk` mode는 unresolved ask를 deny로 바꾼다.
13. `BypassPermissions` mode도 circuit breaker target과 containment precondition을 우회하지 않는다.
14. Final decision이 `allow`일 때만 tool runtime으로 action을 넘겨야 한다.

## 데이터/상태 모델

1. `AutoEvaluatorInput`: user intent summary, action, capability classification, target summary, containment snapshot, mode snapshot, prompt injection signals.
2. `AutoEvaluatorVerdict`: verdict, confidence, scope match, risk summary, evidence refs, expires at.
3. `PermissionPolicyDecision`: allow, ask, deny, reason, evaluator ref, approval requirement, diagnostics.
4. `PermissionPolicyReason`: mode baseline, static deny, protected target, evaluator allow, evaluator fail, approval required, containment unknown.
5. `PromptInjectionSignal`: source ref, reason, confidence.

## 정상 시퀀스

1. Action과 rule classification이 runtime policy에 들어온다.
2. Static deny가 없고 protected target도 아니다.
3. Active mode가 `Auto`다.
4. Runtime policy가 evaluator input을 만든다.
5. Evaluator가 high confidence `allow_candidate`와 requested scope match를 반환한다.
6. Runtime policy가 mode, rule, evaluator, snapshot expiration을 다시 확인한다.
7. Final decision을 `allow`로 만들고 tool runtime에 넘길 수 있다.

## 실패 시퀀스

1. Evaluator가 `allow_candidate`를 반환하지만 target이 protected target이다.
2. Runtime policy가 protected target rule을 우선해 ask 또는 deny로 접는다.
3. Evaluator output parse가 실패한다.
4. Interactive session이면 ask, non-interactive면 deny로 접는다.
5. Web content에서 prompt injection 의심 signal이 있다.
6. Runtime policy는 silent allow를 만들지 않는다.

## 검증 관점

1. Evaluator allow가 protected target deny를 우회하지 못하는지 확인한다.
2. Evaluator timeout이 allow로 이어지지 않는지 확인한다.
3. Low confidence verdict가 ask 또는 deny로 접히는지 확인한다.
4. `DontAsk`에서 unresolved ask가 deny로 바뀌는지 확인한다.
5. `AcceptEdits`가 `proc_exec`를 mode만으로 allow하지 않는지 확인한다.
6. `Auto` mode가 evaluator 없이 side effect를 silent allow하지 않는지 확인한다.
7. `allow` decision만 tool runtime handoff를 만드는지 확인한다.

## 완료 기준

1. Runtime policy decision table이 구현과 테스트로 고정된다.
2. Evaluator는 advisory signal로만 소비된다.
3. `auto` mode의 silent execution은 rule, mode, evaluator, snapshot 조건을 모두 통과해야 한다.
4. Evaluator failure와 uncertainty는 allow가 아닌 ask 또는 deny로 접힌다.
5. Prompt injection suspicion이 silent allow를 막는다.
