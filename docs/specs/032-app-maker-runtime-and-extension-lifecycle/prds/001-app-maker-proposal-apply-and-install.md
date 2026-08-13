# PRD 001. App Maker proposal, apply, and install

Status: Complete (Scoped)

## Goal

App Maker draft를 current-user authoring/apply decision, checkpoint, apply, verify, install/update로 연결하되 runtime authorization을 만들지 않는다.

## Scope

1. Draft, candidate, validation report, risk summary, proposal receipt.
2. Current-user authoring/apply decision과 checkpoint linkage.
3. New app install과 existing app snapshot/diff/update handoff.
4. Apply failure, verify failure, interrupted update recovery evidence.

## Non Scope

1. Validation 중 package manager, shell, MCP, network auth test를 실행하지 않는다.
2. Install은 process start, hook dispatch, tool exposure, executable activation을 만들지 않는다.
3. Authoring decision을 tool/credential/activation/replay authorization으로 재사용하지 않는다.

## Parent Requirement Mapping

- Owned scope 4-5
- Invariants 1-2, 8-9
- Primary Must Have: 6-7
- Primary Acceptance Criteria: 6-7

## Required Contract

1. Proposal receipt는 proposal id, user intent, generated candidate, validation/risk summary, revision digest를 가진다.
2. Apply decision은 대상 proposal revision과 installed snapshot digest에 묶인다.
3. Existing app은 installed bundle을 직접 덮어쓰지 않고 snapshot, diff, checkpoint, apply, verify를 통과한다.
4. Install/update handoff는 registry mutation 결과를 남기지만 app process나 executable resource를 활성화하지 않는다.

## Acceptance Criteria

1. New app flow가 draft부터 install handoff까지 traceable하다.
2. Stale proposal 또는 changed installed snapshot은 apply 전에 거부된다.
3. Verify failure와 interrupted apply가 checkpoint/recovery evidence를 남긴다.
4. Receipt audit가 authoring decision이 runtime authorization으로 사용되지 않음을 보인다.

## Closure Evidence

1. Proposal/apply/install scenario matrix.
2. Existing-app diff와 stale revision test.
3. Installed registry before/after artifact와 recovery receipt.
4. Authoring decision non-authorization audit.
