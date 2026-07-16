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
- Spec 024 release evidence bucket checklist including PRD009 runtime execution coverage

## 비범위

- visual UI layout
- channel-specific formatting
- analytics dashboard
- SaaS admin console

## 구현 매핑

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowProjection`
  - `workflow_projection`
  - `WorkflowSpec024ReleaseEvidenceBucket`
  - `WorkflowSpec024ReleaseEvidence`
  - `WorkflowSpec024ReleaseEvidenceChecklist`
  - `workflow_spec024_release_evidence_checklist`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_projection_diagnostics_and_spec024_release_gate_are_evidence_backed`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. rendering and command naming consume `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`.
3. diagnostics consume 014 and release gates consume 016.
4. runtime evidence from PRD 000-009 is consumed but not redefined.

## Dependency Cut

1. Projection reads workflow state; it does not mutate session truth directly.
2. UI rendering details belong to 013.
3. Release gate cannot pass without verifier, budget, permission, replay evidence.
4. hosted dashboard and SaaS admin console are 비범위다.

## 데이터/상태 모델

1. `WorkflowProjection`: run id, state, pattern, progress, blocked reason, resume action을 가진다.
2. `WorkflowChildProjection`: child id, role, status, evidence digest를 가진다.
3. `WorkflowVerifierProjection`: verifier id, verdict, evidence status를 가진다.
4. `WorkflowReleaseGate`: PRD coverage bucket and pass/fail reason을 가진다.

## 정상 시퀀스

1. user requests workflow inspect/status.
2. projection reads workflow run record and diagnostics evidence.
3. CLI/TUI/API/channel show same state language.
4. release gate validates PRD evidence buckets.

## 실패 시퀀스

1. projection missing evidence cannot show success.
2. verifier failure is displayed as blocked/failed, not hidden.
3. budget exhaustion is visible with reason.
4. incomplete release evidence prevents closure claim.

## 검증 관점

1. projection snapshot covers running, blocked, failed, resume-needed, succeeded states.
2. release gate fails without verifier/budget/replay evidence.
3. channel/API/CLI projection consumes same data model.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml -p shacs-cli -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-cli --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-cli workflow_projection`
4. workflow projection model을 건드렸다면 `cargo test --manifest-path crates/Cargo.toml -p shacs-workflow workflow_projection`

## 완료 기준

- projection은 workflow id, objective, pattern, state를 보존한다.
- projection은 completed step, active child, pending barrier count를 계산한다.
- verifier gate는 user-facing status label로 표면화된다.
- resume point가 있으면 resume available이 true다.
- release checklist는 PRD 000-009 모든 bucket의 test 또는 manual QA evidence를 요구한다.
- evidence ref는 owner `024`와 redaction-valid 상태를 만족해야 release coverage로 인정된다.

## 구현 메모

- `crates/shacs-workflow` owns the shared `WorkflowProjection` vocabulary and release evidence checklist.
- `crates/shacs-session` projects persisted `runtime_workflow` metadata without raw workflow prompt or full diagnostic payload.
- `crates/shacs-cli`, `crates/shacs-api`, and `crates/shacs-channels` consume the same sanitized session/runtime workflow projection for inspect, diagnostics, API response, and bounded channel outbound summaries.
- `crates/shacs-tui` provides the minimal terminal-facing consumer through `workflow_progress_view(&WorkflowProjection)` and plain-text rendering; visual layout remains a UI-owner concern, not a 024 contract.
