# PRD 001. image edit, variation, and streaming lifecycle

Status: Planned

## Goal

Image edit, mask, variation request와 partial/final streaming state를 provider-neutral model과 provenance chain으로 구현한다.

## Scope

1. Edit/mask/variation request/result model.
2. Source/mask artifact admission, MIME/size/provenance checks.
3. Started, partial, final, failed, cancelled streaming states.
4. Source-to-output artifact provenance chain.

## Non Scope

1. 모든 provider의 기능 parity를 요구하지 않는다.
2. Editor UI, canvas, prompt gallery를 만들지 않는다.
3. Partial frame을 final artifact로 자동 승격하지 않는다.

## Parent Requirement Mapping

- Owned scope 2-3
- Invariants 4-5
- Primary Must Have: 2-4
- Primary Acceptance Criteria: 2-3

## Acceptance Criteria

1. Provider capability absence is explicit unsupported, not generic failure or success.
2. Source/mask admission rejects invalid provenance, MIME, size, path traversal.
3. Partial events remain status evidence until a separate finalization rule succeeds.
4. Final artifact links all source ids and normalized generation options.

## Closure Evidence

1. Provider-neutral model tests and one adapter fixture.
2. Source/mask admission failure matrix.
3. Partial/final/cancel/failure lifecycle artifacts.
