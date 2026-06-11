# PRD 002: provider input budget and handoff

## 목표

Context files와 inline references가 provider context budget을 공유할 때의 priority, truncation, skipped evidence, 009 assembly handoff 계약을 고정한다.

## 범위

- budget priority order
- inline reference vs auto context file precedence
- token/byte estimate
- truncation and skipped evidence
- context block formatting input to 009

## 비범위

- provider-specific attachment API
- semantic compression algorithm
- vector retrieval

## 구현 요구사항

1. Active user message와 required runtime instructions는 context artifact보다 우선해야 한다.
2. Explicit inline reference는 자동 발견 context file보다 우선해야 한다.
3. Safety gate를 통과하지 못한 explicit reference는 budget priority와 무관하게 포함하지 않아야 한다.
4. Budget overflow는 silent drop이 아니라 skipped/truncated evidence를 남겨야 한다.
5. Context artifact formatting은 source label, trust label, truncation label을 포함해야 한다.
6. 009 assembly handoff는 typed artifact list로 이루어져야 하며 session message 원문을 수정하면 안 된다.

## SPEC 입력

1. 주관 spec은 `docs/specs/026-context-files-and-inline-references/SPEC.md`다.
2. provider input assembly는 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 소비한다.
3. provider request shaping은 `docs/specs/003-provider-runtime/SPEC.md`를 소비한다.
4. safety/redaction gate는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 소비한다.
5. diagnostics/replay evidence는 014/018을 소비한다.

## Dependency Cut

1. PRD 000 artifact model과 PRD 001 context file discovery가 선행된다.
2. 이 PRD는 provider input에 들어갈 artifact ordering과 budget cut을 소유한다.
3. Resolver source read는 PRD 003이 소유한다.
4. Session message 원문은 rewrite하지 않는다.
5. provider-native attachment API와 semantic compression은 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| budget priority | `crates/shacs-core/src/runtime/context.rs` 또는 context assembly adapter | explicit ref beats auto context |
| token/byte estimate | `crates/shacs-core/src/runtime/context.rs` | overflow truncates/skips with evidence |
| 009 handoff | `crates/shacs-core/src/runtime/runner.rs` | artifact list injected without message rewrite |
| diagnostics | `crates/shacs-core/src/runtime/diagnostics.rs` | budget usage snapshot |

## 데이터/상태 모델

1. `ContextBudgetInput`: active user message size, runtime instruction size, model context window option을 가진다.
2. `ContextArtifactPriority`: explicit inline, nearest context file, ancestor context file 등 ordering reason을 가진다.
3. `ContextBudgetDecision`: `included`, `truncated`, `skipped_budget`, `skipped_safety`를 구분한다.
4. `ProviderContextBlock`: source label, trust label, content, truncation label을 가진 009 handoff unit이다.
5. `ContextBudgetEvidence`: artifact digest, estimated tokens, final action, reason을 가진다.

## 정상 시퀀스

1. parser/resolver가 resolved artifact list를 만든다.
2. budget planner가 active user message와 runtime instructions를 먼저 예약한다.
3. explicit inline artifact를 자동 context file보다 먼저 배치한다.
4. 남은 budget에 따라 context files를 nearest-first로 포함한다.
5. 009 assembly에 provider context block list와 evidence를 넘긴다.

## 실패 시퀀스

1. safety gate가 denied한 artifact는 budget priority와 무관하게 포함하지 않는다.
2. budget overflow는 silent drop이 아니라 skipped/truncated evidence를 남긴다.
3. context window를 알 수 없으면 conservative limit 또는 explicit unknown evidence를 사용한다.
4. formatting 실패는 provider input corruption이 아니라 artifact-level failure로 남긴다.
5. session message 원문을 rewrite하는 fallback은 금지한다.

## 검증 관점

1. 첫 failing test는 explicit `@file`이 ancestor context file보다 우선하는지 확인한다.
2. budget overflow가 skipped/truncated evidence를 남기는지 확인한다.
3. safety-denied artifact가 explicit reference여도 포함되지 않는지 확인한다.
4. provider input snapshot은 source/trust/truncation label을 포함해야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml context_budget`
4. runner handoff를 건드렸다면 `cargo test --manifest-path crates/shacs-core/Cargo.toml runtime_runner`

## 완료 기준

- Budget test가 explicit reference 우선순위와 auto context truncation을 검증한다.
- Provider input snapshot evidence가 artifact source와 skipped reason을 표시한다.
