# PRD 005. sequential integration and Spec 035 closure

Status: Planned

## Goal

PRD 000-004를 migration/layout/snapshot/context/activation flow로 통합하고 Spec 035 parent requirement와 closure evidence를 완전히 검증한다.

## Scope

1. Dependency DAG and one-to-one requirement mapping.
2. Config migration, runtime admission, provider snapshot, context budget, activation persistence integration.
3. Compatibility, replay, diagnostics, owner-fact and documentation audits.
4. Final coverage, cleanup, closure verdict.

## Non Scope

1. 새 auth, activation, evaluator, media domain truth를 정의하지 않는다.
2. Specs 029-034의 `Complete` 상태를 요구하지 않는다.
3. Missing external facts를 safe/default values로 조작하지 않는다.

## Dependency DAG

```text
PRD000_config_migration
  -> PRD001_runtime_layout
PRD000_config_migration
  -> PRD002_execution_snapshot
PRD000_config_migration + PRD002_execution_snapshot
  -> PRD003_budget_context
  -> PRD004_activation_replay
PRD000..PRD004
required_owner_fact_audits
  -> PRD005_final_closure
```

## Requirement Mapping

1. Config/profile/auth declaration migration: PRD 000.
2. Runtime layout ownership/admission: PRD 001.
3. Immutable execution snapshot/provenance: PRD 002.
4. Token budget and explicit context wiring: PRD 003.
5. Activation persistence/replay/diagnostics: PRD 004.
6. Integration, compatibility, docs, final closure: PRD 005.

Primary parent requirements owned by this PRD:

- Primary Acceptance Criteria: 13

## Acceptance Criteria

1. Every parent Must Have and Acceptance Criterion has one primary PRD.
2. Existing JSON user data migrates or remains untouched with recoverable evidence.
3. Snapshot/replay tests preserve diagnostic provenance without authorization semantics.
4. Exact owner facts may pass local audits while source specs remain Open.

## Closure Evidence

1. Requirement/DAG and compatibility audit.
2. End-to-end migration/layout/snapshot/context/activation transcript.
3. External owner-fact audits and documentation evidence.
4. Final Spec035 closure summary with commands, failures and cleanup receipts.
