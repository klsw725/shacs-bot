# PRD 004. context, extension, app, and media projection

Status: Planned

## Goal

Context files, inline references, plugins, hooks, apps, attachments, generated/uploaded media, analyzer 결과의 owner evidence를 shared projection으로 변환하고 모든 지원 표면에서 inclusion/reason 의미와 redaction을 보존한다.

## Scope

1. Context file and inline reference projection.
2. Plugin/hook discovery, enablement, readiness, diagnostics projection.
3. App lifecycle/readiness projection adapter for evidence produced by Spec 032.
4. Attachment/media/analyzer projection adapter for evidence produced by Spec 034.
5. Included/skipped/blocked/degraded/missing/unsupported/extraction_failed reason parity.

## Non Scope

1. Context resolution, plugin execution, hook dispatch, app lifecycle, media generation, or analyzer execution rules를 소유하지 않는다.
2. Missing Spec 032/034 owner evidence를 fixture-only success로 대체하지 않는다.
3. Raw filesystem path, URL credential, prompt, attachment body, media bytes, stdout, stderr를 projection에 포함하지 않는다.

## SPEC Inputs

1. PRDs 000, 001, and 003.
2. `crates/shacs-core/src/runtime/context_diagnostics.rs`, `context_handoff.rs`, `context_safety.rs`.
3. Existing CLI plugin/app/context surfaces in `crates/shacs-cli/src/lib.rs`.
4. Spec 025 plugin/hook evidence, Spec 026 context evidence, Spec 027 attachment baseline, Specs 032 and 034 external owner contracts.
5. Parent Spec 031 `Must Have` 4 and 7, `Acceptance Criteria` 5.

## Dependency Cut

1. PRD 000 owns canonical vocabulary, PRD 001 owns adapter parity, PRD 003 owns readiness aggregation.
2. Spec 032 owns app state and receipts; Spec 034 owns media/analyzer state and evidence.
3. Missing external owner evidence is canonical state `blocked` or `unavailable` with safe reason code `missing_external_owner_evidence`; such a result cannot satisfy final Spec 031 closure.

## Projection Requirements

| Capability | Required safe fields | Required reason coverage |
|---|---|---|
| context file | opaque ref, source kind, order, budget/result summary | included, skipped, blocked, missing |
| inline reference | opaque ref, resolver kind, result summary | included, unsupported, extraction_failed, blocked |
| plugin/hook | opaque extension ref, enabled/readiness state, safe diagnostic | ready, degraded, blocked, unavailable |
| app | opaque app ref, lifecycle/readiness state, receipt ref when owner provides it | ready, degraded, blocked, missing, unsupported |
| attachment/media | opaque artifact ref, media kind, analysis/result summary | included, skipped, unsupported, extraction_failed, blocked |

Reason codes must remain canonical across CLI, TUI, API, WebSocket, and channel presentation. Human summaries may differ only in formatting.

## Failure Rules

1. A displayed filename or label cannot substitute for an opaque owner ref.
2. Missing extractor/analyzer support is `unsupported`, not `included` or generic success.
3. A blocked item cannot disappear from aggregate readiness or context diagnostics.
4. Absolute host paths and raw URLs containing credentials fail redaction tests.
5. Plugin/app/media output text cannot change permission or readiness state.

## Verification

1. Build deterministic fixtures for every capability/reason row.
2. Compare canonical output across supported adapters.
3. Test mixed batches containing included, skipped, blocked, and `extraction_failed` items.
4. Test absent Spec 032/034 owner evidence as explicit blocked external dependency.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core context
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-projection
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli plugins
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-api
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

## Agent-Executed Surface QA

1. Create an isolated workspace with one context file, one unsupported inline reference, one disabled plugin, one app fixture, and attachment/media recorded fixtures.
2. Run CLI inspect/diagnostics, API diagnostics/session queries, and WebSocket/channel projection capture.
3. Verify canonical reason equality and redaction.
4. Record external owner blocked artifacts when Spec 032 or 034 evidence is unavailable; do not mark closure pass.

## Closure Evidence

1. Capability/reason matrix: `.omo/evidence/spec031/prd004/capability-reason-matrix.json`.
2. Cross-surface parity artifact: `.omo/evidence/spec031/prd004/projection-parity.json`.
3. External owner read audits: `.omo/evidence/spec031/prd004/external/`.
4. Redaction and mixed-batch audit: `.omo/evidence/spec031/prd004/redaction-audit.md`.
5. QA transcripts and cleanup receipts under `.omo/evidence/spec031/prd004/qa/`.

## Exit Criteria

1. Every capability preserves canonical reason and opaque lineage.
2. Raw path, secret, payload, and process output are absent.
3. Missing external-owner evidence remains a visible closure blocker.
4. Focused gates and real-surface QA pass.
