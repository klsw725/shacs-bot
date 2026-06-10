# PRD 004: budget timeout and model routing

## 목표

Workflow가 비용과 intelligence routing을 숨기지 않도록 budget/model snapshot contract를 고정한다. Budget exhaustion은 success로 포장할 수 없으며, model routing은 provider 호출 전에 inspect 가능한 decision snapshot으로 남아야 한다.

## 범위

- workflow budget usage와 policy 비교
- token, iteration, heavy command exhaustion blocked decision
- remaining token projection
- classifier/child/verifier/synthesis role별 model hint snapshot
- fallback model policy preservation

## 비범위

- provider별 실제 과금 계산
- wall-clock timer implementation
- model availability probing
- automatic model benchmark selection

## 구현 매핑

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowBudgetDecision`
  - `workflow_budget_decision`
  - `WorkflowModelRouteSnapshot`
  - `workflow_model_route_snapshot`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_worktree_budget_and_model_routing_contracts_are_explicit`

## 완료 기준

- used token count가 max total 이상이면 budget decision은 blocked다.
- child run count가 max iteration 이상이면 blocked다.
- heavy command count가 max heavy command 이상이면 blocked다.
- budget이 남아 있으면 remaining token snapshot을 반환한다.
- verifier role은 verifier model hint를, unknown role은 fallback policy만 보존한다.
