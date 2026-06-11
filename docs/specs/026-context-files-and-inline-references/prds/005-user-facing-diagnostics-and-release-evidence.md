# PRD 005: user-facing diagnostics and release evidence

## 목표

사용자가 context files와 inline references의 포함/스킵/절단 이유를 확인할 수 있는 CLI/TUI/local API projection과 release evidence gate를 고정한다.

## 범위

- context files list/inspect projection
- reference parse/resolve dry-run projection
- included/skipped/truncated/denied diagnostics
- release evidence checklist
- user docs update requirements

## 비범위

- visual design
- hosted dashboard
- remote connector management UI

## 구현 요구사항

1. Context file inspect는 discovered path, ordering, included/skipped/truncated status를 보여야 한다.
2. Reference parse dry run은 source content를 읽지 않고 token span/kind/target만 보여야 한다.
3. Reference resolve dry run은 read-only resolver를 사용하고 permission/redaction/budget status를 보여야 한다.
4. Diagnostics는 byte/token budget usage와 redaction/truncation status를 포함해야 한다.
5. User docs는 supported reference syntax, limits, safety behavior를 설명해야 한다.
6. Release gate는 parser, discovery, resolver, budget, safety, replay, docs evidence를 모두 요구해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/026-context-files-and-inline-references/SPEC.md`다.
2. CLI/TUI/local API projection은 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 소비한다.
3. diagnostics bundle은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.
4. release gates는 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`를 소비한다.
5. user docs는 context assembly owner 009를 대체하지 않고 026 syntax/safety만 설명한다.

## Dependency Cut

1. PRD 000 parser가 있어야 `context refs parse`가 의미를 가진다.
2. PRD 001-004가 구현되기 전에도 projection은 partial/not-supported status를 보여야 한다.
3. Rendering layout은 013이 소유하고 이 PRD는 projection data와 command semantics만 소유한다.
4. Diagnostics는 raw secret과 oversized content를 저장하지 않는다.
5. hosted dashboard와 remote connector management UI는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| context files list/inspect | `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs` | included/skipped/truncated projection |
| refs parse dry-run | `crates/shacs-cli/src/lib.rs` | no source read |
| refs resolve dry-run | `crates/shacs-cli/src/lib.rs`, `crates/shacs-core/src/runtime/context_refs.rs` | permission/redaction/budget status |
| release evidence | `docs/scripts` or existing release gate tests | parser/discovery/resolver/safety/docs coverage |

## 데이터/상태 모델

1. `ContextFilesInspectView`: path, order, source kind, include status, reason, digest를 가진다.
2. `ContextRefsParseView`: span, raw token, kind, normalized target, parse diagnostic을 가진다.
3. `ContextRefsResolveView`: artifact source, resolution state, permission decision, budget decision, redaction status를 가진다.
4. `ContextDiagnosticsSummary`: included/skipped/truncated/denied counts와 top-level reasons를 가진다.
5. `ContextReleaseEvidence`: parser, discovery, resolver, budget, safety, replay, docs bucket을 가진다.

## 정상 시퀀스

1. 사용자가 `context refs parse <message>`를 실행한다.
2. CLI는 source read 없이 parser output을 표시한다.
3. 사용자가 `context refs resolve <message>`를 실행한다.
4. runtime은 read-only resolver, safety, budget gate를 거쳐 status를 표시한다.
5. diagnostics bundle은 redaction-safe summary와 digest를 포함한다.

## 실패 시퀀스

1. parse-only command가 file/network를 읽으려 하면 실패로 본다.
2. resolve에서 denied artifact는 content 없이 denied reason만 표시한다.
3. diagnostics redaction 실패 시 raw content를 저장하지 않는다.
4. release evidence bucket이 비어 있으면 026 closure를 선언하지 않는다.
5. docs가 long-term memory/vector search/hosted connector로 과장하면 release gate에서 막는다.

## 검증 관점

1. 첫 failing test는 parse dry-run이 source read 없이 span/kind/target만 표시하는지 확인한다.
2. resolve dry-run은 missing/denied/truncated/budget-skipped reason을 표시해야 한다.
3. diagnostics snapshot은 raw secret 없이 summary와 digest만 포함해야 한다.
4. release evidence checklist는 parser, discovery, resolver, safety, replay, docs bucket을 모두 요구한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-cli/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-cli/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-cli/Cargo.toml context_refs`
4. core resolver를 건드렸다면 `cargo test --manifest-path crates/shacs-core/Cargo.toml context`

## 완료 기준

- 사용자는 왜 `@file` 또는 context file이 provider input에 포함되지 않았는지 알 수 있다.
- Release evidence가 owner `026` refs와 관련 regression을 요구한다.
