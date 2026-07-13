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

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowBudgetDecision`
  - `workflow_budget_decision`
  - `WorkflowModelRouteSnapshot`
  - `workflow_model_route_snapshot`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_worktree_budget_and_model_routing_contracts_are_explicit`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. provider/model routing은 `docs/specs/003-provider-runtime/SPEC.md`를 소비한다.
3. runtime cancellation/interrupt semantics는 001/012를 소비한다.
4. diagnostics and cost evidence는 014/018을 소비한다.

## Dependency Cut

1. PRD 000 harness plan이 budget/model snapshot을 담아야 한다.
2. 이 PRD는 budget accounting과 timeout enforcement를 소유한다.
3. Provider adapter 구현이나 새 model family 추가는 비범위다.
4. Budget exhaustion은 success가 아니라 blocked/failed state다.

## 데이터/상태 모델

1. `WorkflowBudgetSnapshot`: total tokens, child limits, verifier limits, wall-clock timeout, max parallelism을 가진다.
2. `WorkflowBudgetUsage`: consumed tokens, elapsed time, child count, verifier count를 가진다.
3. `WorkflowModelRoute`: parent/child/verifier model and reason을 가진다.
4. `WorkflowBudgetDecision`: continue, throttle, block_exhausted, cancel_timeout을 구분한다.

## 정상 시퀀스

1. admission이 budget snapshot을 plan에 고정한다.
2. child/verifier spawn 전에 remaining budget을 확인한다.
3. runner가 timeout과 parallelism limit을 적용한다.
4. usage가 workflow progress와 diagnostics에 누적된다.

## 실패 시퀀스

1. budget snapshot 없는 child spawn은 거부된다.
2. timeout은 cancel event와 blocked/failed state로 기록된다.
3. model route 실패는 fallback을 silent 적용하지 않고 evidence를 남긴다.
4. budget exhaustion을 success로 포장하지 않는다.

## 검증 관점

1. 첫 failing test는 budget 없이 child spawn이 거부되는지 확인한다.
2. timeout cancellation이 workflow state에 반영되는지 확인한다.
3. max parallelism enforcement regression을 둔다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml -p shacs-workflow -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-workflow --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-workflow workflow_budget`

## 완료 기준

- used token count가 max total 이상이면 budget decision은 blocked다.
- child run count가 max iteration 이상이면 blocked다.
- heavy command count가 max heavy command 이상이면 blocked다.
- budget이 남아 있으면 remaining token snapshot을 반환한다.
- verifier role은 verifier model hint를, unknown role은 fallback policy만 보존한다.
