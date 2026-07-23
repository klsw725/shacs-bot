# PRD 002: durable work queue scheduler retry and cancellation

## 목표

Process-local bus, follow-up queue, active task registry를 보강할 durable work record를 만들고 restart 뒤 pending work, retry wake, cancellation request를 복원한다.

Status: Complete (Scoped). Typed bounded payload, durable lifecycle/retry/cancellation evidence, restart replay/admission, external dispatcher integration, bounded retention/resource limits, shutdown recovery와 priority `/stop` race regressions가 workspace gate와 독립 review를 통과했다. Queue는 계속 policy/session/channel delivery/child result truth owner가 아니며 exactly-once 실행을 보장하지 않는다.

## 범위

1. Durable work item과 payload reference
2. Pending, leased, waiting retry, cancelled, terminal 상태
3. Scheduler wake time와 retry attempt/backoff state
4. Durable stop/restart/task cancellation record
5. Restart restore와 process-local dispatcher adapter
6. Dedupe hint와 conservative redelivery semantics

## 비범위

- distributed/multi-node queue
- policy decision과 retry 승인
- channel별 delivery truth
- exactly-once work execution

## SPEC 입력

1. 필수 선행 PRD: `001-checkpoint-tail-replay-and-corruption-admission.md`
2. Execution identity/outcome: `../../028-formal-execution-reentry-and-outcome-contracts/SPEC.md`
3. Current service baseline: `../../012-runtime-services/SPEC.md`

## Dependency Cut

1. Queue는 무엇을 언제 깨울지 보존하지만 실행 채택 권한은 갖지 않는다.
2. Retry record는 attempt/next wake를 보존하고 policy verdict를 만들지 않는다.
3. Cancellation request와 cancellation outcome을 구분한다.
4. Payload는 inline raw command가 아니라 typed bounded payload 또는 artifact reference를 사용한다.

## 구현 요구사항

1. Work record는 work id, kind, session key, optional turn/effect correlation, payload ref, attempt, next wake, state, timestamps를 가진다.
2. Enqueue와 state transition은 PRD 000 event sequence와 연결된다.
3. Restart 뒤 leased/running item은 success로 추정하지 않고 recovery decision 대상이 된다.
4. Cancellation은 process memory 없이 inspect 가능해야 한다.
5. Scheduler는 wall clock 변화와 expired wake를 보수적으로 처리한다.
6. Dedupe hint는 duplicate suppression 근거이지 session truth가 아니다.
7. Queue compaction/retention은 terminal evidence를 무제한 보존하지 않도록 bounded rule을 가진다.

## 정상 시퀀스

1. Orchestrator가 accepted work를 enqueue한다.
2. Scheduler가 due item을 dispatcher에 lease한다.
3. Dispatcher가 work를 process-local 실행 경계로 전달하고, AgentLoop/orchestrator가 current policy를 다시 확인해 실행 채택을 결정한다.
4. Outcome이 event store에 기록되고 work state가 terminal 또는 retry waiting으로 바뀐다.
5. Restart 시 replay가 pending/waiting/cancel state를 복원한다.

## 실패 시퀀스

1. Lease 중 crash한 work는 completed로 처리하지 않는다.
2. Cancellation과 completion이 경쟁하면 event sequence와 outcome contract로 결정한다.
3. Payload reference가 없거나 손상되면 work를 blocked/inspect-only로 남긴다.
4. Attempt limit 초과는 silent drop이 아니라 terminal evidence가 된다.

## 검증 관점

1. Enqueue 후 restart, waiting retry 후 restart, cancellation 후 restart를 검증한다.
2. Lease 중 crash와 stale lease를 검증한다.
3. Duplicate wake/dedupe hint가 double session mutation을 만들지 않는지 확인한다.
4. Stop/restart marker가 durable cancellation과 구분되는지 확인한다.
5. Exactly-once wording이 public projection에 없는지 확인한다.

## 완료 기준

- Pending work, retry wake, cancellation request가 restart 뒤 복원된다.
- Queue와 scheduler가 policy/session truth owner가 아니다.
- Crash와 duplicate delivery matrix가 테스트로 고정된다.
