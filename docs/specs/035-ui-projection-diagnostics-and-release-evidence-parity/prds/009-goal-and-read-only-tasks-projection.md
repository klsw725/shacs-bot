# PRD 009. goal and read-only Tasks projection

Status: Planned

## Goal

Spec 033 goal accounting과 기존 child/workflow/recovery owner record를 별도 truth store 없이 하나의 read-only Tasks view로 투영한다.

## Scope

1. Goal id, state, stop reason, continuation budget, usage summary의 cross-surface parity.
2. Child, workflow, automation, app task, recovery row의 owner-backed aggregation.
3. Stale, blocked, running, recovered, done 상태와 safe next action 표시.
4. CLI, local API, TUI에서 같은 canonical fields를 소비하는 adapter.

## Non Scope

1. Resident-agent identity, durable family messaging, 별도 task database를 도입하지 않는다.
2. Projection이 goal, child, workflow, app, recovery state를 변경하지 않는다.
3. Owner locator 없는 synthetic task row를 만들지 않는다.

## Required Contract

1. 모든 row는 owner kind, opaque owner locator, observed-at/freshness, bounded state를 가진다.
2. Mutation action은 기존 command owner로 route하고 projection은 requested/completed를 구분한다.
3. Missing owner evidence는 unavailable/unknown이며 empty success로 표시하지 않는다.
4. Goal accounting은 Spec 033의 domain fact를 그대로 보존하고 Prime goal store를 새 authority로 만들지 않는다.

## Acceptance Criteria

1. CLI, local API, TUI가 같은 goal id, stop reason, continuation budget을 표시한다.
2. Child/workflow/automation/app/recovery mixed fixture가 owner locator와 freshness를 보존한다.
3. Stale/blocked/recovered rows와 next action이 owner evidence 없이 생성되지 않는다.
4. Tasks view에서 요청한 action은 owner command 결과 전까지 completed로 표시되지 않는다.

## Closure Evidence

1. Goal accounting parity matrix.
2. Owner locator coverage audit.
3. Mixed-state Tasks view fixture와 terminal TUI/API/CLI transcripts.
4. 별도 truth store와 mutation authority가 없음을 확인하는 architecture read audit.
