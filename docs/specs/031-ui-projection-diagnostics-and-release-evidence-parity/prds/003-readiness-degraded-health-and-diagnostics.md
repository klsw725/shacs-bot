# PRD 003. readiness, degraded health, and diagnostics parity

Status: Planned

## Goal

Process liveness와 분리된 bounded readiness model을 만들고 provider auth, storage, containment, channel worker, plugin/app readiness, queue health를 CLI diagnostics와 local API에서 같은 severity와 redacted reason으로 표시한다.

## Scope

1. Component readiness aggregation and overall readiness rules.
2. `ready`, `degraded`, `blocked`, `unavailable`, `unknown` state and severity mapping.
3. CLI status/diagnostics, API readiness/diagnostics, diagnostics bundle parity.
4. Safe remediation hints and stale observation handling.

## Non Scope

1. Provider authentication, storage repair, containment enforcement, channel supervision, plugin/app lifecycle 자체를 구현하지 않는다.
2. Process alive를 ready로 승격하지 않는다.
3. Hosted monitoring, fleet health, administrator dashboard를 추가하지 않는다.

## SPEC Inputs

1. PRDs 000 and 001.
2. `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs`, `crates/shacs-session/src/diagnostics.rs`.
3. `crates/shacs-utils/src/diagnostics.rs`, `crates/shacs-utils/src/diagnostics_sanitizer.rs`.
4. Spec 030 owner records for redaction and containment evidence.
5. Parent Spec 031 `Invariants` 6-7, `Must Have` 5 and 8, `Acceptance Criteria` 6.

## Dependency Cut

1. Component owners supply observations; this PRD owns only projection and aggregation. Spec 030 remains the redaction and containment evidence owner.
2. Missing external-owner observations remain `unknown` or `unavailable` and block final closure where the capability is required.
3. PRD 004 adds context/extension/media component details; PRD 005 consumes readiness in interactive flows.

## Readiness Contract

Every component observation must contain component kind, bounded state, severity, safe reason code, observed-at/freshness, and optional safe remediation action.

Aggregation rules:

1. Any required component `blocked` makes overall readiness `blocked`.
2. A usable runtime with a limited component is `degraded`, never `ready`.
3. Missing required observation is `unknown` or `unavailable`, never ready.
4. Optional unavailable integrations may leave the runtime usable but must remain visible.
5. Stale evidence cannot silently retain ready status.

Required component families:

| Component | Owner evidence consumed |
|---|---|
| provider auth | configured provider/auth readiness without raw credential |
| storage | event/checkpoint/migration/space admission evidence |
| containment | current containment evidence and non-guarantee |
| channel worker | configured, running, skipped, failed, restart/delivery hint |
| plugin/app | discovery, enabled state, dependency/lifecycle readiness when owner exists |
| queue | bounded depth/capacity, admission block, stale work summary |

## Failure Rules

1. Raw token, env value, absolute path, owner id, PID, provider payload, or queue payload is forbidden.
2. `/health` success alone cannot prove component readiness.
3. Diagnostics bundle and API must not disagree on component severity for the same observation set.
4. A remediation hint cannot claim an action already succeeded.

## Verification

1. Table-test all aggregation combinations and stale observations.
2. Test absent credentials, blocked migration, unknown containment, failed channel, disabled plugin, unavailable app owner, and queue admission block.
3. Compare CLI, API, and bundle canonical fields from one fixture.
4. Audit redaction and non-guarantee wording.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-projection
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-session diagnostics
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-api diagnostics
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli runtime
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-api --all-targets -- -D warnings
```

## Agent-Executed Surface QA

1. Create one ready, one degraded, and one blocked isolated workspace fixture.
2. Run CLI status and diagnostics, request API readiness/diagnostics, and create a diagnostics bundle for each.
3. Read the artifacts and compare component state, severity, reason code, freshness, and remediation hint.
4. Verify cleanup and absence of raw secrets/paths/process handles.

## Closure Evidence

1. Aggregation matrix: `.omo/evidence/spec031/prd003/readiness-aggregation.json`.
2. Cross-surface comparison: `.omo/evidence/spec031/prd003/readiness-parity.json`.
3. Diagnostics bundle read audit: `.omo/evidence/spec031/prd003/diagnostics-bundle-audit.md`.
4. QA transcripts and cleanup receipts under `.omo/evidence/spec031/prd003/qa/`.

## Exit Criteria

1. Required component families are independently visible.
2. Ready/degraded/blocked/unavailable/unknown aggregation is deterministic.
3. CLI, API, and bundle preserve canonical severity and redacted reason.
4. Focused gates and real-surface QA pass.
