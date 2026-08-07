# PRD 000. config, profile, and auth-source migration

Status: Planned

## Goal

기존 JSON compatibility를 보존하면서 schema-versioned config, provider/trusted-runtime/context profile, auth-source declaration migration을 구현한다.

## Scope

1. Schema version validator and migration entrypoint.
2. Dry-run, apply, interrupted marker, recover, rollback evidence.
3. Provider/trusted-runtime/context profiles.
4. Environment, local-auth-entry, literal, command-backed source locator/declaration.

## Non Scope

1. Evidence 없이 TOML을 필수화하지 않는다.
2. Raw auth store lifecycle, credential precedence/refresh를 재소유하지 않는다.
3. Migration이 env placeholder를 resolved secret으로 writeback하지 않는다.

## Parent Requirement Mapping

- Owned scope 1-4
- Invariants 1-4
- Primary Must Have: 1-5
- Primary Acceptance Criteria: 1-3, 5-6

## Acceptance Criteria

1. Current/legacy/future-unsupported schema states are deterministic.
2. Existing JSON reads before/after migration and no-op migration does not write back.
3. Placeholder, auth locator, workspace override, profile selection survive migration.
4. Interrupted apply blocks mutation and produces recover evidence.

## Closure Evidence

1. Migration compatibility matrix.
2. Dry-run/apply/interruption/recover transcripts.
3. Spec 030 auth-source owner-fact audit.
4. JSON/TOML decision record.
