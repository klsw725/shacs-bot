# PRD 007. release runner and Spec 035 closure

Status: Open

구현 상태: 카탈로그·분류 검증 및 별도 execution 증거 수용 경로 구현, closure 차단. 기존 `spec031-release-runner`는 Spec031 66행과 Spec035 80행의 총 146행 및 typed classification v2를 구분한다. Classification은 실행 증거가 아니므로 최신 보존 current-worktree 원본은 commands 0·exit 1, `BLOCKED`이며 [차단 기록](../CLOSURE.md)을 따른다.

## Goal

이 PRD는 Spec035의 순차 통합 및 최종 closure gate다. 구현된 PRD000-009의 검증을 통합하며 release runner, coverage matrix, lifecycle/projection parity smoke, 실패 분류, 외부 owner 증거와 최종 판정 조건을 정의한다. 구현과 과거 QA가 존재해도 현재 소스 결속과 독립 gate가 충족되지 않으면 닫지 않는다.

This PRD defines no new domain state, projection vocabulary, interactive behavior, delivery guarantee, or external-owner evidence.

## Scope

1. Acyclic dependency DAG and implementation waves.
2. One-to-one mapping from Spec 035 Must Have, Acceptance Criteria, and Closure Evidence to owner PRDs.
3. Machine-readable and human-readable release runner artifacts.
4. Focused and full-workspace Cargo gates.
5. CLI, TUI, REPL, onboard, API, WebSocket/channel, diagnostics, backpressure, lifecycle smoke QA.
6. Failure injection, redaction, non-guarantee, documentation, and cleanup gates.

## Non Scope

1. No Rust implementation is owned by this PRD except the release runner shell and artifact assembly defined here.
2. No partial closure, manual approval, prose-only, grep-only, screenshot-only, or cargo-test-only proof is accepted.
3. Missing required owner evidence from Specs 029 through 034 remains a hard closure blocker for the capabilities that consume it.
4. PRD008 transport/reconnect와 PRD009 mixed Tasks CLI/API/TUI는 구현되어 있다. 과거 검사 기록을 현재 release PASS로 승격하지 않으며 parent Spec035는 `Status: Open`을 유지한다.

## Dependency DAG

```text
PRD000_shared_projection
  -> PRD001_surface_adapters
  -> PRD002_approval_progress_recovery
  -> PRD003_readiness_diagnostics
  -> PRD008_transport_snapshot_reconnect
  -> PRD009_goal_tasks_projection

PRD001_surface_adapters
  -> PRD002_approval_progress_recovery
  -> PRD003_readiness_diagnostics
  -> PRD004_context_extension_app_media
  -> PRD006_backpressure_accounting
  -> PRD008_transport_snapshot_reconnect
  -> PRD009_goal_tasks_projection

PRD002_approval_progress_recovery
  -> PRD005_interactive_flows
  -> PRD006_backpressure_accounting

PRD003_readiness_diagnostics
  -> PRD004_context_extension_app_media
  -> PRD005_interactive_flows

PRD004_context_extension_app_media
  -> PRD005_interactive_flows

PRD005_interactive_flows
  -> PRD009_goal_tasks_projection

PRD006_backpressure_accounting
  -> PRD008_transport_snapshot_reconnect

Spec030_trusted_runtime_operational_facts
  -> PRD002_approval_progress_recovery
  -> PRD003_readiness_diagnostics
  -> PRD005_interactive_flows

Spec032_app_lifecycle_facts
  -> PRD004_context_extension_app_media

Spec034_media_analyzer_facts
  -> PRD004_context_extension_app_media

Spec029_recovery_delivery_facts
  -> PRD006_backpressure_accounting
  -> PRD008_transport_snapshot_reconnect

Spec033_automation_event_facts
  -> PRD006_backpressure_accounting
  -> PRD009_goal_tasks_projection

Spec031_config_auth_source_snapshot_facts
  -> PRD005_interactive_flows
  -> PRD008_transport_snapshot_reconnect

PRD008_transport_snapshot_reconnect
  -> PRD007_final_closure

PRD009_goal_tasks_projection
  -> PRD007_final_closure

PRD000..PRD006,PRD008..PRD009
  -> PRD007_final_closure

Required_external_owner_fact_audits
  -> PRD007_final_closure
```

Forbidden edges:

1. PRD 007 cannot redefine PRD 000 through 006 contracts.
2. Spec 035 cannot create domain truth owned by Specs 029 through 034.
3. Spec 035 cannot define config/profile auth-source persistence or execution snapshot truth owned by Spec 031.
4. External specs cannot depend on Spec 035 already being closed to produce their typed owner records. A local external read audit may pass while an external spec remains open only when the exact required owner facts and artifact-backed evidence exist; an external spec closure status is not required and cannot create a cycle.

