# PRD 000: workflow state and harness plan

## 목표

Spec 023의 첫 구현 웨이브는 dynamic workflow를 실제 실행하기 전에 필요한 typed foundation을 닫는다. 이 PRD는 prototype이 아니라 이후 PRD들이 공유할 workflow identity, state, harness plan, admission, checkpoint/resume, release evidence contract를 Rust 타입과 테스트로 고정하는 것을 목표로 한다.

## 범위

- `WorkflowRunState`와 terminal state 판정
- `WorkflowPattern` taxonomy
- workflow admission input/result와 기본 admission decision helper
- `WorkflowHarnessPlan` typed schema
- workflow child/verifier/context/tool/permission/worktree/model/budget/checkpoint/merge/resume policy snapshot
- harness plan stable digest
- admitted workflow run record
- checkpoint construction and resume decision
- PRD 000 release evidence checklist

## 비범위

- 실제 subagent spawn orchestration
- barrier execution engine
- verifier execution
- worktree creation/merge
- provider invocation or model routing execution
- CLI/TUI/local API projection

이 비범위는 기능 축소가 아니라 후속 PRD의 owner boundary다. PRD 000은 이들이 공유할 schema와 checkpoint 기준을 먼저 고정한다.

## 구현 매핑

- `crates/shacs-core/src/runtime/workflow.rs`: typed workflow foundation과 helper 함수
- `crates/shacs-core/src/runtime/mod.rs`: public runtime exports
- `crates/shacs-core/tests/runtime_workflow.rs`: admission, digest, checkpoint/resume, release evidence tests

## 완료 기준

- workflow state와 pattern이 serde 가능한 Rust 타입으로 제공된다.
- admission helper가 regular loop, quick workflow, dynamic workflow, ask-user, blocked를 구분할 수 있다.
- harness plan digest가 stable JSON digest로 계산된다.
- admitted workflow record가 plan digest와 origin session/turn을 보존한다.
- checkpoint resume decision이 terminal state, plan digest mismatch, missing resume point를 fail-closed로 처리한다.
- release evidence checklist가 owner `023`과 redaction validity를 요구한다.
- `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`, `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path crates/shacs-core/Cargo.toml workflow`를 통과한다.
