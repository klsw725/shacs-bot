# PRD 001: pattern engine and child graph

## 목표

Spec 024의 workflow pattern과 child graph contract를 runtime이 소비할 수 있는 typed decision으로 고정한다. 이 PRD는 실제 subagent spawn engine이 아니라, fan-out, barrier, structured child result, synthesis가 success를 조기 선언하지 못하게 막는 핵심 의미론을 닫는다.

## 범위

- child run status와 terminal 판정
- structured child result와 evidence refs
- completed dependency 기준 ready step 계산
- required child 누락 또는 실패에 대한 barrier decision
- synthesis accepted/rejected/unresolved child 분류
- unresolved child 또는 required failure가 final success를 막는 contract

## 비범위

- provider 호출 또는 child process 실행
- tournament round scheduler
- child prompt 생성 template
- 사용자에게 보이는 실시간 progress rendering

## 구현 매핑

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowChildRunStatus`
  - `WorkflowChildResult`
  - `WorkflowBarrierDecision`
  - `WorkflowSynthesisOutcome`
  - `workflow_ready_step_ids`
  - `workflow_barrier_decision`
  - `workflow_synthesis_outcome`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_barrier_verifier_and_synthesis_fail_closed`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. PRD 000의 workflow state, pattern, harness plan type을 소비한다.
3. child runtime execution은 `docs/specs/011-subagent-runtime/SPEC.md`를 소비하되 이 PRD에서는 실행하지 않는다.

## Dependency Cut

1. PRD 000 typed foundation이 선행되어야 한다.
2. 이 PRD는 graph planning, barrier decision, synthesis outcome만 소유한다.
3. provider call, scheduler, prompt template, progress rendering은 비범위다.

## 데이터/상태 모델

1. `WorkflowChildNode`: id, dependencies, required flag, expected output contract를 가진다.
2. `WorkflowBarrierDecision`: waiting, ready, blocked를 구분하고 reason을 가진다.
3. `WorkflowSynthesisOutcome`: accepted, rejected, unresolved child sets와 final-success eligibility를 가진다.
4. `WorkflowEvidenceRef`: owner `024`, digest, redaction status를 가진다.

## 정상 시퀀스

1. harness plan의 child graph가 ready calculation에 들어간다.
2. completed dependencies만 ready step 계산에 사용된다.
3. required child가 모두 complete되면 barrier가 ready가 된다.
4. synthesis가 accepted/rejected/unresolved child를 분리한다.

## 실패 시퀀스

1. required child result가 없으면 barrier는 waiting이다.
2. required child가 terminal failure이면 blocked가 된다.
3. unresolved child가 있으면 final success가 금지된다.
4. invalid evidence ref는 synthesis evidence에서 제외된다.

## 검증 관점

1. 첫 failing test는 required child missing이 waiting을 반환하는지 확인한다.
2. required failure가 blocked로 이어지는지 확인한다.
3. unresolved child가 final success를 막는 regression을 둔다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml -p shacs-workflow -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-workflow --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-workflow workflow_barrier`

## 완료 기준

- ready step 계산은 완료된 dependency만 신뢰한다.
- required child result가 없으면 barrier는 waiting을 반환한다.
- required child가 terminal failure면 barrier는 blocked를 반환한다.
- synthesis는 completed/running/failed child를 분리한다.
- unresolved child가 남아 있으면 final success를 허용하지 않는다.
- child evidence refs는 owner `024`와 redaction 상태가 유효한 항목만 synthesis evidence로 남는다.

## 구현 메모

- Live runtime은 pattern별 별도 native engine 대신 dependency와 barrier를 표현한 bounded DAG를 공통 scheduler로 실행한다.
- `tournament`는 pre-expanded bounded static DAG만 허용하며 runtime이 round/bracket을 동적으로 확장하는 native tournament scheduler는 이 PRD의 비범위다.
