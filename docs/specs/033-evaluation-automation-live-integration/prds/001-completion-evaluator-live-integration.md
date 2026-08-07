# PRD 001. completion evaluator live integration

Status: Planned

## Goal

Completion evaluator를 turn end, scheduled wake, subagent/app/channel/background result 경계에서 advisory input으로 소비하고 task outcome을 route한다.

## Scope

1. Evaluator input/output record와 bounded verdict.
2. Notify, suppress, continue, escalate, verify, rollback-candidate routing.
3. AgentLoop, service, channel, app task, local API background-result integration.
4. User interruption, bounded continuation, evidence preservation.

## Non Scope

1. Evaluator가 hook, confirmation, process, credential, sandbox, app state를 override하지 않는다.
2. Evaluator verdict가 tool/app/delivery/config action을 직접 실행하지 않는다.
3. Suppress는 evidence deletion이 아니다.

## Parent Requirement Mapping

- Owned scope 1, 3, 5
- Invariants 1-3, 7
- Primary Must Have: 2, 4
- Primary Acceptance Criteria: 2, 4

## Acceptance Criteria

1. Every supported boundary records evaluator input, output, route, owner result locator.
2. Evaluator output cannot mutate owner state without routed command handling.
3. User interruption and continuation budget stop unbounded continuation.
4. Notify/delivery result remains separate from task success.

## Closure Evidence

1. Boundary integration matrix.
2. Advisory-authority regression tests.
3. Service/channel/app/local-API outcome-routing artifacts.