## Implementation Waves

### Wave 0. Baseline characterization

Exit: current session/workflow/CLI/API/TUI/channel/diagnostics/release helpers are inventoried; known missing TUI/REPL/wizard/readiness/drop/runner surfaces are recorded as blockers.

### Wave 1. Shared projection foundation

Owner: PRD 000. Exit: typed schema, vocabulary, redaction, and consumer inventory pass.

### Wave 2. Non-interactive adapter parity

Owner: PRD 001. Exit: CLI/API/WebSocket/channel canonical parity and real-surface evidence pass.

### Wave 3. Lifecycle and health projection

Owners: PRDs 002 and 003. Exit: approval/progress/recovery lineage and readiness/diagnostics severity parity pass.

### Wave 4. Context and extension projection

Owner: PRD 004. Exit: context/plugin/app/media reason parity passes and required external-owner evidence exists.

### Wave 5. Interactive surfaces

Owner: PRD 005. Exit: live TUI, REPL command parity, and credential-source/status-only onboard wizard QA pass.

### Wave 6. Delivery accounting

Owner: PRD 006. Exit: slow consumer, reconnect, coalescing, drops, and final delivery accounting pass.

### Wave 7. Transport and reconnect coherence

Owner: PRD 008. Exit: capability matrix, unsupported-mutation preflight rejection, snapshot-first reconnect ordering, generation/gap accounting이 통과한다.

### Wave 8. Goal and read-only Tasks projection

Owner: PRD 009. Exit: goal accounting parity, owner locator coverage, CLI/API/TUI read-only Tasks view가 통과한다.

### Wave 9. Release and closure

Owner: PRD 007. Entry requires all prior exit evidence and external locators. Exit requires every gate below in one release ledger.

## Requirement Ownership Mapping

| Parent requirement | Primary contract owner | Required proof or external input |
|---|---|---|
| Must Have 1 | PRD 000 | PRD 001 adapter fixtures |
| Must Have 2 | PRD 001 | Capability owner fixtures |
| Must Have 3 | PRD 005 | PRDs 002-003 projections |
| Must Have 4 | PRD 005 | Existing shared command contract |
| Must Have 5 | PRD 005 | Specs 030 and 031 owner facts |
| Must Have 6 | PRD 002 | PRDs 001 and 005 surface proof |
| Must Have 7 | PRD 004 | Specs 032 and 034 owner facts where required |
| Must Have 8 | PRD 003 | Component owner observations |
| Must Have 9 | PRD 006 | Specs 029 and 033 owner facts where required |
| Must Have 10 | PRD 007 | PRDs 000-006 and 008-009 artifacts |
| Must Have 11-12 | PRD 008 | PRDs 000-001 and 006, Specs 029 and 031 facts |
| Must Have 13 | PRD 009 | PRDs 000-001 and 005, Spec 033 goal facts |
| Acceptance 1 schema contract | PRD 000 | PRD 001 adapter proof |
| Acceptance 1 adapter proof | PRD 001 | PRD 000 schema |
| Acceptance 2 | PRD 002 | PRD 005 interactive proof |
| Acceptance 3 | PRD 006 | PRD 001 adapters |
| Acceptance 4 | PRD 002 | PRD 005 interactive proof |
| Acceptance 5 | PRD 004 | PRD 001 adapters |
| Acceptance 6 | PRD 003 | PRD 001 adapters |
| Acceptance 7 | PRD 005 | Live runtime source and recorded fixtures |
| Acceptance 8-9 | PRD 007 | Release ledger and documentation audit |
| Closure Evidence 1 | PRD 000 | Schema/read audit |
| Closure Evidence 2 | PRD 001 | CLI surface QA |
| Closure Evidence 3 | PRD 005 | TUI terminal QA |
| Closure Evidence 4 | PRD 005 | REPL/onboard terminal QA |
| Closure Evidence 5 | PRD 001 | API/WebSocket/channel QA |
| Closure Evidence 6 | PRD 006 | Deterministic accounting QA |
| Closure Evidence 7-8 | PRD 007 | Release and documentation audits |
| Acceptance 10-11 | PRD 008 | Capability matrix and snapshot-first reconnect proof |
| Acceptance 12 | PRD 009 | Goal accounting and owner-locator Tasks proof |
| Closure Evidence 9 | PRD 008 | Transport/reconnect artifacts |
| Closure Evidence 10 | PRD 009 | Goal/Tasks parity artifacts |

Shared acceptance rows name one primary contract owner and, where necessary, one proof consumer. This does not duplicate domain ownership.

## 외부 증거 위치와 유효 범위

