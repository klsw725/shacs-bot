# PRD 004: permission redaction and replay safety

## 목표

Context files와 inline references가 local permission, secret redaction, replay safety를 약화하지 않도록 공통 safety gate를 고정한다.

## 범위

- protected target gate
- secret-like content redaction
- prompt injection labeling
- network permission gate
- diagnostics redaction
- replay without live refetch

## 비범위

- full DLP engine
- organization compliance workflow
- signed remote document policy

## 구현 요구사항

1. Context artifact는 tool permission이나 approval state를 부여하지 않아야 한다.
2. Protected path denial은 provider input에 raw path/content를 과도하게 노출하지 않아야 한다.
3. Secret-like content는 diagnostics와 provider context formatting 전에 redaction pass를 통과해야 한다.
4. URL and external git-like content는 untrusted prompt-injection label을 가져야 한다.
5. Replay는 live URL fetch나 mutable working tree diff를 다시 수행하지 않고 recorded digest/excerpt/evidence를 사용해야 한다.
6. Strict mode가 아닌 한 reference denial은 전체 turn crash가 아니라 visible skipped evidence여야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/026-context-files-and-inline-references/SPEC.md`다.
2. protected target과 secret redaction은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 소비한다.
3. approval/permission mode는 `docs/specs/022-auto-approval-permissions/SPEC.md`를 소비한다.
4. diagnostics bundle은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.
5. replay evidence는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`를 소비한다.

## Dependency Cut

1. 이 PRD는 모든 resolver와 context file artifact가 통과해야 할 safety gate다.
2. Context reference는 tool permission이나 approval state를 부여하지 않는다.
3. URL/external content는 untrusted prompt-injection label을 가져야 한다.
4. Replay는 live refetch나 mutable git state 재실행을 하지 않는다.
5. full DLP engine과 organization compliance workflow는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| protected target denial | `crates/shacs-core/src/runtime/context_refs.rs`, safety helper | protected path denied |
| redaction pass | `crates/shacs-core/src/runtime/diagnostics.rs` 또는 redaction helper | secret-like content redacted |
| replay evidence | `crates/shacs-core/src/runtime/` | no live URL refetch |
| prompt-injection label | `crates/shacs-core/src/runtime/context.rs` | external content labeled |

## 데이터/상태 모델

1. `ContextPermissionDecision`: `allowed`, `denied_protected`, `denied_network`, `denied_outside_workspace`, `requires_approval`을 구분한다.
2. `ContextRedactionStatus`: `not_needed`, `redacted`, `redaction_failed_blocked`를 가진다.
3. `ContextTrustLabel`: `workspace_user_authored`, `workspace_file`, `git_readonly`, `external_untrusted`를 구분한다.
4. `ContextReplayEvidence`: source digest, redacted excerpt, resolution time metadata, no-live-refetch marker를 가진다.
5. `ContextSafetyDiagnostic`: denied/skipped reason과 redaction status를 user-facing하게 설명한다.

## 정상 시퀀스

1. resolver output이 safety gate로 들어온다.
2. permission decision이 source kind와 path/network policy를 평가한다.
3. allowed artifact는 redaction pass를 통과한다.
4. trust label과 replay evidence가 붙는다.
5. provider handoff는 redaction/trust metadata를 포함한다.

## 실패 시퀀스

1. protected target은 content를 읽지 않고 denied evidence만 남긴다.
2. secret-like content redaction 실패 시 artifact 포함을 중단한다.
3. network disabled 상태의 URL reference는 skipped diagnostic이 된다.
4. replay 중 live URL fetch 또는 mutable git diff 재실행을 시도하면 fail-closed한다.
5. strict mode가 아닌 denial은 전체 turn crash가 아니라 skipped evidence로 남는다.

## 검증 관점

1. 첫 failing test는 protected path reference가 content 없이 denied되는지 확인한다.
2. secret redaction fixture는 diagnostics와 provider context 양쪽을 확인한다.
3. replay no-refetch regression은 URL/git mutable source fixture로 고정한다.
4. external URL artifact는 prompt-injection/trust label을 반드시 가져야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml context_safety`
4. permission mode를 건드렸다면 `cargo test --manifest-path crates/shacs-config/Cargo.toml permission`

## 완료 기준

- Secret redaction, protected path denial, replay no-refetch regression이 존재한다.
- Diagnostics bundle은 raw secret과 oversized content를 저장하지 않는다.

## 구현 상태

Status: Implemented for PRD 004 runtime safety boundary. PRD 005 projection/release evidence and live runtime handoff are also implemented.

Evidence:

- `crates/shacs-core/src/runtime/context_safety.rs` adds a shared safety gate for resolved artifacts, redacts secret-like content before provider handoff, emits user-facing diagnostics, labels trust, and records replay evidence with a no-live-refetch marker.
- `crates/shacs-core/src/runtime/context_resolvers.rs` denies protected file targets such as `.env` and SSH/private-key paths before reading content; `context_files.rs` also denies context-file symlinks whose canonical target is protected; `context.rs` applies the same canonical workspace/protected-target gate to legacy bootstrap files before they enter the system prompt.
- `crates/shacs-core/src/runtime/context_handoff.rs` consumes trust labels from the safety model, including `external_untrusted` for URL artifacts and `workspace_user_authored` for context files.
- `cargo test --manifest-path crates/shacs-core/Cargo.toml context_safety` passes with secret redaction, protected path recognition, external URL trust labeling, and replay no-refetch coverage. `cargo test --manifest-path crates/shacs-core/Cargo.toml bootstrap_files_skip` covers bootstrap symlinks to protected or outside-workspace targets.
