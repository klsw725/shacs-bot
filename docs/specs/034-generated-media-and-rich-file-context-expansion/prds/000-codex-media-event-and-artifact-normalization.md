# PRD 000. Codex media event and artifact normalization

Status: Complete (Scoped)

## Goal

Codex Responses image-generation event를 chat text와 분리해 provider-neutral generated-media event와 artifact persistence handoff로 정규화한다.

## Scope

1. Responses stream image event detection and normalization.
2. Raw payload/reference에서 generated artifact candidate로의 handoff.
3. Started, final, failed, cancelled base lifecycle.
4. Tool result의 opaque artifact reference와 safe diagnostics.

## Non Scope

1. Edit/mask/variation과 partial lifecycle은 PRD 001이 소유한다.
2. Remote URL admission/persistence는 PRD 002가 소유한다.
3. Raw base64, provider payload, signed URL을 session truth로 만들지 않는다.

## Parent Requirement Mapping

- Owned scope 1
- Invariants 1-3, 9-10
- Primary Must Have: 1
- Primary Acceptance Criteria: 1

## Acceptance Criteria

1. Image-generation event creates a generated-media candidate, not a text message.
2. Provider-specific raw options remain outside the stable artifact contract.
3. Raw base64/provider response/signed URL are absent from normalized result and projection artifacts.
4. Artifact candidate preserves provider/model/event lineage and failure reason.

## Closure Evidence

1. Provider event parser fixtures.
2. Tool/runtime handoff contract tests.
3. Projection-boundary payload absence audit.

Completion evidence: [Todo 6 Codex event receipt](../../../../.omo/evidence/spec034/task-6-codex-events.json), [Todo 8 persistence receipt](../../../../.omo/evidence/spec034/task-8-persistence.json), and [Spec034 closure mapping](../CLOSURE.md).