Todo10의 [외부 owner 감사](../../../../.omo/evidence/spec035/prd000-009/external-owner-audits.json)는 아래 선택된 검사 7개의 PASS를 기록한다. 이는 외부 스펙 전체 완료나 모든 필수 소비 fact의 검증, 현재 source-bound release PASS를 뜻하지 않는다. F1은 요구 fact 전체와 이 검사들의 대응이 충분하지 않다고 보고했다. 현재 검증을 위해서는 낡은 todo10 결속을 보존한 채 필요한 fact별 새 실행 증거가 필요하다.

| Owner | 실제 기록된 검사 범위 | Todo10 검사 기록 |
|---|---|---|
| Spec029 | Durable inbound 복원·stale lease requeue, durable cancellation 뒤 child late success 거절 | `commands/020-ext-spec029-dispatch.txt`, `commands/021-ext-spec029-child.txt`; 2개 통과 |
| Spec030 | Local provider의 live resource·diagnostics·trace discovery | `commands/022-ext-spec030.txt`; 1개 통과 |
| Spec031 | Runtime inspect owner source의 API/CLI/bundle readiness parity; config/auth/snapshot 전체 증거는 아님 | `commands/026-ext-spec031-exact-adapter.txt`; 1개 통과 |
| Spec032 | Enable/disable projection이 process truth를 생성하지 않음 | `commands/023-ext-spec032.txt`; 1개 통과 |
| Spec033 | User-visible automation channel event의 delivery projection | `commands/024-ext-spec033.txt`; 1개 통과 |
| Spec034 | 주입된 analyzer를 통한 stored video runtime context routing | `commands/025-ext-spec034.txt`; 1개 통과 |

위 상대 경로의 기준은 `.omo/evidence/spec035/prd000-009/`다. Spec034의 정확한 adapter 검사 통과는 누락된 `.omo/evidence/spec034/task-12-integration.json` 때문에 실패한 별도 workspace fixture 검사를 대체하지 않는다. 상세 차단과 artifact 위치는 [CLOSURE.md](../CLOSURE.md)를 따른다.

## Release Runner Contract

One repository-owned command or script must:

1. Create a run id, evidence root, fixture registry, command registry, and cleanup registry.
2. Execute or ingest focused Cargo gates, workspace gates, lifecycle smoke, projection parity smoke, and failure injections.
3. Write machine-readable `manifest.json`, `coverage-matrix.json`, `results.json`, and `failure-triage.json`.
4. Write a human-readable `summary.md` with exact command locators and failed/blocked reasons.
5. Return non-zero when any required gate, artifact, cleanup receipt, or external evidence is missing.
6. Remain independent of a specific CI vendor.
7. Record `package`, `filter`, `tests_run`, and `tests_failed` for every focused Cargo test gate; a required gate with `tests_run == 0` fails even when Cargo exits zero.

Current command examples:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --run-id spec031-current --evidence-root /tmp/spec031-current --repo-root . --mode current-worktree
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --run-id spec031-success-fixture --evidence-root /tmp/spec031-success-fixture --repo-root . --mode success-fixture
```

`current-worktree`의 최신 canonical 입력은 종료 조건 45행 모두 BLOCKED인 classification v2다. 정확한 행·owner 집합, 분류 구조와 inventory를 검증한 뒤 실행 receipt가 아니므로 `BlockedExternalEvidence`로 차단하며 required command를 실행하지 않는다. 역사적 v1의 40행 검증과 혼동하지 않는다. 별도 `spec035.prd000_009_closure_execution.v1` 경로는 요구사항·owner·gate·명령 transcript·전후 소스 결속을 검증해 coverage를 산출한다. 구성 증거를 사용한 수용 테스트는 실제 제품 closure 증명이 아니다. Dirty worktree 자체는 별도 provenance 관측이지 자동 실패가 아니며 source binding 불일치와 구분한다. `success-fixture`도 격리된 runner 동작 검사일 뿐 semantic closure가 아니다. 기존 binary `spec031-release-runner`, schema `spec031.release_runner.v2`와 mode 식별자는 유지한다. 최종 [current-worktree 원본](../../../../.omo/evidence/spec035/closure/final-compiled-runner-current/summary.md)은 146행 중 PASS 5/BLOCKED 141, Spec035 PASS 0/BLOCKED 80, commands 0·exit 1이고 [fixture 원본](../../../../.omo/evidence/spec035/closure/final-compiled-runner-fixture/summary.md)은 commands 17·exit 0이다. Todo11의 source digest 불일치와 66행 결과는 별도 역사 기록이다.

## Cargo Gates

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml --locked -p shacs-cli -p shacs-tui
```

## Required Surface Smoke

