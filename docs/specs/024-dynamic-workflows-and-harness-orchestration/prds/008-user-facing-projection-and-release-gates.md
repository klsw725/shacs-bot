# PRD 008: user-facing projection and release gates

## 목표

사용자에게 workflow 상태를 과장 없이 보여주고, Spec 024 closure를 release evidence checklist로 닫는 projection/release gate contract를 고정한다. Projection은 CLI/TUI/local API/channel이 공유할 수 있는 runtime-facing view이며, release gate는 PRD 000-008 전체 evidence coverage를 요구한다.

## 범위

- workflow projection schema label/version
- progress, active child, pending barrier count
- verifier status label
- budget usage projection
- worktree refs, blocked reason, next action, resume availability
- projection evidence filtering
- Spec 024 release evidence bucket checklist

## 비범위

- visual UI layout
- channel-specific formatting
- analytics dashboard
- SaaS admin console

## 구현 매핑

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowProjection`
  - `workflow_projection`
  - `WorkflowSpec024ReleaseEvidenceBucket`
  - `WorkflowSpec024ReleaseEvidence`
  - `WorkflowSpec024ReleaseEvidenceChecklist`
  - `workflow_spec024_release_evidence_checklist`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_projection_diagnostics_and_spec024_release_gate_are_evidence_backed`

## 완료 기준

- projection은 workflow id, objective, pattern, state를 보존한다.
- projection은 completed step, active child, pending barrier count를 계산한다.
- verifier gate는 user-facing status label로 표면화된다.
- resume point가 있으면 resume available이 true다.
- release checklist는 PRD 000-008 모든 bucket의 test 또는 manual QA evidence를 요구한다.
- evidence ref는 owner `024`와 redaction-valid 상태를 만족해야 release coverage로 인정된다.
