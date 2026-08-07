# PRD 003. self-improvement CAS, apply, and verify

Status: Planned

## Goal

Self-improvement proposal을 immutable execution snapshot, apply-time compare-and-swap, live hook/confirmation, checkpoint, apply, verify, rollback candidate로 연결한다.

## Scope

1. Proposal target identity/digest와 execution snapshot ref.
2. Apply 직전 current target digest CAS.
3. Pre-tool hook, 필요한 ephemeral confirmation, headless denial.
4. Checkpoint, apply, independent verify, record, rollback candidate.

## Non Scope

1. Durable approval, remembered allow, replay authorization을 만들지 않는다.
2. Silent runtime code replacement나 universal auto rollback을 허용하지 않는다.
3. Timeout/kill을 side-effect rollback proof로 사용하지 않는다.

## Parent Requirement Mapping

- Owned scope 6
- Invariants 8-12
- Primary Must Have: 5
- Primary Acceptance Criteria: 5-6

## Acceptance Criteria

1. Changed target digest rejects stale proposal before side effects.
2. Hook veto and required confirmation denial prevent apply without durable authorization artifacts.
3. Verify failure records evidence and exposes only a rollback candidate.
4. Rollback execution re-enters current hook/confirmation/process/sandbox boundaries.

## Closure Evidence

1. CAS contention and stale-target tests.
2. Hook/headless confirmation/checkpoint/apply/verify receipts.
3. Rollback-candidate non-automatic execution audit.
