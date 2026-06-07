# PRD 005. inherited permission ceilings for runtime boundaries

## 목표

이 문서는 subagent, cron/background run, local API, channel inbound, app task, MCP/deferred tool call이 parent 또는 session permission ceiling을 넓히지 못하게 하는 기준을 정의한다.

목표는 auto approval이 main turn에서만 안전한 척하고 다른 runtime boundary에서 우회되는 일을 막는 것이다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. PRD 000의 mode와 source boundary를 소비한다.
3. PRD 001의 action envelope를 소비한다.
4. PRD 003의 runtime decision pipeline을 소비한다.
5. subagent runtime은 `docs/specs/011-subagent-runtime/SPEC.md`를 따른다.
6. runtime service와 background wake는 `docs/specs/012-runtime-services/SPEC.md`를 따른다.
7. app runtime과 manifest permission declaration은 `docs/specs/017-app-operating-environment/SPEC.md`를 따른다.
8. deferred tool surface는 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`를 따른다.

## Dependency Cut

1. 011은 child lifecycle과 result merge를 계속 소유한다.
2. 012는 service wake와 channel runtime boundary를 계속 소유한다.
3. 017은 app install/runtime lifecycle을 계속 소유한다.
4. 020은 deferred tool catalog와 bridge call scope를 계속 소유한다.
5. 022는 permission ceiling inheritance와 non-widening rule을 소유한다.

## 범위

1. Parent permission ceiling snapshot.
2. Child mode non-widening rule.
3. Subagent per-action evaluation.
4. Cron and automation rule approval ref requirement.
5. Local API request-level permission snapshot.
6. Channel inbound external delivery restriction.
7. App permission declaration non-approval rule.
8. Deferred MCP and bridge same-gate enforcement.
9. Late result permission decision reuse rejection.

## 범위 제외

1. Subagent execution implementation detail.
2. Channel UI rendering.
3. App install implementation.
4. MCP server process implementation.
5. Automation scheduler physical backend.
6. 원격 운영 콘솔과 다중 사용자 권한 관리.

## 구현 요구사항

1. Subagent는 parent보다 넓은 `PermissionMode`를 가질 수 없어야 한다.
2. Parent가 `Default`이면 child가 `Auto`로 승격하면 안 된다.
3. Parent가 `Auto`여도 child action은 별도 `PermissionedAction`으로 평가되어야 한다.
4. Child system prompt는 permission grant source가 될 수 없다.
5. Child result는 parent approval decision을 새로 만들 수 없다.
6. Cron wake는 승인된 automation rule ref 또는 explicit user-approved source가 있어야 permissioned action으로 승격될 수 있다.
7. Local API background request는 request-level permission mode snapshot을 가져야 한다.
8. Channel inbound는 해당 session/channel scope를 벗어난 external delivery를 자동 승인하면 안 된다.
9. App manifest permission declaration은 approval grant가 아니다.
10. App task는 app permission declaration과 user approval state를 모두 통과해야 한다.
11. Deferred MCP tool call은 provider-visible bridge 단계가 아니라 underlying tool 실행 직전에 같은 permission gate를 통과해야 한다.
12. Closed turn 또는 superseded action의 permission decision은 late result에 재사용하면 안 된다.

## 데이터/상태 모델

1. `PermissionCeilingSnapshot`: parent mode, capability ceiling, approved scope refs, origin.
2. `InheritedPermissionContext`: child/session/background run이 소비하는 ceiling snapshot.
3. `RuntimeBoundaryOrigin`: subagent, cron wake, local API, channel inbound, app task, deferred MCP.
4. `AutomationApprovalRef`: approved rule id, scope, expiration, actor.
5. `BoundaryPermissionViolation`: mode widening, missing approval ref, stale decision reuse, app declaration only, deferred gate bypass.
6. `LateResultPermissionDisposition`: discard, observe only, require new decision.

## 정상 시퀀스

1. Parent turn이 subagent를 spawn한다.
2. Runtime이 parent permission ceiling snapshot을 child context에 넣는다.
3. Child가 tool call을 만든다.
4. Tool call은 child origin의 `PermissionedAction`으로 정규화된다.
5. Runtime policy가 parent ceiling과 child action을 함께 평가한다.
6. Child action이 parent scope 안이면 후속 decision pipeline으로 넘어간다.

## 실패 시퀀스

1. Child가 parent `Default` mode에서 `Auto`를 요청한다.
2. Runtime은 mode widening으로 deny한다.
3. Cron wake가 approved automation rule ref 없이 실행을 요청한다.
4. Runtime은 background non-interactive deny로 접는다.
5. Deferred MCP bridge call이 permission gate 없이 underlying tool을 실행하려 한다.
6. Runtime은 gate bypass violation으로 deny한다.

## 검증 관점

1. Child mode widening이 deny되는지 확인한다.
2. Parent `Default`에서 child `Auto` 승격이 거부되는지 확인한다.
3. Parent `Auto`에서도 child action이 별도 평가되는지 확인한다.
4. Cron wake가 approved rule ref 없이 deny되는지 확인한다.
5. Local API background request가 permission snapshot 없이 실행되지 않는지 확인한다.
6. App permission declaration만으로 approval이 생기지 않는지 확인한다.
7. Deferred MCP call이 same gate를 통과하는지 확인한다.
8. Late result가 closed turn decision을 재사용하지 않는지 확인한다.

## 완료 기준

1. 모든 runtime boundary가 permission ceiling snapshot을 소비한다.
2. Subagent와 background run은 parent/session permission을 넓힐 수 없다.
3. App, MCP, deferred tool call은 declaration이나 bridge visibility만으로 approval을 얻지 않는다.
4. Late result는 stale permission decision으로 실행되지 않는다.
5. Boundary violation은 user-visible denied outcome과 diagnostics로 남는다.