1. CLI: status, diagnostics, session, subagent, tool, approval, recover, context/plugin/app/media projection.
2. TUI: active session, approval, progress, degraded health, recovery action, invalid input, cancellation.
3. REPL: ordinary turn, shared command, priority command, malformed input, EOF.
4. Onboard: secret-ref-only 입력의 정상·취소·재개, raw secret 거절 및 credential-source 감사.
5. API: health/readiness, diagnostics, session, subagent, and tool projections where supported.
6. WebSocket/channel: subagent/tool events where supported, progress, final outcome, unsupported/skipped integration, reconnect/slow consumer.
7. Lifecycle: local install/onboard/start/diagnose/stop/recover using an isolated workspace and recorded cleanup.

## Failure Injection Matrix

| Failure | Required result |
|---|---|
| unknown projection schema/state | explicit parse or compatibility failure |
| raw secret/credential-bearing path or URL/payload/process output | rejected or projection-redacted before artifact persistence; no runtime-wide redaction claim |
| missing owner evidence | unavailable/unknown or blocked, never success |
| approval expiry/retry | lineage preserved, no silent allow |
| ephemeral confirmation or hook veto | separate state; no durable approval or remembered allow |
| unsupported transport mutation | rejected before side effect |
| reconnect delta before snapshot or stale generation | rejected with visible gap/generation reason |
| task row without owner locator | rejected or unavailable, never synthetic success |
| stale checkpoint/marker | no recovery success claim |
| slow consumer/queue full | backpressure/drop visible |
| reconnect gap | gap visible, no lossless claim |
| dropped progress with final delivery | both facts visible |
| misleading success text | typed owner outcome wins |
| repeated cancellation/interruption | idempotent terminal projection, no duplicate action |
| hung command | bounded timeout and cleanup receipt |
| dirty worktree | 별도 provenance 관측으로 기록하며 source binding 불일치는 차단 |

## Documentation and Non-Guarantee Review

Before closure, verify:

1. No visual design system, mobile app, SaaS/admin dashboard, CI vendor, fleet operation, or multi-user control is introduced as a requirement.
2. No text claims exactly-once delivery, complete redaction, universal sandbox/process envelope, kernel isolation, durable confirmation, typed secret reference, resource-digest authorization, or process-alive readiness.
3. Old owner specs link to 035 only for projection parity and release rendering; their closed domain contracts remain unchanged.
4. `README.md` and `docs/USAGE.md` describe new TUI/REPL/onboard/readiness/release surfaces only after real surface QA passes.

## Final Closure Condition

Spec 035 may leave `Status: Open` and `docs/specs/README.md` may remove it from the open owner set only when:

1. PRDs 000 through 006 and PRDs 008 through 009 have passing focused gates, real-surface QA, artifacts, and cleanup receipts.
2. All external read audits required by implemented capabilities have local `PASS` and no blocked owner remains.
3. The dependency DAG is acyclic and every parent requirement is mapped.
4. Full workspace Cargo gates pass with the workspace manifest.
5. The release runner returns zero and its machine/human artifacts pass artifact-backed read audit.
6. All required surface smoke and failure injection rows pass.
7. Redaction and non-guarantee review passes.
8. User documentation matches the actually verified surface.

If any item is missing, Spec 035 remains `Status: Open`.

현재 결과: PRD000-009 구현과 todo10 증거는 존재한다. 그러나 todo11 runner 수정 이후 todo10의 역사적 source binding은 낡았고 current-worktree는 `BLOCKED`다. Workspace fixture 누락, 소유권 불명 경로 288개, 미해결 기본 설정 재작성 사고도 독립 차단이다. 과거 PASS 행이나 success-fixture로 이를 해소하지 않으며 Spec035는 `Status: Open`을 유지한다.

카탈로그 열거, 원본 산출물 보존, F1-R01 execution 수용 경로 및 R01-A01 경로 별칭 차단은 구현과 제한적 재검증을 마쳤다. 원래 네 F2 결함과 stale 비동기 queue 역방향 사례도 scoped confirmed다. 최종 compiled QA는 encoded WebSocket query를 포함한 실제 표면 검증을 통과했으며, 과거 URL 실패와 바이너리 부재 기록은 역사 자료다. 그러나 F1의 baseline·정확한 owner fact·전체 의미 증거와 외부 차단은 남아 있다. 최종 전체 F1/F4 승인을 부여하거나 canonical BLOCKED 행을 PASS로 바꾸지 않는다. 최신 실행 근거는 [CLOSURE](../CLOSURE.md)를 따른다.

## Authoring Verification

이번 문서 갱신은 실제 구현, 과거 QA 기록, 현재 release 유효성을 구분한다. Todo10·11 artifact와 역사적 hash는 수정하지 않는다. 문서 감사는 제품 재검증이나 최종 독립 감사를 대체하지 않으며 parent는 모든 필수 gate가 통과할 때까지 `Open`이다.
