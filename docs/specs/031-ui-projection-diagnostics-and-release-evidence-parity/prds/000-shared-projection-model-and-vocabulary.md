# PRD 000. shared projection model and vocabulary

Status: Planned

## Goal

Spec 031의 모든 사용자 표면이 소비할 typed projection model과 bounded status vocabulary를 `shacs-projection` 안에 정의한다. 이 PRD는 owner runtime record를 읽기 위한 view contract만 소유하며 session, approval, queue, app, media 같은 domain truth나 transition을 새로 만들지 않는다.

## Scope

1. Session, turn, subagent, approval, tool, context, plugin, app, media, diagnostics, release evidence를 표현하는 공통 envelope와 item vocabulary.
2. Projection status, reason, severity, freshness, lineage, redacted reference의 공통 표현.
3. owner record에서 projection으로 변환할 때의 provenance와 unavailable/unknown 처리.
4. CLI, TUI, API, WebSocket, channel adapter가 재사용할 serialization contract.

## Non Scope

1. Domain state transition, durable storage schema, approval 판단, recovery 판단, delivery truth를 소유하지 않는다.
2. Visual theme, layout system, mobile 또는 hosted dashboard를 정의하지 않는다.
3. 032의 app state, 033의 automation state, 034의 media/analyzer state를 선행 구현하지 않는다.

## SPEC Inputs

1. Parent Spec 031의 `Invariants`, `Must Have` 1-2, `Acceptance Criteria` 1, `Closure Evidence` 1.
2. Existing projection roots: `crates/shacs-projection/src/projection.rs`, `crates/shacs-projection/src/diagnostics_release.rs`.
3. Existing domain projections: `crates/shacs-session/src/lib.rs`, `crates/shacs-session/src/diagnostics.rs`, `crates/shacs-workflow/src/lib.rs`.
4. Redaction boundary: `crates/shacs-redaction/src/lib.rs`, `crates/shacs-utils/src/diagnostics_sanitizer.rs`.

## Dependency Cut

1. This is the foundation PRD and has no Spec 031 predecessor.
2. Existing owner records remain authoritative. Missing owner evidence maps to `unavailable` or `unknown`; it must not be fabricated as success, empty, or zero.
3. PRDs 001 through 007 consume this contract and must not add private surface-only status names.

## Required Contract

The implementation must provide one shared envelope with at least:

1. Schema version and projection kind.
2. Bounded state and severity.
3. Stable opaque subject ref and optional parent/action/digest lineage.
4. Redacted reason code plus safe summary; raw payload is forbidden.
5. Source owner and observed-at/freshness metadata sufficient to distinguish current, stale, unavailable, and unknown evidence.
6. Optional child items and capability-specific details represented by typed variants, not arbitrary surface JSON.

Required bounded vocabularies:

| Family | Required values |
|---|---|
| Common availability | `ready`, `degraded`, `blocked`, `unavailable`, `unknown` |
| Approval | `pending`, `allowed`, `denied`, `expired`, `skipped`, `retry_consumed` |
| Inclusion/result reason | `included`, `skipped`, `blocked`, `degraded`, `missing`, `unsupported`, `extraction_failed` |
| Progress delivery | `live`, `coalesced`, `dropped`, `reconnected`, `final_delivered`, `final_pending`, `final_failed` |

Capability-specific states may be added only when an owner contract requires them. Every adapter must preserve the canonical value and may add presentation text without changing its meaning.

## Failure Rules

1. Unknown enum values or unsupported schema versions fail parsing explicitly.
2. Raw path, secret, provider payload, tool arguments, environment map, process handle, raw stdout, or raw stderr must not enter the projection envelope.
3. Missing evidence must not become `ready`, `allowed`, `included`, `final_delivered`, or numeric zero.
4. Projection serialization must not mutate or persist owner records.

## Verification

1. Add round-trip and unknown-version tests in `shacs-projection`.
2. Add table tests proving every required vocabulary value serializes identically for all adapter-facing formats.
3. Add redaction tests with secret, absolute path, raw payload, process handle, stdout, and stderr fixtures.
4. Characterize existing session/workflow projection behavior before migration; preserve domain meaning while replacing duplicate surface vocabulary.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-projection
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-session session_ux
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-projection --all-targets -- -D warnings
```

## Closure Evidence

1. Typed model and vocabulary symbols under `crates/shacs-projection/`.
2. Vocabulary and serialization test transcript at `.omo/evidence/spec031/prd000/projection-schema-tests.txt`.
3. Redaction audit at `.omo/evidence/spec031/prd000/projection-redaction-audit.md`.
4. Consumer inventory showing no supported adapter defines a conflicting canonical state at `.omo/evidence/spec031/prd000/consumer-inventory.json`.

## Exit Criteria

1. The shared model exists inside surface adapters.
2. Required vocabulary is typed, bounded, versioned, and redacted.
3. Missing evidence is never projected as success.
4. Focused format, test, and clippy gates pass.
