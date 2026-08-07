# PRD 005. interactive TUI, REPL, and onboard flows

Status: Planned revision (implemented baseline)

## Goal

TUI, REPL, and onboard wizard surfaces consume actual runtime projection and the shared command contract. Mock-only 화면이나 surface별 command semantics는 closure 증거가 될 수 없다.

## Scope

1. Active session, pending durable approval, ephemeral confirmation, hook denial, progress, degraded health, recovery action을 표시하고 조작하는 interactive TUI.
2. CLI command router 또는 동등한 shared command contract를 사용하는 REPL.
3. Running turn 중 priority stop/restart semantics 보존.
4. Config stub, credential source/local auth entry status, channel/app/plugin readiness를 안내하는 onboard wizard.
5. Cancellation, validation error, interrupted flow, resume/recovery behavior.

## Non Scope

1. Theme polish, visual design system, layout framework, mobile UI를 closure 조건으로 삼지 않는다.
2. Wizard는 masked credential input을 auth owner에 전달할 수 있지만 projection, fixture, diagnostics, wizard persistence에는 저장하지 않는다. Handoff 이후 local auth persistence는 Spec 030이 소유한다.
3. REPL 또는 TUI가 CLI/API와 다른 command, permission, recovery contract를 만들지 않는다.

## SPEC Inputs

1. PRDs 000 through 004.
2. `crates/shacs-tui/src/lib.rs`, `crates/shacs-tui/src/main.rs`, `crates/shacs-tui/src/remembered_permissions.rs`.
3. Existing command routing and onboard baseline in `crates/shacs-cli/src/lib.rs`.
4. Spec 030 credential status and raw-data disclosure contract, plus Spec 035 config/profile auth-source declaration owner contract.
5. Parent Spec 031 `Must Have` 3-6, `Acceptance Criteria` 2, 4, 6, and 7, `Closure Evidence` 3-4.

## Dependency Cut

1. TUI and REPL consume PRD 000 projection and existing shared command semantics.
2. Onboard consumes Spec 030 credential status/disclosure facts and Spec 035 config/profile auth-source facts; it does not define config schema or migration, become a secret store, promise central redaction, or become an app/plugin lifecycle owner.
3. Recorded release fixtures may drive deterministic tests, but final QA must also consume a live runtime projection source.
4. `runtime.surface_approval` is only the Spec031 surface IPC transport for TUI/REPL approval button decisions. Its durable work terminal means the runtime owner applied, rejected, superseded, or failed that transport request; it is not permission allow/deny truth. Approval truth remains the `AgentLoop`/session owner facts for the approval lineage. The request `target_owner_id` is an internal owner-generation fence and must not be displayed or documented as a user-facing owner identity.
5. Spec 030 confirmation은 별도 ephemeral surface event다. Headless confirmation-required step은 auto-allow하지 않고 blocked/denied로 표시하며 `runtime.surface_approval` lineage에 저장하지 않는다. Hook veto도 approval denial로 변환하지 않는다.
6. `remembered_permissions.rs` 같은 기존 artifact가 남아 있어도 Spec 030 trusted-runtime closure나 confirmation surface의 필수 보장으로 재사용하지 않는다.

## Required Interactive Flows

### TUI

1. Select or inspect an active session.
2. Observe progress separately from final outcome.
3. Approve or deny a pending approval using its opaque lineage.
4. Observe degraded/blocked readiness with safe reason.
5. Request stop/restart/recover and display requested versus completed state.
6. Cancel/exit without corrupting the active runtime state.
7. Show trusted runtime profile, adapter-specific process control, sandbox scope/fallback, credential source/status, resource trusted-code disclosure, raw-content disclosure without inventing safety guarantees.

### REPL

1. Submit ordinary turns through the same session command boundary.
2. Route exact/prefix commands with CLI-equivalent parsing and validation.
3. Preserve priority stop/restart behavior during a running turn.
4. Display projection output using canonical vocabulary.
5. Handle EOF, cancellation, malformed command, and interrupted input deterministically.

### Onboard wizard

1. Generate or merge config stubs without overwriting existing values.
2. Display credential source and local auth entry status separately from masked credential input; hand raw input directly to the auth owner without retaining it in wizard state.
3. Show channel/app/plugin readiness and missing requirements using PRD 003 states.
4. Support cancel and restart without claiming partial configuration is complete.

## Verification

1. State-machine tests cover every flow and invalid transition.
2. Command parity tests feed identical commands to CLI and REPL routers and compare normalized outcomes.
3. TUI tests consume recorded owner projections, then manual QA drives the compiled TUI against a live isolated workspace.
4. Wizard tests prove masked credential input is not retained in projection, fixture, diagnostics, or wizard persistence after auth-owner handoff; auth owner storage policy is tested in Spec 030 rather than redefined here.
5. Confirmation and hook tests prove ephemeral allow/deny/veto never becomes durable approval or remembered permission.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-tui
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli repl
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli onboard
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-tui --all-targets -- -D warnings
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-cli --all-targets -- -D warnings
```

## Agent-Executed Surface QA

1. Launch the TUI in a terminal session, navigate active session/readiness views, exercise one approval, one recovery request, one cancellation, and one invalid action.
2. Launch the REPL, run help, an ordinary turn fixture, a priority command during an active turn, malformed input, and EOF.
3. Launch the onboard wizard in an isolated workspace, complete one valid flow, cancel one flow, and inspect resulting config for preservation and absence of raw secrets.
4. Save terminal transcripts or screenshots, normalized projection artifacts, exit codes, and cleanup receipts.

## Closure Evidence

1. TUI flow transcript and projection comparison index: `.omo/evidence/spec031/prd005/tui/phase2/current/index.md`.
2. REPL command parity matrix: `.omo/evidence/spec031/prd005/repl/command-parity.json`.
3. Onboard config diff and secret audit manifest: `.omo/evidence/spec031/prd005/onboard/manifest.json`.
4. Live-versus-recorded source audit: `.omo/evidence/spec031/prd005/runtime-source-audit.md`.

## Exit Criteria

1. TUI exposes all required interactive states and actions from runtime projection.
2. REPL preserves CLI command semantics and priority behavior.
3. Wizard guides readiness through credential sources and status-only projection; masked input is discarded after auth-owner handoff.
4. Invalid, cancelled, and interrupted flows are evidenced.
5. Focused gates and terminal QA pass.
