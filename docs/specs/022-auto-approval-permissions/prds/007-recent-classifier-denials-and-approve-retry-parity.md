# PRD 007. recent classifier denials and approve retry parity

## 구현 상태

Status: Implemented. Bounded digest-only sanitized recent denial visibility, `/permission recent`, process-local exact one-shot retry token, and formal approve/retry execution are implemented.

이 PRD는 Claude Code식 auto mode recent denial UX를 `shacs-bot`의 self-hosted / personal-use permission model에 맞춰 정의한다. 목표는 classifier가 거절한 최근 action을 사용자가 나중에 확인하고, formal approval correlation을 통과한 exact one-shot retry만 허용하는 것이다.

## 목표

1. Auto mode classifier가 `deny_candidate`를 낸 최근 action을 bounded list로 보존한다.
2. Recent denial record는 사용자에게 설명 가능한 sanitized summary만 담는다.
3. 사용자는 recent denial을 확인하고 exact action에 대해 한 번만 retry approval을 줄 수 있다.
4. Retry는 existing approval request/decision correlation을 통과해야 하며 permission bypass가 아니다.
5. Hard deny, protected target deny, static rule deny, classifier failure는 retryable recent denial로 취급하지 않는다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. PRD 001의 action digest와 snapshot digest를 소비한다.
3. PRD 003의 evaluator/classifier verdict contract를 소비한다.
4. PRD 004의 formal approval request와 decision correlation을 소비한다.
5. PRD 006의 audit, diagnostics, redaction 기준을 소비한다.

## Dependency Cut

1. 이 PRD는 recent denial record와 retry token 계약을 소유한다.
2. PRD 004는 retry approval이 formal approval correlation을 통과해야 한다는 규칙을 계속 소유한다.
3. PRD 006은 persisted summary와 diagnostics redaction 기준을 계속 소유한다.
4. 013은 TUI/widget/channel별 rendering을 계속 소유한다. 이 PRD는 UI 디자인을 정의하지 않는다.
5. Deferred bridge classifier routing은 resolved bridge `tool_call` underlying action 기준으로 구현됐다.

## 범위

1. Recent classifier denial record type.
2. Bounded in-memory 또는 session metadata summary list.
3. Process-local exact retry token.
4. Retry approval request 생성과 one-shot consumption.
5. Redaction and non-persistence rules.
6. CLI/runtime permission surface에서 recent denial inspection.

## 범위 제외

1. Persisted executable retry payload.
2. Hidden allow rule 또는 permanent permission grant 생성.
3. Classifier parser 완화.
4. Static hard deny/protected target retry.
5. Raw command, raw arguments, raw prompt, raw classifier response 저장.
6. 원격 운영 콘솔과 다중 사용자 approval chain.

## 구현 요구사항

1. Recent denial은 final decision이 classifier-origin `deny_candidate`일 때만 기록한다.
2. Static deny, protected target deny, mode deny, parser failure, provider error, low confidence, scope mismatch는 recent classifier denial retry 대상이 아니다.
3. Recent denial list는 최신순으로 유지하며 최대 20개만 보존한다.
4. Persisted summary에는 tool name, capability set, session digest, turn digest, action digest, argument digest, snapshot digest, sanitized target summary, classifier reason category, created at, retryability만 담는다.
5. Persisted summary에는 raw session id, raw turn id, raw command string, raw arguments, raw prompt, raw classifier response, secrets, env, config value, host path를 저장하지 않는다.
6. Retry에 필요한 executable payload는 process-local token으로만 보관한다.
7. Process-local retry token은 original action digest, argument digest, snapshot digest, expiry, original runtime tool call, execution context ref를 포함한다. Approval request id와 pending status는 persisted pending approval metadata에 분리 저장하고, consumed flag는 process-local token store entry가 관리한다.
8. Retry는 user-local formal approval decision이 request id, action digest, snapshot digest, scope, expiry를 모두 통과할 때만 실행된다.
9. Retry token은 one-shot이다. 성공, 실패, mismatch, expiry 중 어느 경우든 consumed 또는 invalidated 상태가 되어야 한다.
10. Non-interactive context에서 retry approval을 새로 요구해야 하면 fail-closed deny로 접는다.
11. Recent denial inspection은 raw executable payload 없이도 왜 거절됐는지 설명해야 한다.
12. Retry 실행은 기존 approved permission tool execution path를 사용해야 하며 별도 bypass path를 만들면 안 된다.

