# PRD 000. goal accounting and projection input

Status: Planned

## Goal

Goal lifecycle과 continuation accounting을 typed owner facts로 만들고 Spec 035가 truth를 재구성하지 않고 투영할 수 있게 한다.

## Scope

1. Goal set/status/pause/resume/clear/done/blocked.
2. Goal id, stop reason, continuation budget, usage summary, user interruption.
3. CLI/local API command와 projection input contract.
4. Stale/unknown/missing goal evidence와 deterministic transitions.

## Non Scope

1. Prime goal store나 별도 session truth를 도입하지 않는다.
2. Projection adapter와 Tasks view는 035가 소유한다.
3. Goal state가 tool execution을 직접 허용하지 않는다.

## Parent Requirement Mapping

- Owned scope 1-2
- Invariants 1-3
- Primary Must Have: 1
- Primary Acceptance Criteria: 1

## Acceptance Criteria

1. Every goal transition records goal id, prior/current state, stop reason, budget, observed-at.
2. User interruption wins over automated continuation.
3. CLI/local API return the same canonical goal facts.
4. Missing evidence remains unknown/unavailable and never becomes done.

## Closure Evidence

1. Goal transition and accounting tests.
2. CLI/local API parity transcript.
3. 035 projection fixture and no-separate-truth audit.
