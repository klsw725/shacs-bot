# PRD 003: worktree isolation and merge handoff

## 목표

Write-capable workflow child가 main worktree를 조용히 오염시키지 않도록 worktree isolation decision contract를 고정한다. 이 PRD는 git worktree 생성 명령 자체가 아니라, runtime이 언제 기존 격리를 쓰고, 언제 새 격리를 요구하며, 언제 parent approval 없이는 blocked로 남겨야 하는지를 닫는다.

## 범위

- child worktree request snapshot
- read-only child의 worktree 미필요 decision
- existing worktree ref reuse
- isolated worktree required/optional policy
- approval 없는 required write child blocked decision
- merge handoff 전 branch/worktree ref evidence surface

## 비범위

- git command execution
- 자동 merge 또는 conflict resolution
- destructive cleanup
- parent 승인 UI

## 구현 매핑

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowWorktreeRequest`
  - `WorkflowWorktreeDecision`
  - `workflow_worktree_decision`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_worktree_budget_and_model_routing_contracts_are_explicit`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. filesystem and shell safety는 010/022를 소비한다.
3. worktree mutation은 session truth와 분리된 effect로 다룬다.
4. diagnostics/replay evidence는 014/018을 소비한다.

## Dependency Cut

1. PRD 000-002의 plan/graph/verifier가 선행되어야 한다.
2. Write-capable child만 isolated worktree policy를 요구한다.
3. Merge approval은 child가 직접 수행하지 않고 parent handoff로 남긴다.
4. destructive git reset/force checkout은 비범위다.

## 데이터/상태 모델

1. `WorkflowWorktreePolicy`: none, read-only, isolated-write를 구분한다.
2. `ChildWorktreeAssignment`: child id, path, base ref, cleanup policy를 가진다.
3. `WorkflowDiffEvidence`: changed files, diff digest, redaction status를 가진다.
4. `MergeHandoff`: pending, approved, rejected, blocked_cleanup을 구분한다.

## 정상 시퀀스

1. write-capable child admission이 isolated worktree assignment를 만든다.
2. child는 assigned path 안에서만 실행된다.
3. completion 후 diff evidence가 수집된다.
4. verifier가 diff evidence를 검토한다.
5. parent workflow가 merge handoff를 사용자에게 표시한다.

## 실패 시퀀스

1. child가 assigned path 밖을 수정하려 하면 blocked된다.
2. diff collection 실패는 merge success가 아니라 blocked evidence가 된다.
3. cleanup 실패는 diagnostics에 남고 success로 숨기지 않는다.
4. unverified diff는 merge-ready가 아니다.

## 검증 관점

1. child path escape가 blocked되는 regression을 둔다.
2. diff digest와 redaction evidence를 snapshot으로 검증한다.
3. cleanup failure가 visible diagnostic인지 확인한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml -p shacs-workflow -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-workflow --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-workflow workflow_worktree`

## 완료 기준

- write가 필요한 child가 `None` 또는 `ReadOnlySnapshot` policy만 가지면 blocked다.
- `IsolatedWorktreeRequired`는 approval 없이는 blocked다.
- approval된 required isolation은 deterministic branch name을 반환한다.
- existing worktree ref가 있으면 새 격리를 만들지 않고 reuse decision을 반환한다.
- 이 contract는 merge를 직접 수행하지 않고 parent handoff evidence만 만든다.