## 데이터/상태 모델

1. `RecentAutoModeDenial`: denial id, created at, session digest, turn digest, tool name, capabilities, sanitized target summary, action digest, argument digest, snapshot digest, decision reason, classifier verdict, classifier confidence, classifier scope match, retryable. Persisted record는 raw session id 또는 raw turn id를 저장하지 않는다.
2. `RecentAutoModeDenialStore`: newest-first bounded store, max 20.
3. `RecentAutoModeRetryToken`: denial id, original runtime tool call, execution context ref, action digest, argument digest, snapshot digest, expires at. Consumed state는 process-local token store entry가 보유하며 persisted pending approval metadata에는 approval request id, digest tuple, expiry, requester digest만 저장한다.
4. `RecentAutoModeRetryOutcome`: approved and executed, missing token, expired, consumed, digest mismatch, snapshot mismatch, not retryable.

## 정상 시퀀스

1. Auto mode direct tool action이 local fast path와 static rules를 통과한다.
2. Provider-backed classifier가 high confidence `deny_candidate`와 requested/out-of-scope reason을 반환한다.
3. Runtime은 action을 실행하지 않고 denied outcome을 남긴다.
4. Runtime은 sanitized recent denial summary를 list 앞에 추가하고 process-local retry token을 만든다.
5. 사용자가 permission surface에서 recent denial을 확인한다.
6. 사용자가 exact retry approval을 선택한다.
7. Runtime은 formal approval decision correlation을 검증한다.
8. 검증이 통과하면 기존 approved execution path로 한 번 실행하고 token을 소비한다.

## 실패 시퀀스

1. Classifier parser가 실패한다.
2. Runtime은 fail-closed ask/deny로 접고 recent retryable denial로 기록하지 않는다.
3. Static rule이 action을 hard deny한다.
4. Runtime은 hard deny evidence만 남기고 retry token을 만들지 않는다.
5. 사용자가 retry하기 전에 process가 재시작된다.
6. Sanitized summary는 보일 수 있지만 executable retry token이 없으므로 retry는 fail-closed로 거절된다.
7. Retry approval 후 action digest나 snapshot digest가 다르다.
8. Runtime은 실행하지 않고 mismatch outcome을 남긴다.

## 검증 관점

1. Classifier-origin `deny_candidate`만 recent denial로 기록되는지 확인한다.
2. Recent denial list가 20개로 cap되는지 확인한다.
3. Static hard deny와 protected target deny가 retryable recent denial을 만들지 않는지 확인한다.
4. Recent denial summary가 raw session id, raw turn id, raw command, raw prompt, raw classifier response, env/config/secret/host path를 포함하지 않는지 확인한다.
5. Retry가 formal approval correlation을 통과해야만 실행되는지 확인한다.
6. Retry token이 one-shot이고 expiry/mismatch/consumed 상태에서 fail-closed인지 확인한다.
7. Non-interactive retry path가 prompt 없이 deny 또는 abort되는지 확인한다.

## 완료 기준

1. Recent classifier denial model과 bounded newest-first digest-only store가 구현된다. 현재 구현됨.
2. Classifier-origin denied action이 sanitized summary로 inspect 가능하다. 현재 구현됨.
3. Exact one-shot retry가 approval correlation을 통과할 때만 실행된다. 현재 구현됨.
4. Hard deny/protected target/static deny/classifier failure는 retryable recent denial이 아니다. 현재 구현됨.
5. Redaction tests가 raw executable payload와 sensitive values persistence를 막는다. 현재 구현됨.
