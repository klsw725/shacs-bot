# PRD 003. task outcome and scheduled automation

## 목표

이 문서는 task outcome evaluator와 scheduled automation의 완전 구현 기준을 정의한다. heartbeat, cron, subagent, app task, channel worker, local API background result가 같은 outcome language를 쓰고, 사용자가 볼 수 있고 멈출 수 있고 되돌릴 수 있는 로컬 자동화로 동작하게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
- 교차 의존:
  - `docs/specs/011-subagent-runtime/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`

## Dependency Cut

- 000의 task ledger와 evaluation ledger split을 사용한다.
- 002의 capability, approval, checkpoint gate를 background action에도 적용한다.
- 012는 heartbeat, cron, channel worker, local API reentry primitive를 소유한다.
- 011은 subagent spawn과 merge primitive를 소유한다.
- 017은 app task와 app process boundary를 소유한다.
- 018은 다양한 background result를 같은 task outcome evaluator 계약으로 묶는다.

## 범위

- task outcome evaluator의 `notify`, `suppress`, `continue`, `escalate`, `verify`, `rollback` verdict
- heartbeat, cron, subagent, app, channel, local API background result 수집
- one shot, recurring, skill backed, script only, no agent, app job 모델
- timeout, delivery, retry, escalation, rollback request
- recursion prevention과 automation loop guard
- task ledger와 projection 요구사항

## 범위 제외

- cron expression parser 선택
- 특정 channel vendor 구현
- distributed queue 또는 multi node scheduling
- 조직 inbox와 관리자 dashboard
- 원격 job runner SaaS
- agent 없는 script sandbox 내부 구현

## 구현 요구사항

- automation job은 one shot 또는 recurring이어야 하며, source가 user, approved runtime, app, skill, local API 중 무엇인지 기록해야 한다.
- job은 skill backed agent 실행, script only 실행, no agent check, app task 중 하나 이상의 execution mode를 명시해야 한다.
- 모든 background result는 task outcome evaluator 입력 전에 redacted result ref로 정규화되어야 한다.
- outcome evaluator는 `notify`, `suppress`, `continue`, `escalate`, `verify`, `rollback` 중 하나를 반환해야 한다.
- `continue`는 persistent goal과 turn budget, recursion guard, permission gate를 통과해야 한다.
- `rollback`은 002의 checkpoint gate와 owner rollback primitive가 가능할 때만 action request로 바뀐다.
- `verify`는 결과가 성공처럼 보여도 별도 검증 surface가 필요함을 뜻하며, 자동 성공으로 기록하면 안 된다.
- timeout은 task ledger에 terminal 또는 retryable state로 기록하고, delivery 정책을 별도로 판단해야 한다.
- delivery는 CLI/TUI/local API/channel projection이 읽을 수 있는 redacted message와 severity를 가져야 한다.
- recursion prevention은 evaluator가 만든 continuation이 다시 같은 evaluator를 무한 호출하지 못하게 source chain과 depth를 추적해야 한다.

## 데이터/상태 모델

- `AutomationJob`: job id, source, schedule kind, execution mode, capability requirements, owner ref, status.
- `AutomationRun`: run id, job id, trigger, started at, timeout at, result ref, outcome verdict id.
- `TaskOutcomeVerdict`: class, reason, severity, delivery hint, next action hint, rollback hint.
- `BackgroundResultRef`: source kind, source id, redacted payload digest, exit status, timing, error class.
- `RecursionGuard`: root trigger id, depth, source chain, max depth, blocked reason.
- `DeliveryRecord`: destination, rendered summary ref, delivered, suppressed, failed, retry hint.

## 정상 시퀀스

1. recurring job이 scheduled wake를 받는다.
2. runtime이 capability gate와 recursion guard를 확인한다.
3. job execution mode에 맞춰 script, skill backed agent, subagent, app task 중 하나를 실행한다.
4. background result가 redacted ref로 정규화된다.
5. task outcome evaluator가 `notify` 또는 `continue` 같은 verdict를 반환한다.
6. orchestrator가 verdict를 소비해 delivery, continuation, verify, rollback 중 하나를 실행한다.
7. task ledger가 run, result, outcome, delivery를 연결해 기록한다.

## 실패 시퀀스

1. timeout이 발생하면 run을 timeout state로 닫고 outcome evaluator에 timeout result를 전달한다.
2. channel delivery가 실패하면 task 성공과 delivery 실패를 분리해 기록한다.
3. recursion depth가 초과되면 `escalate` 또는 `suppress`로 접고 continuation을 만들지 않는다.
4. script only job이 permission을 초과하면 denied outcome을 남기고 실행하지 않는다.
5. rollback verdict가 왔지만 checkpoint가 없으면 rollback 실행 대신 escalate한다.

## 검증 관점

- 각 background source가 같은 `BackgroundResultRef`로 정규화되는지 확인한다.
- outcome class별 후속 action이 정확히 분리되는지 확인한다.
- one shot job이 중복 실행되지 않고 recurring job이 다음 schedule을 유지하는지 확인한다.
- timeout과 delivery failure가 task result를 오염시키지 않는지 확인한다.
- recursion guard가 self triggering loop를 막는지 확인한다.

## 완료 기준

- heartbeat, cron, subagent, app, channel, local API 결과가 같은 outcome evaluator 계약을 사용한다.
- automation job 종류별 lifecycle과 ledger record가 정의되고 검증된다.
- notify, suppress, continue, escalate, verify, rollback의 의미가 구현과 projection에서 일관된다.
- timeout, delivery 실패, recursion prevention이 release gate에서 확인 가능하다.
