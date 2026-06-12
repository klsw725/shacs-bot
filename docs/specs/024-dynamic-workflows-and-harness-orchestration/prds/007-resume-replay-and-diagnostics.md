# PRD 007: resume replay and diagnostics

## 목표

Workflow interruption, stale child result, replay/debugging이 destructive re-execution 없이 해석 가능하도록 diagnostics manifest contract를 고정한다. Diagnostics는 harness plan, child graph, verifier graph, stale refs, redacted evidence refs를 연결한다.

## 범위

- workflow diagnostics manifest
- harness plan digest preservation
- child graph digest
- verifier graph digest
- stale result refs
- owner `024`와 redaction-valid evidence filtering
- checkpoint 기반 projection과 resume availability 연계

## 비범위

- full event log replay engine
- diagnostic file persistence layout
- stale child cancellation command
- external trace viewer

## 구현 매핑

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowDiagnosticsManifest`
  - `workflow_diagnostics_manifest`
  - existing `WorkflowCheckpoint` and `workflow_resume_decision`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_checkpoint_resume_requires_matching_plan_digest_and_nonterminal_state`
  - `workflow_projection_diagnostics_and_spec024_release_gate_are_evidence_backed`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. checkpoint and replay evidence consumes 001/014/018.
3. child stale result semantics consume 011 subagent boundary.
4. destructive replay prohibition consumes 010/022 safety boundary.

## Dependency Cut

1. PRD 000 checkpoint schema and PRD 001 graph ids are prerequisites.
2. Replay explains recorded evidence; it does not live-run child tools.
3. Resume failure is visible blocked state, not success.
4. stale/late result handling is owned here, not by child agents.

## 데이터/상태 모델

1. `WorkflowResumePoint`: plan digest, graph cursor, completed child ids, pending verifier ids를 가진다.
2. `StaleWorkflowResult`: child id, result digest, stale reason, discard evidence를 가진다.
3. `WorkflowReplayRecord`: event log, child graph, verifier verdict, merge decision summary를 가진다.
4. `WorkflowDiagnosticsBundle`: redacted plan, events, budget, permission, blocked reason을 가진다.

## 정상 시퀀스

1. workflow event log와 checkpoint가 저장된다.
2. restart 후 plan digest와 resume point가 검증된다.
3. incomplete child/verifier는 policy에 따라 resume 또는 blocked로 표시된다.
4. diagnostics bundle은 replay 가능한 redacted evidence를 제공한다.

## 실패 시퀀스

1. plan digest mismatch는 blocked resume이 된다.
2. late child result는 terminal workflow에 merge되지 않는다.
3. replay가 destructive action을 live-run하려 하면 fail-closed한다.
4. missing evidence는 success가 아니라 diagnostics gap으로 남는다.

## 검증 관점

1. digest mismatch resume failure test를 둔다.
2. stale child result discard regression을 둔다.
3. replay no-live-dispatch regression을 destructive fixture로 검증한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-workflow/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-workflow/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-workflow/Cargo.toml workflow_resume`

## 완료 기준

- diagnostics manifest는 plan, child graph, verifier graph digest를 모두 가진다.
- stale result refs는 manifest에서 손실되지 않는다.
- invalid owner 또는 failed redaction evidence는 manifest evidence에서 제외된다.
- checkpoint resume은 plan digest mismatch와 terminal state를 fail-closed로 처리한다.
- replay/debugging은 destructive workflow command 없이 manifest를 해석할 수 있다.
