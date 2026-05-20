# PRD 008. runtime evaluator enforcement and ledger consumption

## 목표

이 문서는 000-007에서 정의한 evaluator verdict와 ledger record가 실제 `MainOrchestrator` runtime decision으로 소비되는 기준을 정의한다. 목표는 evaluator가 만든 `done`, `continue`, `blocked`, approval, task outcome 제안을 권한 있는 runtime 정책 입력으로 바꾸되, stale verdict와 ledger 중복 소비가 goal continuation을 오염시키지 않게 하는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/001-persistent-goal-and-continuation-loop.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/003-task-outcome-and-scheduled-automation.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/007-projections-diagnostics-and-release-gates.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`

## Dependency Cut

- 000의 evaluator envelope, frozen snapshot digest, evaluation ledger, task ledger를 그대로 소비한다.
- 001의 persistent goal lifecycle과 continuation budget을 runtime decision의 입력으로 사용한다.
- 002의 approval과 checkpoint gate 결과를 verdict enforcement 전에 확인한다.
- 003의 task outcome verdict는 background result decision으로만 소비하며 scheduler primitive를 재정의하지 않는다.
- 007은 projection 의미를 제공한다. 이 PRD는 projection에 들어갈 decision status와 ledger consumption evidence를 만든다.
- Spec 007은 `MainOrchestrator`의 최종 policy authority를 소유한다. 018은 evaluator verdict를 policy input으로 연결하는 통합 규칙만 소유한다.
- Spec 010은 host safety와 permission guard primitive를 소유한다. 018은 그 결과를 확인하지 않고 capability widening을 적용하면 안 된다.

## 범위

- evaluator verdict를 runtime decision input으로 정규화하는 adapter
- goal continuation, done, blocked 반영 조건
- stale verdict discard와 superseded verdict 기록
- evaluation ledger와 task ledger의 idempotent consumption
- duplicate, late, expired evaluator result 처리
- user interruption, stop, pause, clear 우선순위 반영
- projection과 diagnostics가 읽을 decision evidence 생성

## 범위 제외

- evaluator prompt 상세 문구 변경
- session store 물리 파일 형식 재설계
- permission grant engine 구현
- queue, scheduler, durable timer 구현
- CLI/TUI visual design
- 관리자 콘솔, 조직 정책, fleet 운영 흐름

## 구현 요구사항

- runtime은 evaluator verdict를 직접 실행 명령으로 보지 않고 `EvaluatorDecisionInput`으로 정규화해야 한다.
- `EvaluatorDecisionInput`은 verdict id, evaluator kind, evaluator version, source ledger ref, frozen snapshot digest, goal id, turn id, expiration, suggested action, confidence, evidence refs를 포함해야 한다.
- runtime은 verdict의 frozen snapshot digest가 현재 decision target의 digest와 맞지 않으면 stale로 discard해야 한다.
- stale verdict는 삭제하지 않고 evaluation ledger에 `discarded_stale` consumption status와 discard reason을 기록해야 한다.
- expiration이 지난 verdict는 goal continuation, approval request, rollback request로 승격하면 안 된다.
- 같은 verdict id는 한 번만 terminal consumption으로 기록해야 한다.
- ledger consumer는 `pending`, `consumed`, `discarded_stale`, `discarded_expired`, `superseded`, `blocked_by_policy`, `failed_to_apply` 상태를 구분해야 한다.
- goal `continue` verdict는 active goal, turn budget, user interruption gate, permission gate, recursion guard, runtime cancellation state를 모두 통과해야 다음 turn request가 된다.
- goal `done` verdict는 active goal의 session truth 상태 전이로 반영되어야 하며, evaluator verdict만으로 goal record가 종료된 것으로 간주하면 안 된다.
- goal `blocked` verdict는 user visible blocked reason과 unblock hint를 projection에 남겨야 한다.
- user stop, pause, clear, 새 user input은 evaluator continuation보다 우선한다.
- capability 관련 verdict는 002 approval state와 010 permission guard 결과 없이는 runtime effect로 바뀌면 안 된다.
- task outcome verdict의 `rollback`과 `verify`는 owner primitive 가능 여부를 확인한 뒤 action request 또는 blocked status로 기록해야 한다.
- late evaluator result가 이미 더 최신 verdict로 superseded된 target에 도착하면 `superseded`로 기록하고 runtime effect를 만들면 안 된다.
- ledger consumption은 crash 후 재시작해도 같은 verdict를 중복 적용하지 않는 idempotent key를 가져야 한다.
- 모든 runtime decision은 evaluation ledger ref, task ledger ref, session event ref 중 하나 이상의 evidence link를 가져야 한다.

## 데이터/상태 모델

- `EvaluatorDecisionInput`: verdict id, evaluator kind, source refs, target refs, snapshot digest, expiration, suggested action, confidence, evidence refs, redaction status.
- `LedgerConsumptionRecord`: consumption id, ledger ref, consumer id, idempotency key, status, decision ref, reason, created at, completed at.
- `RuntimeDecisionRecord`: decision id, session id, goal id, turn id, decision kind, policy gate results, selected action, blocked reason, projection ref.
- `ContinuationDecision`: goal id, source verdict id, budget state, interruption state, recursion guard state, final action.
- `StaleVerdictRecord`: verdict id, expected digest, current digest, discard reason, superseding verdict ref.

## 정상 시퀀스

1. evaluator가 frozen snapshot digest와 함께 verdict envelope를 기록한다.
2. runtime ledger consumer가 미소비 verdict를 idempotency key로 읽는다.
3. runtime이 target goal, turn, task 상태와 snapshot digest를 비교한다.
4. runtime이 expiration, user interruption, budget, permission, recursion guard를 확인한다.
5. `continue` verdict가 통과하면 `MainOrchestrator` policy input으로 continuation request를 만든다.
6. runtime이 decision record와 ledger consumption record를 `consumed`로 기록한다.
7. projection은 goal status, next action, evidence refs, redacted reason을 같은 shared status로 노출한다.

## 실패 시퀀스

1. evaluator result가 늦게 도착했지만 target goal snapshot digest가 이미 바뀌었다.
2. runtime이 verdict를 stale로 판단하고 runtime effect를 만들지 않는다.
3. ledger consumption record는 `discarded_stale`과 expected digest, current digest를 기록한다.
4. projection은 필요할 때만 stale evaluator result가 무시되었다고 inspect 가능한 상태로 노출한다.
5. diagnostics evidence에는 stale 판단 근거와 superseding decision ref가 포함된다.

## 검증 관점

- 같은 verdict id를 두 번 소비해도 goal continuation이 한 번만 생성되는지 확인한다.
- snapshot digest가 다른 verdict가 도착하면 stale로 discard되고 runtime effect가 없는지 확인한다.
- expired verdict가 approval, rollback, continuation request로 승격되지 않는지 확인한다.
- user pause 또는 clear 이후 도착한 `continue` verdict가 goal을 재시작하지 않는지 확인한다.
- capability verdict가 approval과 permission guard 없이 tool exposure를 넓히지 않는지 확인한다.
- diagnostics bundle에서 decision record, consumption record, source verdict ref가 함께 추적되는지 확인한다.

## 완료 기준

- runtime이 evaluator verdict를 직접 실행하지 않고 policy input과 decision record로만 소비한다.
- stale, expired, duplicate, superseded verdict가 각각 별도 consumption status로 기록된다.
- goal continuation은 user interruption, budget, permission, recursion guard를 통과할 때만 발생한다.
- evaluation ledger와 task ledger consumption이 crash replay 후에도 idempotent하게 복구된다.
- shared projection과 diagnostics가 runtime decision evidence를 읽을 수 있다.
