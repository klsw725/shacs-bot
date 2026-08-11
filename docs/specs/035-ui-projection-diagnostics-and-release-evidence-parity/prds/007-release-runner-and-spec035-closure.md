# PRD 007. release runner and Spec 035 closure

Status: Planned revision (implemented baseline)

## Goal

This PRD is the sequential integration and final closure gate for Spec 035. It orders planned revisions 000 through 007, which retain implemented baselines, and unimplemented planned PRDs 008 through 009 before final closure. It defines the release runner, coverage matrix, lifecycle and projection parity smoke, failure triage, external-owner evidence, and exact conditions for closing Spec 035.

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
4. This revision preserves implementation evidence for PRDs 000-007 without treating their revised contracts as implemented. PRDs 008-009 remain unimplemented planned work, and parent Spec 035 stays `Status: Open`.

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

## External Evidence Locators

| External owner | Must prove | Required local read audit |
|---|---|---|
| Spec 029 | recovery, queue, reconnect, channel delivery facts consumed without reinterpretation | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec029-read-audit.md` |
| Spec 030 | trusted runtime profile, hook denial, path-specific process controls, sandbox mode, credential status, resource diagnostics, and data disclosures are rendered without inventing safety guarantees | `.omo/evidence/spec030/prd006/current-worktree-final/closure-manifest.json` |
| Spec 032 | app lifecycle/readiness/receipt facts exist for app projection | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec032-read-audit.md` |
| Spec 033 | goal id/state/stop reason/continuation budget owner facts and automation/event/coverage facts exist where PRD 009, release, or drop projection consumes them | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec033-read-audit.md` |
| Spec 034 | media/analyzer facts exist for media projection | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec034-read-audit.md` |
| Spec 031 | config/profile auth-source and execution-snapshot facts exist for onboard projection without moving schema, migration, or persistence ownership into 035 | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec035-read-audit.md` (historical locator) |

A blocked external locator is useful implementation evidence but is not final `PASS`. Each passing locator records the source spec status observed by `Read`, exact owner-fact artifact paths, command transcripts where applicable, and an artifact-backed audit. It does not require the external spec itself to be closed.

The historical 2026-08-04 machine verdict is indexed by `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/manifest.json`. The current Spec 030 row below supersedes its historical read-audit result:

| External owner | Current verdict | Current locator | Current blocker |
|---|---|---|---|
| Spec 029 | PASS | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec029-read-audit.md` | None. |
| Spec 030 | PASS | `.omo/evidence/spec030/prd006/current-worktree-final/closure-manifest.json` | Source-bound trusted-runtime owner facts, surface parity, bwrap lifecycle, manual QA and cleanup evidence pass. |
| Spec 032 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec032-read-audit.md` | Required app/resource lifecycle owner-fact artifacts are absent. |
| Spec 033 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec033-read-audit.md` | Required automation/evaluation owner-fact artifacts are absent. |
| Spec 034 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec034-read-audit.md` | Required media/analyzer owner-fact artifacts are absent. |
| Spec 031 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec035-read-audit.md` | Required config/auth-source/execution-snapshot owner-fact artifacts are absent. The historical artifact filename remains unchanged. |

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

`current-worktree` is the closure run shape. It returns nonzero while any required external owner audit is blocked, and it records dirty worktree state as separate triage when present. `success-fixture` proves the runner can assemble and validate a passing isolated fixture; it is not a semantic Spec 035 closure run. The `spec031-*` command, schema, and evidence names are shipped compatibility identifiers and remain unchanged after this document renumbering.

## Cargo Gates

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
cargo build --manifest-path crates/Cargo.toml --locked -p shacs-cli
cargo build --manifest-path crates/Cargo.toml --locked -p shacs-tui
```

## Required Surface Smoke

1. CLI: status, diagnostics, session, subagent, tool, approval, recover, context/plugin/app/media projection.
2. TUI: active session, approval, progress, degraded health, recovery action, invalid input, cancellation.
3. REPL: ordinary turn, shared command, priority command, malformed input, EOF.
4. Onboard: valid, cancelled, and resumed flow with masked auth handoff and credential-source audit.
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
| dirty worktree | unrelated mutations fail the release run |

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

Current result: the existing runner produced 56 coverage rows and all historical Spec031-owned command/artifact rows are mapped. PRD 008-009 evidence는 아직 없고, Spec029와 Spec030 external audit는 PASS다. Specs031, 032, 033, 034 owner-fact audit는 blocked 상태이므로 현재 summary는 `status: BLOCKED`이며 semantic Spec 035는 `Status: Open`을 유지한다.

## Authoring Verification

이번 revision은 기존 implemented-but-closure-blocked PRD 증거를 보존하고 PRD 008-009를 planned work로 추가한다. Parent는 새 PRD와 모든 required external audit이 통과한 current-worktree release runner가 zero를 반환할 때까지 `Open`이다.
