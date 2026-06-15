# PRD 004. approval request cache and user decision correlation

## 구현 상태

Status: Implemented.

구현 증거는 `crates/shacs-core/src/runtime/permission_approval.rs`, `crates/shacs-core/src/runtime/permission_policy.rs`, `crates/shacs-core/src/runtime/tool_execution.rs`, `crates/shacs-core/src/runtime/tool_search.rs`, `crates/shacs-core/tests/permission_policy.rs`, `crates/shacs-core/tests/runtime.rs`다. 대표 테스트는 `approval_correlation_rejects_mismatched_expired_inspect_only_and_consumed`, `runtime_ask_user_skips_later_denied_tool_without_permission_message`, `bridge_ask_user_skips_later_denied_tool_without_permission_message`다.

이 closure는 typed approval request/decision correlation과 `ask` permission outcome의 non-execution 처리를 닫는다. TUI widget, channel별 button UI, hidden grant store 구현을 주장하지 않는다.

## 목표

이 문서는 `ask` decision을 사용자에게 보여 주고, 사용자의 decision을 action digest와 snapshot digest에 묶어 안전하게 소비하는 기준을 정의한다.

목표는 `ask_user` interruption과 formal approval을 구분하고, stale 또는 mismatched approval이 실행으로 이어지지 않게 하는 것이다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. PRD 001의 action digest와 snapshot digest를 소비한다.
3. PRD 003의 `ask` decision을 소비한다.
4. user-facing projection 의미는 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 따른다.
5. orchestrator stale decision 경계는 `docs/specs/007-main-orchestrator-policy/SPEC.md`를 따른다.

## Dependency Cut

1. 022는 formal approval request와 decision correlation을 소유한다.
2. 013은 user-facing rendering과 transport별 UI를 계속 소유한다.
3. 004 tool runtime의 `AskUserInterrupt`는 formal approval engine이 아니다.
4. 이 PRD는 evaluator 구현이나 static rule 구현을 다시 정의하지 않는다.

## 범위

1. `ApprovalRequest` 타입.
2. `ApprovalDecision` 타입.
3. Approval request id, action digest, snapshot digest, expiration correlation.
4. `inspect_only`와 acknowledgement의 비승인 의미.
5. User-facing approval prompt content.
6. Approval cache scope와 expiration.
7. Interactive/non-interactive fallback.

## 범위 제외

1. TUI widget 디자인.
2. Channel별 button rendering 세부 구현.
3. Long-lived hidden grant store.
4. 다중 사용자 승인 체인.
5. Secret vault implementation.

## 구현 요구사항

1. `ask` decision은 formal `ApprovalRequest`를 만들 수 있어야 한다.
2. Approval request는 request id, action digest, snapshot digest, requested scope, risk summary, allowed decisions, expires at을 가져야 한다.
3. Approval decision은 request id, decision, approved scope, actor local user, decided at을 가져야 한다.
4. Approval decision은 request id, action digest, snapshot digest가 일치할 때만 소비되어야 한다.
5. Expired approval은 실행으로 이어지면 안 된다.
6. `inspect_only`는 approval로 해석하면 안 된다.
7. Message acknowledgement와 channel read receipt는 approval로 해석하면 안 된다.
8. `ask_user` tool result는 formal approval correlation이 없으면 approval decision이 아니다.
9. Approval prompt는 action summary, target summary, risk summary, decision options, scope, expiration을 포함해야 한다.
10. Non-interactive `DontAsk` flow는 approval prompt를 만들지 않고 deny로 접어야 한다.
11. Auto-approved action도 user-facing summary에서 숨기면 안 된다.
12. Approval cache는 사용자에게 보이지 않는 장기 권한 grant를 만들면 안 된다.

## 데이터/상태 모델

1. `ApprovalRequest`: request id, action digest, snapshot digest, requested scope, risk summary, allowed decisions, expires at.
2. `ApprovalDecision`: request id, decision, approved scope, actor, decided at.
3. `ApprovalDecisionKind`: approved, denied, inspect only.
4. `ApprovalCacheEntry`: request ref, approved scope, expiration, consumed state.
5. `UserFacingPermissionStatus`: allowed automatically, waiting for approval, denied by policy, denied by scope, denied by protected target, evaluator unavailable, containment unknown.
6. `ApprovalCorrelationError`: request mismatch, action mismatch, snapshot mismatch, expired, consumed, inspect only.

## 정상 시퀀스

1. Runtime policy가 action을 `ask`로 결정한다.
2. Approval request를 만들고 risk summary를 사용자에게 보여 준다.
3. 사용자가 approved를 선택한다.
4. Runtime이 request id, action digest, snapshot digest, expiration을 검증한다.
5. 검증이 통과하면 해당 action 또는 approved scope 안에서 decision을 소비한다.
6. Tool runtime handoff는 새 final allow decision을 통해서만 일어난다.

## 실패 시퀀스

1. 사용자가 승인한 뒤 action arguments가 바뀐다.
2. Action digest mismatch가 발생한다.
3. Runtime은 approval을 소비하지 않고 deny 또는 새 ask로 접는다.
4. Approval이 만료된 뒤 도착한다.
5. Runtime은 expired rejection을 기록한다.
6. 사용자가 inspect evidence만 선택한다.
7. Runtime은 approval로 해석하지 않는다.

## 검증 관점

1. Action digest mismatch가 실행으로 이어지지 않는지 확인한다.
2. Snapshot digest mismatch가 실행으로 이어지지 않는지 확인한다.
3. Expired approval이 deny로 접히는지 확인한다.
4. `inspect_only`가 approval로 소비되지 않는지 확인한다.
5. Message acknowledgement가 approval로 해석되지 않는지 확인한다.
6. `ask_user` resume만으로 formal approval이 되지 않는지 확인한다.
7. Approval prompt가 필수 summary와 expiration을 포함하는지 확인한다.

## 완료 기준

1. Approval request와 decision correlation이 action digest와 snapshot digest로 고정된다.
2. Stale, expired, mismatched approval은 실행으로 이어지지 않는다.
3. User-facing approval 상태가 013 projection에서 소비 가능하다.
4. `ask_user`, acknowledgement, inspect-only decision이 approval과 분리된다.
5. Hidden long-lived grant 없이 scoped approval만 소비된다.
