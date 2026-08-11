# PRD 006. sequential integration and Spec 030 closure

Status: Complete (Scoped)

Implementation evidence: `crates/shacs-projection/tests/spec030_release_runner.rs`, `spec030_bwrap_record.rs`, `crates/shacs-projection/src/spec030/release_runner/surface_owner_linux_tests.rs`와 `.omo/evidence/spec030/prd006/current-worktree-final/closure-manifest.json`의 source-bound current-worktree evidence tree.

## Goal

PRD 000부터 005를 실제 trusted runtime surface로 순차 통합하고 Spec 030 closure evidence를 확정한다.

## Required dependency and integration order

```text
PRD 000 trusted profile
  -> PRD 001 tool hook
  -> PRD 002 process controls
  -> PRD 003 auth lifecycle
  -> PRD 004 optional sandbox
  -> PRD 005 resource/data disclosure
  -> PRD 006 integration closure
```

PRD 001과 PRD 002는 PRD 000을 요구한다. PRD 003, PRD 004, PRD 005는 PRD 000 이후 병렬 구현할 수 있다. 위 도식은 권장 integration order이며 final closure는 모든 evidence를 요구한다.

## External gates

1. Spec 035: trusted runtime, hook denial, process, sandbox, credential, resource diagnostics projection parity.
2. Spec 032: app/skill install과 supervisor lifecycle의 trusted-code disclosure.
3. Spec 031: config/profile/auth locator와 runtime layout persistence.

External spec 전체의 `Complete` 상태는 요구하지 않는다. 각 external gate는 030 owner fact를 소비하는 adapter, focused test, artifact locator만 제공하면 된다. 031·032·035가 Open이어도 해당 030-specific evidence가 통과하면 030 closure에 사용할 수 있다.

## Verification gates

1. Rust 변경이 있으면 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`.
2. Rust 변경이 있으면 `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`.
3. Rust 변경이 있으면 `cargo test --manifest-path crates/Cargo.toml --workspace`.
4. CLI/TUI/API에서 profile, hook block, process timeout, sandbox fallback, credential status, resource collision을 실제 사용한다.
5. Session/log/trace disclosure와 trace opt-in을 실제 artifact로 확인한다.
6. Core, optional extension, external service 기능을 서로 구분하는 문서 검토를 통과한다.

## Closure rule

Spec 030은 다음 조건이 모두 충족될 때만 `Complete (Scoped)`가 된다.

1. PRD 000~005가 각각 실제 owner surface와 evidence locator를 가진다.
2. 031·032·035 external gate가 current state를 정확히 투영한다.
3. Native trusted execution과 optional sandbox fallback이 사용자에게 숨겨지지 않는다.
4. 제거된 이전 permission-safety 계약을 현재 030 guarantee로 주장하는 문서가 없다.
5. 보안 sandbox, complete redaction, durable approval, universal process gate를 closure 결과로 과장하지 않는다.

## Non-scope

- 새 permission engine
- 새로운 process manager
- 중앙 secret vault
- 조직 policy rollout
- kernel isolation 증명
