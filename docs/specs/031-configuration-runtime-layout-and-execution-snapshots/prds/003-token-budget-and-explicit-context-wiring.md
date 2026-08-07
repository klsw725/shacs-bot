# PRD 003. token budget and explicit context wiring

Status: Planned

## Goal

Provider/model-aware token budget과 truncation evidence를 만들고 explicit config-provided extra context를 live agent loop에 연결한다.

## Scope

1. Tokenizer or explainable estimator selection.
2. Included, truncated, skipped context-block evidence.
3. Active user message/required instruction preservation.
4. Explicit extra context source, precedence, duplicate handling, snapshot linkage.

## Non Scope

1. Budget을 permission, capability, safety accounting으로 표현하지 않는다.
2. Overflow를 silent drop으로 처리하지 않는다.
3. Configured context가 trusted-resource disclosure/path/budget gates를 우회하지 않는다.

## Parent Requirement Mapping

- Owned scope 8-9
- Invariants 8-10
- Primary Must Have: 9-11
- Primary Acceptance Criteria: 10-11

## Acceptance Criteria

1. Provider/model chooses tokenizer or records estimator fallback and uncertainty.
2. Truncation plan preserves required messages and explains every excluded block.
3. Explicit context reaches live provider handoff without duplicating default discovery.
4. Snapshot records source identity, precedence, inclusion/truncation result.

## Closure Evidence

1. Tokenizer/estimator/budget matrix.
2. Truncation and required-message regression tests.
3. Explicit-context live handoff transcript.
4. 009/026 owner-boundary audit.
