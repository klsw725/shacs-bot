# PRD 001. permissioned action normalization and decision gate

## 목표

이 문서는 provider tool call, bridge tool call, deferred MCP tool call을 실행 직전 `PermissionedAction`으로 정규화하는 기준을 정의한다.

목표는 tool runtime 앞에 단일 decision gate가 소비할 action envelope와 digest를 만드는 것이다.

이 단계는 decision을 최종 실행에 연결하지 않는다. 실제 `allow | ask | deny` 합성은 후속 PRD가 소유한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. tool runtime 경계는 `docs/specs/004-tool-runtime/SPEC.md`를 따른다.
3. session과 turn identity는 `docs/specs/001-session-kernel/SPEC.md`와 `docs/specs/006-session-store/SPEC.md`를 따른다.
4. deferred tool과 bridge surface는 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`를 따른다.
5. PRD 000의 `PermissionMode`와 `SafetyCapability`를 소비한다.

## Dependency Cut

1. 004는 tool executor와 `RuntimeToolCall` 경계를 계속 소유한다.
2. 020은 provider-visible deferred tool schema와 bridge dispatch 의미를 계속 소유한다.
3. 022는 실제 실행 전 permission 판단에 필요한 action envelope를 소유한다.
4. 이 PRD는 capability 세부 분류, Docker containment, evaluator, approval cache를 구현하지 않는다.

## 범위

1. `PermissionedAction` 타입 정의.
2. `action_id`, `argument_digest`, `snapshot_digest` 생성 기준 정의.
3. tool name, arguments schema, origin, target refs 정규화.
4. provider tool call과 deferred tool call을 같은 envelope로 표현.
5. unknown tool, malformed arguments, missing id 처리 기준.
6. redacted snapshot과 raw secret 비저장 기준.
7. decision gate 입력 타입 정의.

## 범위 제외

1. Capability classification detail.
2. Static deny rule과 protected target 판정.
3. Auto evaluator 호출.
4. Approval request 생성.
5. Tool 실행 허용 여부 최종 결정.
6. Provider adapter wire format 변경.
7. 원격 운영 콘솔과 다중 사용자 권한 관리.

## 구현 요구사항

1. 모든 provider tool call은 tool runtime 실행 전에 `PermissionedAction`으로 변환되어야 한다.
2. Deferred MCP 또는 tool-search bridge call도 underlying tool 실행 전에 같은 변환을 거쳐야 한다.
3. `PermissionedAction`은 action id, session id, turn id, tool name, capabilities, target refs, argument digest, origin, permission mode snapshot, containment snapshot ref, intent snapshot ref를 담을 수 있어야 한다.
4. Tool arguments digest는 raw secret value를 포함하지 않는 canonical redacted representation으로 계산해야 한다.
5. `snapshot_digest`는 permission mode snapshot과 action-relevant context가 바뀌면 달라져야 한다.
6. Unknown tool은 panic이 아니라 permission gate의 deny candidate가 되어야 한다.
7. Malformed arguments는 provider에 바로 실행되지 않고 denied outcome 또는 tool error로 정규화 가능한 상태가 되어야 한다.
8. Action origin은 user turn, subagent, cron wake, app task, local API, channel inbound, deferred bridge 중 하나 이상으로 표현되어야 한다.
9. `ask_user` tool call은 user interruption 후보일 수 있지만 formal approval decision으로 해석하면 안 된다.
10. Decision gate input은 tool executor가 아닌 orchestrator 또는 runtime policy layer에서 만들어야 한다.

## 데이터/상태 모델

1. `PermissionedAction`: 실행 직전 permission 판단용 envelope.
2. `PermissionedActionOrigin`: user turn, subagent, cron, app task, local API, channel inbound, deferred bridge.
3. `ActionDigest`: tool name, redacted arguments, target refs, capability set의 stable digest.
4. `PermissionSnapshotDigest`: mode snapshot과 containment snapshot의 stable digest.
5. `PermissionDecisionInput`: action, mode snapshot, optional prior approval refs, diagnostics sink.
6. `ActionNormalizationError`: unknown tool, invalid arguments, missing id, unsafe raw secret, unsupported origin.

## 정상 시퀀스

1. Provider가 tool call을 반환한다.
2. Runner가 tool name과 arguments를 읽는다.
3. Runtime policy layer가 `PermissionedAction`을 만든다.
4. Redacted argument digest와 snapshot digest를 계산한다.
5. Action origin과 target refs를 기록한다.
6. Decision gate가 action을 후속 policy에 넘길 수 있다.
7. 이 PRD 단계에서는 실제 tool 실행 behavior가 아직 바뀌지 않는다.

## 실패 시퀀스

1. Provider가 unknown tool name을 반환한다.
2. Normalizer가 unknown tool action으로 기록하고 deny candidate를 만든다.
3. Arguments가 schema에 맞지 않는다.
4. Normalizer가 invalid argument outcome을 만들 수 있는 error로 접는다.
5. Raw secret-like value가 digest material에 포함되려 한다.
6. Normalizer가 redaction failure로 action을 allow 후보로 만들지 않는다.

## 검증 관점

1. 일반 provider tool call이 `PermissionedAction`으로 변환되는지 확인한다.
2. Deferred MCP tool call이 같은 envelope를 거치는지 확인한다.
3. Unknown tool이 panic이나 direct execution으로 이어지지 않는지 확인한다.
4. Malformed arguments가 direct execution으로 이어지지 않는지 확인한다.
5. Argument digest가 raw secret을 포함하지 않는지 확인한다.
6. Snapshot 변경이 digest 변경으로 이어지는지 확인한다.
7. `ask_user`가 formal approval로 소비되지 않는지 확인한다.

## 완료 기준

1. 모든 tool 실행 후보는 `PermissionedAction`으로 표현될 수 있다.
2. Action digest와 snapshot digest가 approval correlation에 쓸 수 있게 stable하다.
3. Deferred tool과 direct tool이 같은 permission gate 입력을 쓴다.
4. Unknown tool과 malformed argument가 safe failure로 정규화된다.
5. Raw secret이 action digest, snapshot, diagnostics에 저장되지 않는다.
