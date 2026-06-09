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

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowChildRunStatus`
  - `WorkflowChildResult`
  - `WorkflowBarrierDecision`
  - `WorkflowSynthesisOutcome`
  - `workflow_ready_step_ids`
  - `workflow_barrier_decision`
  - `workflow_synthesis_outcome`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_barrier_verifier_and_synthesis_fail_closed`

## 완료 기준

- ready step 계산은 완료된 dependency만 신뢰한다.
- required child result가 없으면 barrier는 waiting을 반환한다.
- required child가 terminal failure면 barrier는 blocked를 반환한다.
- synthesis는 completed/running/failed child를 분리한다.
- unresolved child가 남아 있으면 final success를 허용하지 않는다.
- child evidence refs는 owner `024`와 redaction 상태가 유효한 항목만 synthesis evidence로 남는다.
