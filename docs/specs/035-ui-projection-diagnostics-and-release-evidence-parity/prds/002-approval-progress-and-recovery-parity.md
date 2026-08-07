# PRD 002. approval, progress, and recovery parity

Status: Planned revision (implemented baseline)

## Goal

Durable approval lifecycle, Spec 030의 ephemeral confirmation과 hook denial, progress versus final outcome, recovery state를 shared projection으로 연결해 CLI와 interactive/API/channel 표면이 owner lineage와 terminal meaning을 보존하게 한다.

## Scope

1. Approval pending, allowed, denied, expired, skipped, retry-consumed projection.
2. Progress event와 final outcome의 명시적 분리.
3. Interrupted run, pending marker, restart marker, recover command, session checkpoint projection.
4. Pending follow-up와 active turn 상태의 surface parity fixtures.
5. Ephemeral confirmation allow/deny, headless confirmation denial, `tool:before` hook veto projection.

## Non Scope

1. Permission policy, approval decision, checkpoint, marker, recovery transition을 새로 정의하지 않는다.
2. Queue depth, reconnect, slow-consumer, drop counter의 full accounting은 PRD 006이 소유한다.
3. TUI interaction layout and key binding은 PRD 005가 소유한다.

## SPEC Inputs

1. PRDs 000 and 001.
2. `crates/shacs-session/src/lib.rs`, `crates/shacs-session/src/diagnostics.rs`, `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-core/tests/runtime_agent.rs`.
3. Parent Spec 035 `Invariants` 3-5, `Must Have` 3 and 6, `Acceptance Criteria` 2 and 4.
4. Spec 029 owner records for durable recovery and channel restart state.
5. AgentLoop/session owner records for durable approval lineage, plus Spec 030 owner records for ephemeral hook denial and trusted runtime operational status.

## Dependency Cut

1. PRD 000 schema and PRD 001 adapter harness must exist.
2. Projection reads owner facts only. AgentLoop/session facts remain the durable approval truth; Spec 030 supplies only ephemeral hook denial and trusted runtime operational facts. This PRD cannot infer approval from confirmation text, infer recovery success from process liveness, or infer final delivery from emitted progress.
3. PRD 005 consumes the interactive actions defined here; PRD 006 extends progress accounting without changing terminal outcome semantics.

## Required Behavior

1. Durable approval projection은 owner가 제공한 경우에만 opaque action id 또는 digest lineage를 표면 간 보존한다.
2. Expiry and retry consumption remain distinct from denial.
3. Progress includes non-terminal status; only owner terminal records produce success, failure, cancellation, or final delivery.
4. Recovery projection links interrupted run, marker, checkpoint, requested action, and observed outcome without exposing raw paths or process handles.
5. Priority stop/restart commands preserve the existing active-turn semantics; projection does not report requested as completed.
6. Spec 030 confirmation은 현재 호출의 event로만 표시하며 approval expiry, retry consumption, remembered allow를 만들지 않는다.
7. Hook veto는 `hook_denied`이며 durable approval denial이 아니다. Tool call id는 correlation일 뿐 safety digest나 authorization proof가 아니다.

## Scenario Matrix

| Scenario | Required projection result |
|---|---|
| approval pending then allow | same lineage, ordered states, one terminal decision |
| approval expiry | `expired`, not denied or skipped |
| denied then retry attempted | denial lineage plus explicit retry state; no silent allow |
| interrupted run | interrupted/recovery-needed state linked to checkpoint or marker evidence |
| restart requested | requested/pending until owner records terminal result |
| progress followed by failure | progress remains non-terminal; failure wins final outcome |
| pending follow-up | visible as pending and not active/completed |
| interactive confirmation allow/deny | `ephemeral_confirmation`; no durable approval receipt |
| headless confirmation required | `headless_confirmation_denied`; never auto-allow |
| `tool:before` veto | `hook_denied`; not approval denied |

## Verification

1. Add deterministic lifecycle fixture tests for all matrix rows.
2. Compare CLI, API, WebSocket/channel, and TUI fixture projections where each surface is supported.
3. Test stale lineage, missing checkpoint, duplicate terminal event, repeated stop/recover, and cancellation.
4. Use fake clocks or explicit completion signals for expiry and timing; sleep-only proof is forbidden.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_approval
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core runtime_loop
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-session
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-api
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

## Agent-Executed Surface QA

1. Run an approval-required CLI fixture and capture pending plus terminal projection with the same opaque lineage.
2. Run `runtime inspect`, `runtime stop` or `runtime restart`, and `runtime recover` against an isolated interrupted fixture; record requested versus completed states.
3. Query corresponding API diagnostics/session projections.
4. Capture a WebSocket progress sequence followed by terminal outcome and verify ordering and vocabulary.

## Closure Evidence

1. Lifecycle scenario matrix: `.omo/evidence/spec031/prd002/lifecycle-parity.json`.
2. Approval lineage audit: `.omo/evidence/spec031/prd002/approval-lineage-audit.md`.
3. Recovery marker/checkpoint audit: `.omo/evidence/spec031/prd002/recovery-projection-audit.md`.
4. Surface transcripts and cleanup receipt: `.omo/evidence/spec031/prd002/qa/manual-cli-transcript.md` and `.omo/evidence/spec031/prd002/qa/cleanup-receipt.md`.

## Exit Criteria

1. Approval states preserve lineage and bounded vocabulary across supported surfaces.
2. Progress never becomes terminal outcome by presentation alone.
3. Recovery states are linked to actual owner evidence.
4. Deterministic tests and real-surface QA pass.
5. Confirmation과 hook denial이 durable approval lineage로 승격되지 않는다.
