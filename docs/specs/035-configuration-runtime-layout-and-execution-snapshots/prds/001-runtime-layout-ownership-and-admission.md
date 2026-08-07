# PRD 001. runtime layout ownership and admission

Status: Planned

## Goal

Config, auth, sessions, media, logs, channels, skills, cache, tmp, snapshots의 directory ownership과 mutation/cleanup admission을 공식화한다.

## Scope

1. Directory owner, marker, creation, mutation, cleanup rules.
2. Conflicting owner and stale marker handling.
3. Config/profile migration marker와 runtime-start admission linkage.
4. Existing path helper와 user documentation parity.

## Non Scope

1. Runtime directory ownership을 security sandbox나 OS isolation으로 표현하지 않는다.
2. Cluster/shared multi-node root를 기본으로 하지 않는다.
3. 029 durable runtime migration/lease truth를 재소유하지 않는다.

## Parent Requirement Mapping

- Owned scope 5
- Invariants 5
- Primary Must Have: 6
- Primary Acceptance Criteria: 4, 7

## Acceptance Criteria

1. Official helper and documentation enumerate the same directories and owners.
2. Conflicting owner, interrupted migration, stale marker block unsafe mutation.
3. Cleanup preserves active owner data and records every removed marker/path.
4. 029 writable-start fact is consumed without requiring Spec029 closure status.

## Closure Evidence

1. Layout/owner/marker matrix.
2. Admission and cleanup failure-injection tests.
3. 015/029 handoff audit and user-facing layout docs.
