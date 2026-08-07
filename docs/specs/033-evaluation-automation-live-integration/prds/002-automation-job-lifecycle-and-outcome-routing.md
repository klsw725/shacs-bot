# PRD 002. automation job lifecycle and outcome routing

Status: Planned

## Goal

One-shot, recurring, no-agent, skill-backed, app-task automation을 같은 lifecycle과 trusted-runtime evidence contract로 실행한다.

## Scope

1. Normalized trigger, job/run state, idempotency, recursion guard, timeout/cancellation.
2. Execution snapshot, trusted-runtime ref, hook status, process/sandbox/credential facts.
3. Job result와 delivery result의 분리.
4. Durable scheduler/recovery facts의 Spec 029 consumption.

## Non Scope

1. 별도 scheduler, owner lease, trace truth를 재구현하지 않는다.
2. 모든 job에 model invocation을 요구하지 않는다.
3. Headless confirmation-required step을 auto-allow하지 않는다.

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

## Closure Evidence

1. Automation lifecycle scenario matrix.
2. Specs 029/030/031 owner-fact audits.
3. Scheduler/service/channel real-surface transcripts and cleanup receipts.
