# PRD 002. automation job lifecycle and outcome routing

Status: Complete (Scoped)

## Goal

One-shot, recurring, read-only no-agent, skill-backed automation을 같은 lifecycle과 trusted-runtime evidence contract로 실행한다. Script-only와 app-task mode는 typed lifecycle에 참여하지만 production adapter가 없으므로 fail closed한다.

## Scope

1. Normalized trigger, job/run state, idempotency, recursion guard, timeout/cancellation.
2. Execution snapshot, trusted-runtime ref, hook status, process/sandbox/credential facts.
3. Job result와 delivery result의 분리.
4. Durable scheduler/recovery facts의 Spec 029 consumption.

## Non Scope

1. 별도 scheduler, owner lease, trace truth를 재구현하지 않는다.
2. 모든 job에 model invocation을 요구하지 않는다.
3. Headless confirmation-required step을 auto-allow하지 않는다.
4. Script-only와 app-task execution adapter 구현은 현재 scoped closure에 포함하지 않는다.

## Parent Requirement Mapping

- Owned scope 3-5
- Invariants 3-5, 9
- Primary Must Have: 3
- Primary Acceptance Criteria: 3

## Acceptance Criteria

1. One-shot, recurring, no-agent, skill-backed job이 같은 lifecycle states를 사용한다.
2. Headless confirmation denial, hook veto, timeout, sandbox fallback/failure, credential unavailable가 explicit result다.
3. Duplicate/superseded run과 recursion guard가 fail closed한다.
4. Job success and delivery success are independently recorded.
5. Script-only와 app-task mode는 unsupported adapter 결과를 남기고 side effect 전에 종료한다.

## Closure Evidence

1. Automation lifecycle scenario matrix.
2. Specs 029/030/031 owner-fact audits.
3. Heartbeat/cron scheduler-service와 owner-accepted subagent real-surface transcripts, unsupported app/channel/local-API result pre-enqueue rejection, cleanup receipts.
