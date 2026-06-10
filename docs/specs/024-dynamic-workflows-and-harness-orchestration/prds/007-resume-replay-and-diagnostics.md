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

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowDiagnosticsManifest`
  - `workflow_diagnostics_manifest`
  - existing `WorkflowCheckpoint` and `workflow_resume_decision`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_checkpoint_resume_requires_matching_plan_digest_and_nonterminal_state`
  - `workflow_projection_diagnostics_and_spec024_release_gate_are_evidence_backed`

## 완료 기준

- diagnostics manifest는 plan, child graph, verifier graph digest를 모두 가진다.
- stale result refs는 manifest에서 손실되지 않는다.
- invalid owner 또는 failed redaction evidence는 manifest evidence에서 제외된다.
- checkpoint resume은 plan digest mismatch와 terminal state를 fail-closed로 처리한다.
- replay/debugging은 destructive workflow command 없이 manifest를 해석할 수 있다.
