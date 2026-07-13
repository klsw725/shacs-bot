# PRD 009: runtime execution wiring and monitoring

## 목표

Dynamic workflow spec closure를 위해 남은 runtime execution wiring gap을 고정한다. 이 PRD는 workflow state, pattern engine, verifier, worktree, budget, resume, projection PRD를 대체하지 않고, MainOrchestrator와 AgentRunner 실제 실행 경로에서 그것들이 연결되어 관찰 가능한지 검증한다.

## 범위

- workflow admission에서 execution start까지의 handoff.
- child task spawn/execution/merge result wiring.
- progress event and monitor state projection.
- interrupt/stop/restart propagation.
- final synthesis and verifier gating.
- runtime diagnostics and closure evidence.

## 비범위

- 새 workflow pattern family 설계.
- JavaScript workflow interpreter.
- external workflow marketplace.
- organization-admin approval workflow.
- provider-native orchestration feature.

## 구현 요구사항

1. MainOrchestrator admission result는 regular loop, quick workflow, dynamic workflow, blocked, ask-user를 runtime execution branch로 명확히 연결해야 한다.
2. Workflow execution start는 typed harness plan, workflow id, budget snapshot, permission ceiling, current session key를 함께 기록해야 한다.
3. Child execution handoff는 parent context를 복사하되 session truth를 직접 공유하지 않아야 한다.
4. Child progress, child completion, verifier verdict, synthesis progress는 monitorable event로 발행되어야 한다.
5. Interrupt/stop/restart marker는 parent workflow와 running child에게 propagation되어야 하며, ignored interrupt가 success로 포장되면 안 된다.
6. Verifier failure 또는 required child failure는 final success를 막아야 한다.
7. Final synthesis는 child output과 verifier evidence를 소비하되 child가 session store를 직접 mutate한 것처럼 취급하면 안 된다.
8. Diagnostics bundle은 admission, child graph, runtime handoff, interrupt, final synthesis, blocked reason을 redacted evidence로 포함해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. PRD 000-008의 state, graph, verifier, worktree, budget, permission, replay, projection contracts를 소비한다.
3. MainOrchestrator/session truth boundary는 001/007을 소비한다.
4. AgentRunner and subagent boundary consume 003/011.

## Dependency Cut

1. This PRD wires runtime execution; it does not redesign workflow patterns.
2. Child execution handoff copies context but does not share mutable session truth.
3. Interrupt propagation must reach parent workflow and running children.
4. Provider-native orchestration and organization-admin approval workflow are 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| admission to execution branch | `crates/shacs-core/src/runtime/runner.rs`, `crates/shacs-workflow/src/lib.rs` | dynamic workflow smoke enters branch |
| child handoff and progress | `crates/shacs-core/src/runtime/subagent.rs`, `crates/shacs-workflow/src/lib.rs` | child progress event emitted |
| interrupt propagation | `crates/shacs-core/src/runtime/runner.rs`, cancellation registry | stop marker cancels workflow and child |
| synthesis and verifier gate | `crates/shacs-workflow/src/lib.rs` | verifier failure blocks final success |

## 데이터/상태 모델

1. `WorkflowExecutionHandle`: run id, parent session key, child handles, cancellation token, budget snapshot을 가진다.
2. `WorkflowProgressEvent`: admitted, child_started, child_completed, verifier_completed, synthesizing, terminal을 구분하고 terminal event의 state로 completed, failed, blocked, cancelled를 표시한다.
3. `WorkflowExecutionOutcome`: succeeded, failed, blocked, cancelled, resume_required를 가진다.
4. `WorkflowRuntimeDiagnostic`: handoff, interrupt, child result, verifier, synthesis evidence refs를 가진다.

## 정상 시퀀스

1. MainOrchestrator admission returns dynamic workflow.
2. runner creates workflow execution handle and event log entry.
3. child handoff copies context and scoped tools.
4. child/verifier progress events are emitted.
5. synthesis consumes child outputs and verifier evidence before final response.

## 실패 시퀀스

1. child handoff failure blocks workflow and emits diagnostic.
2. interrupt marker cancels parent and running children.
3. verifier failure prevents final success.
4. ignored interrupt or missing child result cannot be reported as success.

## 검증 관점

1. end-to-end smoke covers admission, child execution, progress event, verifier gate, synthesis.
2. interrupt propagation regression observes parent and child cancellation.
3. diagnostics/replay evidence explains execution flow without live rerun.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-core --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-core workflow`

## 완료 기준

- End-to-end runtime test 또는 smoke가 dynamic workflow admission에서 child execution, progress event, verifier gate, final synthesis까지 실제 경로를 통과한다.
- Interrupt propagation regression이 workflow와 child를 모두 관찰한다.
- Diagnostics/replay evidence만으로 workflow 실행 흐름을 설명할 수 있다.
- 문서와 사용자 가이드는 runtime wiring이 prototype stub가 아니라 실제 supported path인지, 또는 아직 Draft gap인지 명확히 표시한다.

## 구현 메모

- `crates/shacs-core/src/runtime/workflow.rs`는 provider 호출 없이 기존 `shacs-workflow` contract helper를 소비하는 read-only runtime smoke path를 제공한다.
- `crates/shacs-core/tests/runtime_workflow.rs`는 `admitted`, `child_started`, `child_completed`, `verifier_completed`, `synthesizing`, `terminal` event와 verifier fail/missing fail-closed, read-only child registry, child/verifier provenance fail-closed, parent session truth isolation을 검증한다.
- Admission branch smoke는 `decide_workflow_admission`의 dynamic decision이 read-only runtime workflow path로 들어가고, non-dynamic decision은 regular loop로 남는 것을 검증한다.
- Interrupt propagation smoke는 workflow execution handle의 cancellation token과 child id set이 cancel event 및 `cancelled` terminal state로 반영되는 것을 검증한다.
- Diagnostics/replay smoke는 harness/child/verifier graph digest, event phase, terminal state, verifier status를 live 재실행 없이 설명하며 `replay_live_actions_allowed = false`를 검증한다.
- Spec 024 release evidence checklist는 PRD009 runtime execution bucket을 필수로 요구하며, session UX diagnostics는 persisted metadata에서 diagnostics ref 문자열만 투영해 restart/replay inspection이 raw diagnostic payload 없이 가능하게 한다.
- 이 메모는 Wave 3 / PRD 009의 deterministic runtime closure evidence다. Provider-backed live subagent execution, write-capable worktree merge, UI projection은 PRD 009 read-only closure 범위가 아니라 후속 wave 또는 다른 PRD가 소유한다.
