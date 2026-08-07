# PRD 004. activation persistence, replay, and diagnostics

Status: Planned

## Goal

Executable-resource activation record를 schema-versioned로 persist/migrate하고 snapshot reference와 diagnostic-only replay를 제공한다.

## Scope

1. Activation source/workspace trust/resource/content/dependency identities.
2. Active, stale, disabled, revoked, removed states and migration.
3. Inspect/disable/revoke mutation admission and historical provenance.
4. Snapshot refs, diagnostics, replay interpretation without live source dispatch.

## Non Scope

1. Activation eligibility와 dependency execution gate는 030을 재소유하지 않는다.
2. App-level lifecycle transition은 032를 재소유하지 않는다.
3. Activation digest/status를 permission grant나 verified-entrypoint authorization으로 사용하지 않는다.

## Parent Requirement Mapping

- Owned scope 10-11
- Invariants 11
- Primary Must Have: 12-13
- Primary Acceptance Criteria: 12, 14
- 032 executable-resource handoff

## Acceptance Criteria

1. Migration preserves activation identity/status/reason and detects digest mismatch.
2. Snapshot consumes exact activation ref as diagnostic provenance only.
3. Replay performs no current resource discovery, dependency preparation, credential resolution, or entrypoint execution.
4. New execution rechecks current 030 eligibility and 032 lifecycle facts.

## Closure Evidence

1. Activation schema/migration/state tests.
2. Inspect/disable/revoke and historical receipt artifacts.
3. Replay no-live-dispatch transcript.
4. Specs 030/032 owner-fact audits.
