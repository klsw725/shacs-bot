# PRD 002. immutable execution snapshot and provenance

Status: Planned

## Goal

Provider 호출 직전의 config/profile, trusted-runtime refs, context/provider input을 immutable diagnostic snapshot으로 고정한다.

## Scope

1. Snapshot schema, id, time, source set, provenance digest.
2. Config/profile, trusted runtime, sandbox, credential status refs.
3. Context/provider/model/tool-resource identities and shaping version.
4. Adapter immutability and next-execution fresh snapshot rule.

## Non Scope

1. Snapshot은 policy/permission decision, durable approval, capability ceiling이 아니다.
2. Typed secret/redaction proof, universal containment, side-effect rollback을 보장하지 않는다.
3. Past snapshot을 current live source truth나 execution authorization으로 사용하지 않는다.

## Parent Requirement Mapping

- Owned scope 6-7, 10
- Invariants 6-7, 11
- Primary Must Have: 7-8
- Primary Acceptance Criteria: 8-9

## Acceptance Criteria

1. Snapshot records required non-secret references and provenance digest.
2. Adapter shaping cannot add, remove, or reread snapshot sources.
3. New execution resolves current live facts and creates a new snapshot.
4. Snapshot id/digest cannot authorize replay or live execution.

## Closure Evidence

1. Snapshot schema and round-trip tests.
2. Adapter source-reread/mutation failure tests.
3. Spec 030 fact-reference audit.
4. Authorization/non-guarantee read audit.
