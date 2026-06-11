# PRD 000: workflow state and harness plan

## 목표

Spec 024의 첫 구현 웨이브는 dynamic workflow를 실제 실행하기 전에 필요한 typed foundation을 닫는다. 이 PRD는 prototype이 아니라 이후 PRD들이 공유할 workflow identity, state, harness plan, admission, checkpoint/resume, release evidence contract를 Rust 타입과 테스트로 고정하는 것을 목표로 한다.

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

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. session truth와 turn lifecycle은 `docs/specs/001-session-kernel/SPEC.md`를 소비한다.
3. orchestrator policy boundary는 `docs/specs/007-main-orchestrator-policy/SPEC.md`를 소비한다.
4. diagnostics/replay evidence는 014/018을 소비한다.

## Dependency Cut

1. 이 PRD는 모든 후속 workflow PRD의 typed foundation이다.
2. 실제 child spawn, verifier, worktree mutation, provider call은 구현하지 않는다.
3. Admission helper는 실행 권한을 부여하지 않고 decision만 만든다.

## 데이터/상태 모델

1. `WorkflowRunState`: admitted, running, waiting, blocked, succeeded, failed, cancelled, stale resume 같은 상태를 구분한다.
2. `WorkflowHarnessPlan`: pattern, child graph, verifier, budget, permission, worktree, resume policy snapshot을 가진다.
3. `WorkflowAdmissionResult`: regular, quick workflow, dynamic workflow, ask-user, blocked를 구분한다.
4. `WorkflowCheckpoint`: plan digest, resume point, terminal state, stale reason을 가진다.

## 정상 시퀀스

1. MainOrchestrator가 user request를 admission input으로 변환한다.
2. admission helper가 dynamic workflow 후보를 plan으로 만든다.
3. plan digest와 origin session/turn이 기록된다.
4. admitted run record가 checkpoint 가능한 형태로 저장된다.

## 실패 시퀀스

1. plan digest mismatch는 resume fail-closed가 된다.
2. missing resume point는 success가 아니라 blocked state가 된다.
3. unsupported pattern은 regular loop fallback이 아니라 explicit blocked/ask-user로 남긴다.

## 검증 관점

1. admission decision enum이 모든 branch를 fixture로 커버하는지 확인한다.
2. plan digest stable JSON regression을 둔다.
3. checkpoint resume mismatch가 fail-closed인지 확인한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml workflow_state`

## 완료 기준

- workflow state와 pattern이 serde 가능한 Rust 타입으로 제공된다.
- admission helper가 regular loop, quick workflow, dynamic workflow, ask-user, blocked를 구분할 수 있다.
- harness plan digest가 stable JSON digest로 계산된다.
- admitted workflow record가 plan digest와 origin session/turn을 보존한다.
- checkpoint resume decision이 terminal state, plan digest mismatch, missing resume point를 fail-closed로 처리한다.
- release evidence checklist가 owner `024`와 redaction validity를 요구한다.
- `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`, `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path crates/shacs-core/Cargo.toml workflow`를 통과한다.
