# PRD 004. snapshot replay, review, and release evidence

Status: Planned

## Goal

035 execution snapshot과 recorded trajectory로 destructive live dispatch 없는 replay를 수행하고 reproducible review/release artifacts를 만든다.

## Scope

1. Trajectory selection, snapshot/digest validation, expected verdict/outcome comparison.
2. Goal, automation, evaluator, hook/confirmation, checkpoint, verify, delivery receipt linkage.
3. QA, goal, code, security, docs review artifact.
4. Edge regression과 033 release coverage entry.

## Non Scope

1. Replay 중 hook, confirmation, credential refresh, process, sandbox, delivery, config/apply를 호출하지 않는다.
2. Snapshot을 current authorization이나 live source truth로 사용하지 않는다.
3. Original runtime trace 전체의 complete redaction을 주장하지 않는다.

## Parent Requirement Mapping

- Owned scope 7-10
- Invariants 6-8, 12
- Primary Must Have: 7-11
- Primary Acceptance Criteria: 7-8

## Acceptance Criteria

1. Missing/mismatched snapshot or source mutation fails closed.
2. Replay invokes no destructive live boundary and compares recorded outcomes only.
3. Artifact transform excludes raw credential/hidden reasoning/unnecessary payload and records disclosure status.
4. Edge suite covers hook/headless/process/sandbox/credential/snapshot/redaction/duplicate/superseded/replay failures.

## Closure Evidence

1. Replay no-live-dispatch transcript.
2. Snapshot mismatch/source-mutation fixtures.
3. Reproducible review artifacts and coverage entry.
4. Projection-boundary transform audit.
