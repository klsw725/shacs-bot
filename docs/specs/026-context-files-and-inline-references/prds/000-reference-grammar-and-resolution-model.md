# PRD 000: reference grammar and resolution model

## 목표

User message 안의 inline `@` reference를 안정적으로 파싱하고, provider input에 넘길 normalized resolved context artifact model을 고정한다.

## 범위

- inline `@` token grammar
- escaped `\@` handling
- fenced code block 내부 skip
- file/folder/diff/staged/git/url kind classification
- unresolved, skipped, denied, resolved state
- resolved context artifact shape

## 비범위

- 실제 filesystem/git/url resolver 구현
- provider prompt formatting 최종 UX
- long-term memory 또는 vector search

## 구현 요구사항

1. Parser는 user message 원문을 mutate하지 않고 reference span과 normalized target만 산출해야 한다.
2. Escaped `\@`와 fenced code block 내부 `@` token은 기본적으로 reference로 해석하지 않아야 한다.
3. Ambiguous token은 안전하게 unresolved로 남기고 원문 text로 유지해야 한다.
4. Reference kind는 file, folder, diff, staged, git, url, unsupported를 구분해야 한다.
5. Resolved artifact model은 source, display name, digest, byte count, token estimate, redaction/truncation status, permission evidence를 담아야 한다.
6. Parser failure는 전체 turn failure가 아니라 reference-level diagnostic이어야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/026-context-files-and-inline-references/SPEC.md`다.
2. provider input handoff는 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 소비한다.
3. user message/session truth 경계는 `docs/specs/001-session-kernel/SPEC.md`와 `docs/specs/007-main-orchestrator-policy/SPEC.md`를 소비한다.
4. user-facing parse/resolve projection은 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 소비한다.
5. 이 PRD는 실제 filesystem/git/url read를 구현하지 않는다.

## Dependency Cut

1. 이 PRD는 parser와 artifact type만 소유한다.
2. Resolver는 후속 PRD 003에서 구현한다.
3. Parser는 user message 원문을 rewrite하지 않는다.
4. unsupported/ambiguous token은 fail-open text로 남기고 reference-level diagnostic만 만든다.
5. long-term memory, vector search, hosted connector는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| inline reference parser | `crates/shacs-core/src/runtime/context_refs.rs` 또는 신규 context module | email/handle/code block ignored |
| artifact type | `crates/shacs-core/src/runtime/context.rs` | serde snapshot and digest fields |
| parse diagnostics | `crates/shacs-core/src/runtime/context_refs.rs` | unsupported token warning |
| CLI dry-run hook | `crates/shacs-cli/src/lib.rs` | parse command shows spans without reading files |

## 데이터/상태 모델

1. `ContextReferenceSpan`: original byte range, raw token, normalized target, kind를 가진다.
2. `ContextReferenceKind`: `file`, `folder`, `diff`, `staged`, `git`, `url`, `unsupported`, `unresolved`를 구분한다.
3. `ResolvedContextArtifact`: source, display name, content option, digest, byte count, token estimate, redaction/truncation/permission status를 가진다.
4. `ReferenceParseDiagnostic`: escaped, code block ignored, ambiguous, unsupported, malformed target을 구분한다.
5. `ContextResolutionState`: `parsed`, `resolved`, `skipped`, `denied`, `failed`를 가진다.

## 정상 시퀀스

1. user message가 parser로 들어온다.
2. parser는 fenced code block과 escaped `\@` 범위를 먼저 제외한다.
3. 남은 token을 kind와 normalized target으로 분류한다.
4. parser는 original message와 reference span list를 반환한다.
5. 후속 resolver가 사용할 empty artifact shell 또는 parse model이 만들어진다.

## 실패 시퀀스

1. email address나 social handle처럼 ambiguous한 token은 reference로 해석하지 않는다.
2. malformed URL/git token은 unsupported diagnostic으로 남기고 원문을 보존한다.
3. parser 내부 오류는 turn crash가 아니라 reference parse error로 보고된다.
4. parse 단계에서 filesystem/git/url을 읽으려는 경로는 금지한다.
5. unsupported reference는 provider input에 raw resolved content를 만들지 않는다.

## 검증 관점

1. 첫 failing test는 escaped `\@`, fenced code block, email/handle이 reference가 되지 않는지 확인한다.
2. file/folder/diff/staged/git/url kind fixture를 둔다.
3. parse command는 source read 없이 span/kind/target만 보여야 한다.
4. artifact model serde snapshot은 009 handoff에 필요한 필드를 고정해야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml context_reference_parse`
4. CLI dry-run을 건드렸다면 `cargo test --manifest-path crates/shacs-cli/Cargo.toml context_refs`

## 완료 기준

- Parser unit test가 escape, code block, adjacent punctuation, URL, git revision, missing target case를 포함한다.
- Artifact model은 009 context assembly에 넘길 수 있는 typed boundary를 가진다.

## 구현 상태

Status: Implemented for PRD 000 parser/model boundary. Later Spec 026 PRDs and live runtime handoff are also implemented.

Evidence:

- `crates/shacs-core/src/runtime/context_refs.rs` adds side-effect-free inline `@` parsing with escaped `\@`, fenced code skip, ambiguous email/handle diagnostics, kind classification, and parsed artifact shells.
- Public runtime exports are available through `crates/shacs-core/src/runtime/mod.rs`.
- `cargo test --manifest-path crates/shacs-core/Cargo.toml context_reference_parse` passes with coverage for escape, fenced code, email/handle ignored, adjacent punctuation, bare URL, `@url:`, `@git:<rev>`, `@git:<rev>:<path>`, `@diff`, `@staged`, and missing/malformed targets.
