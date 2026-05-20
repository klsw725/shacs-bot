# PRD 009. scheduled automation runtime execution

## 목표

이 문서는 003에서 정의한 scheduled automation 의미를 실제 runtime service 실행 경로에 연결하는 기준이다. heartbeat, cron, subagent, app task, channel worker, local API background request가 같은 task outcome language와 guard를 쓰면서도 012 runtime service primitive를 재정의하지 않게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/001-persistent-goal-and-continuation-loop.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/003-task-outcome-and-scheduled-automation.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/008-runtime-evaluator-enforcement-and-ledger-consumption.md`
- 교차 의존:
  - `docs/specs/011-subagent-runtime/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`

## Dependency Cut

- 003은 automation job과 task outcome verdict의 의미를 제공한다.
- 008은 evaluator verdict와 ledger consumption enforcement를 제공한다.
- 012는 heartbeat, cron, durable runtime service, local API reentry, delivery primitive를 소유한다.
- 011은 subagent spawn, cancellation, result merge primitive를 소유한다.
- 017은 app manifest와 app task boundary를 소유한다.
- 013은 CLI, TUI, local API, channel rendering을 소유한다. 018은 그 surface가 소비할 status와 delivery 의미만 제공한다.

## 범위

- heartbeat와 cron wake를 automation run request로 정규화
- subagent result, app task result, channel event, local API background request 수집
- timeout, retry, delivery, escalation, cancellation 연결
- recursion guard와 self triggered automation loop 차단
- task outcome evaluator 호출과 ledger consumption 연결
- runtime service primitive를 통한 durable execution coordination

## 범위 제외

- cron expression parser 구현
- durable queue 또는 scheduler 저장소 설계
- channel vendor별 adapter 구현
- app process sandbox 구현
- subagent runtime 내부 프로토콜 구현
- 관리자용 job dashboard, 조직 알림 정책, fleet scheduling

## 구현 요구사항

- automation run은 012 runtime service primitive가 제공한 wake 또는 reentry event에서만 시작해야 한다.
- 018은 durable queue, timer, worker lease를 직접 정의하지 않고 `AutomationRunRequest`와 outcome contract만 정의한다.
- `AutomationRunRequest`는 job id, trigger kind, trigger ref, session id, goal id, execution mode, timeout policy ref, retry policy ref, delivery policy ref, recursion guard token을 포함해야 한다.
- trigger kind는 `heartbeat`, `cron`, `subagent_result`, `app_task_result`, `channel_event`, `local_api_background`, `manual_resume`을 구분해야 한다.
- heartbeat run은 active goal 또는 pending automation이 없으면 task outcome evaluator를 호출하지 않아야 한다.
- cron run은 사용자가 설정하거나 승인한 automation rule ref가 있어야 한다.
- subagent result run은 011의 result merge state가 terminal이거나 reviewable일 때만 outcome evaluator로 전달한다.
- app task result run은 017 app task boundary의 task id, manifest ref, capability scope를 evidence로 포함해야 한다.
- channel event run은 user visible delivery가 필요한 event만 projection으로 승격해야 한다.
- local API background request는 caller auth와 redaction profile ref를 포함해야 하며, raw payload를 ledger에 저장하면 안 된다.
- timeout은 012 runtime service의 terminal event 또는 retryable event로 받아 task ledger에 기록해야 한다.
- retry는 retry policy ref, attempt number, last failure reason, next eligible wake ref를 기록해야 한다.
- delivery는 013 surface가 읽을 redacted message, severity, target surface, suppress reason을 포함해야 한다.
- recursion guard는 automation이 만든 continuation 또는 delivery가 다시 같은 automation source를 즉시 호출하지 못하게 해야 한다.
- 같은 trigger ref와 job id는 idempotent run key를 가져야 하며 중복 wake가 중복 실행을 만들면 안 된다.
- task outcome evaluator가 `continue`를 제안하면 008 runtime enforcement와 001 goal budget을 통과해야 한다.
- task outcome evaluator가 `escalate`를 제안하면 user visible blocked 또는 approval required projection으로만 승격해야 한다.

## 데이터/상태 모델

- `AutomationRunRequest`: run id, job id, trigger kind, trigger ref, session id, goal id, execution mode, policy refs, recursion guard token.
- `AutomationRunState`: requested, leased, running, timed_out, retry_waiting, completed, failed, cancelled, suppressed.
- `AutomationTriggerRef`: runtime service event id, source type, source owner, received at, idempotency key.
- `AutomationDeliveryRecord`: delivery id, run id, target surface, severity, redacted message, suppress reason, acknowledged at.
- `AutomationRecursionGuard`: token, source run id, depth, max depth, parent refs, blocked reason.

## 정상 시퀀스

1. 012 runtime service가 cron wake event를 만든다.
2. 018 automation coordinator가 wake를 `AutomationRunRequest`로 정규화한다.
3. coordinator가 idempotency key, approval state, recursion guard, timeout policy를 확인한다.
4. owner runtime이 subagent, app task, script, no agent check 중 지정된 execution mode를 실행한다.
5. result가 redacted outcome ref로 task ledger에 기록된다.
6. task outcome evaluator가 verdict를 만든다.
7. 008 runtime enforcement가 verdict를 소비하고 continuation, delivery, verification, blocked status 중 하나로 반영한다.

## 실패 시퀀스

1. channel event로 시작한 automation이 다시 같은 channel delivery를 즉시 유발한다.
2. recursion guard가 parent run id와 depth를 확인한다.
3. guard가 max depth 또는 same source loop를 감지해 run을 `suppressed`로 기록한다.
4. task outcome evaluator는 호출하지 않거나 suppressed outcome만 평가한다.
5. projection은 자동화가 loop guard로 중단되었다는 redacted status를 제공한다.

## 검증 관점

- heartbeat에 처리할 goal이나 job이 없으면 evaluator 호출과 ledger record가 생기지 않는지 확인한다.
- 같은 cron wake가 두 번 전달되어도 automation run이 한 번만 실행되는지 확인한다.
- timeout result가 retryable과 terminal로 구분되어 ledger와 projection에 반영되는지 확인한다.
- app task result가 app 권한만으로 approval이나 self improvement apply를 실행하지 않는지 확인한다.
- recursion guard가 self triggered loop를 차단하고 suppress reason을 남기는지 확인한다.
- local API background request가 raw payload 없이 redacted evidence ref만 저장하는지 확인한다.

## 완료 기준

- 모든 background trigger가 `AutomationRunRequest`와 task outcome evaluator 경로로 정규화된다.
- durable execution은 012 primitive를 통해서만 시작되고 018이 queue나 scheduler를 재정의하지 않는다.
- timeout, retry, delivery, cancellation, recursion guard 상태가 task ledger와 projection에서 추적된다.
- duplicate wake와 duplicate result가 idempotent하게 처리된다.
- self hosted 사용자가 로컬 surface에서 automation run의 현재 상태와 막힌 이유를 확인할 수 있다.
