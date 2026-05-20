# PRD 002. capability approval and checkpoint gates

## 목표

이 문서는 safety/capability evaluator, permission mode 소비, approval correlation, checkpoint trigger decision을 완전 구현하기 위한 실행 기준이다. evaluator는 action이 요구하는 capability와 risk를 설명하지만, 허용 여부와 tool 실행은 `MainOrchestrator`와 owner spec의 gate가 확정한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD: `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
- 교차 의존:
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`

## Dependency Cut

- 000의 evaluator envelope와 ledger split을 사용한다.
- 007은 orchestrator policy와 stale discard를 소유한다.
- 010은 permission mode, secret boundary, host safety guard, approval primitive를 소유한다.
- 014는 approval, denial, checkpoint decision의 diagnostics projection을 소유한다.
- 018은 capability evaluator verdict와 checkpoint trigger decision의 통합 계약만 소유한다.

## 범위

- action별 capability evaluation request와 verdict
- permission mode 소비 규칙
- approval request와 decision correlation
- stale 또는 expired approval rejection
- checkpoint trigger decision
- denied outcome standard
- destructive, permissioned, self improvement action의 공통 gate 순서

## 범위 제외

- permission UI 화면 디자인
- secret vault 구현
- filesystem, process, network guard 내부 구현
- checkpoint backend 물리 저장 방식
- 조직 승인 라인과 관리자 콘솔
- fleet policy rollout

## 구현 요구사항

- capability evaluator input은 action kind, target ref, requested capability, permission mode snapshot, approval context, checkpoint policy hint를 포함해야 한다.
- evaluator verdict는 `allow_candidate`, `deny_candidate`, `needs_approval`, `needs_checkpoint`, `needs_secret`, `insufficient_context` 중 하나의 decision hint를 반환해야 한다.
- decision hint는 최종 허용이 아니다. orchestrator는 010의 permission mode와 approval state를 다시 확인해야 한다.
- approval request는 request id, action digest, snapshot digest, expires at, displayed risk summary를 가져야 한다.
- approval decision은 approval request id와 action digest가 일치할 때만 소비된다.
- stale snapshot, expired approval, mismatched action digest는 항상 denied outcome으로 접는다.
- checkpoint trigger decision은 destructive write, restore, rollback, self improvement apply, app task mutation 전에 평가되어야 한다.
- checkpoint가 필요한데 생성되지 않았거나 inspect 불가능하면 action은 실행되지 않아야 한다.
- denied outcome은 user visible reason, redacted evidence ref, retry 가능 여부, required next step을 포함해야 한다.
- 모든 gate 결과는 evaluation ledger와 diagnostics에서 같은 correlation id로 확인 가능해야 한다.

## 데이터/상태 모델

- `CapabilityEvaluationInput`: action id, action kind, target digest, capability set, permission mode, approval context, checkpoint hint.
- `CapabilityVerdict`: hint, reason, risk level, evidence refs, expiration, checkpoint recommendation.
- `ApprovalRequestRef`: request id, action digest, snapshot digest, created at, expires at, status.
- `ApprovalDecisionRef`: decision id, request id, decision, decided at, actor local user.
- `CheckpointGateDecision`: required, optional, skipped, blocked, reason, checkpoint ref.
- `DeniedOutcome`: code, message, evidence ref, retry class, user next step.

## 정상 시퀀스

1. runtime이 permissioned action 실행 직전 capability evaluation을 요청한다.
2. frozen snapshot과 permission mode가 envelope에 포함된다.
3. evaluator가 `needs_approval`과 `needs_checkpoint`를 제안한다.
4. orchestrator가 approval request를 만들고 사용자에게 risk summary를 보여준다.
5. 사용자가 승인하면 request id, action digest, expiration을 검증한다.
6. checkpoint가 생성되고 inspect 가능한 ref가 기록된다.
7. orchestrator가 owner tool 또는 runtime에 action 실행을 위임한다.

## 실패 시퀀스

1. approval decision의 action digest가 다르면 action을 denied outcome으로 닫는다.
2. approval이 만료된 뒤 도착하면 expired rejection을 기록한다.
3. permission mode가 action을 허용하지 않으면 evaluator hint와 관계없이 denied outcome을 반환한다.
4. checkpoint 생성 실패 또는 redaction 실패가 있으면 destructive action을 실행하지 않는다.
5. evaluator가 권한 확정처럼 응답하면 boundary violation으로 기록하고 deny한다.

## 검증 관점

- approval request와 decision correlation이 action digest mismatch를 거부하는지 확인한다.
- stale 또는 expired approval이 실행으로 이어지지 않는지 확인한다.
- checkpoint required action이 checkpoint 없이 실행되지 않는지 확인한다.
- denied outcome이 task, UI, diagnostics에서 같은 code로 보이는지 확인한다.
- permission mode가 evaluator hint보다 우선하는지 확인한다.

## 완료 기준

- capability evaluator와 permission gate의 책임 경계가 구현과 fixture로 확인된다.
- approval correlation, expiration, stale rejection이 반복 가능한 테스트로 닫힌다.
- checkpoint trigger decision이 destructive action과 self improvement flow의 공통 gate로 쓰인다.
- denied outcome standard가 후속 automation, app, MCP, rollback 흐름에서 재사용 가능하다.
