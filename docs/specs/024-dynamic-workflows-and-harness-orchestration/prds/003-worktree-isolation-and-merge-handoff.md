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

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowWorktreeRequest`
  - `WorkflowWorktreeDecision`
  - `workflow_worktree_decision`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_worktree_budget_and_model_routing_contracts_are_explicit`

## 완료 기준

- write가 필요한 child가 `None` 또는 `ReadOnlySnapshot` policy만 가지면 blocked다.
- `IsolatedWorktreeRequired`는 approval 없이는 blocked다.
- approval된 required isolation은 deterministic branch name을 반환한다.
- existing worktree ref가 있으면 새 격리를 만들지 않고 reuse decision을 반환한다.
- 이 contract는 merge를 직접 수행하지 않고 parent handoff evidence만 만든다.
