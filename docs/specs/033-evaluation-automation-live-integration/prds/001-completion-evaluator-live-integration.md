# PRD 001. completion evaluator live integration

Status: Complete (Scoped)

## Goal

Completion evaluator를 AgentLoop turn end, heartbeat/cron scheduled wake, owner-accepted terminal subagent result 경계에서 advisory input으로 소비하고 task outcome을 route한다. App/channel/local-API background result는 typed vocabulary만 유지하며 producer가 없는 동안 unavailable이다.

## Scope

1. Evaluator input/output record와 bounded verdict.
2. Notify, suppress, continue, escalate, verify, rollback-candidate routing.
3. AgentLoop turn end, heartbeat/cron service producer, owner-accepted terminal subagent result integration.
4. User interruption, bounded continuation, evidence preservation.

## Non Scope

1. Evaluator가 hook, confirmation, process, credential, sandbox, app state를 override하지 않는다.
2. Evaluator verdict가 tool/app/delivery/config action을 직접 실행하지 않는다.
3. Suppress는 evidence deletion이 아니다.
4. App-task, channel result, local API background result producer와 synthetic terminal success를 구현하지 않는다.

## Parent Requirement Mapping

- Owned scope 1, 3, 5
- Invariants 1-3, 7
- Primary Must Have: 2, 4
- Primary Acceptance Criteria: 2, 4

## Acceptance Criteria

1. Every supported boundary records evaluator input, output, route, owner result locator. Producer가 없는 typed boundary는 unavailable이며 owner locator를 합성하지 않는다.
2. Evaluator output cannot mutate owner state without routed command handling.
3. User interruption and continuation budget stop unbounded continuation.
4. Notify/delivery result remains separate from task success.

## Closure Evidence

1. Boundary integration matrix.
2. Advisory-authority regression tests.
3. AgentLoop, heartbeat/cron service, owner-accepted subagent outcome-routing artifacts와 app/channel/local-API pre-enqueue rejection evidence.
