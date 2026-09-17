# PRD 005. interactive TUI, REPL, and onboard flows

Status: Open

구현 상태: 구현됨, closure 차단. 실제 TUI/REPL과 secret-ref-only wizard 및 표면 QA 기록이 존재한다. Wizard는 environment reference만 입력받고 완료 시 실제 local Spec030 credential/disclosure 관측과 Spec031 config/profile 선언을 소비한다. 선언은 credential 존재·해석 성공·readiness·권한 부여가 아니며, 미관측 runtime credential은 unavailable로 남는다. Resolved/denied mapper fixture는 live credential·승인 QA가 아니다. 이전 기본 설정 재작성 사고는 미해결이며 [차단 사유와 증거 구분](../CLOSURE.md)을 따른다.

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
2. Raw credential handoff가 필요한 경우에는 auth owner 경계를 따라야 하며 projection, fixture, diagnostics, wizard persistence에는 저장하면 안 된다. 현재 wizard는 secret-ref-only이므로 masked raw credential 입력을 구현한 것으로 주장하지 않는다. Local auth persistence는 Spec 030이 소유한다.
3. REPL 또는 TUI가 CLI/API와 다른 command, permission, recovery contract를 만들지 않는다.

## SPEC Inputs

1. PRDs 000 through 004.
2. `crates/shacs-tui/src/lib.rs`, `crates/shacs-tui/src/main.rs`, `crates/shacs-tui/src/remembered_permissions.rs`.
3. Existing command routing and onboard baseline in `crates/shacs-cli/src/lib.rs`.
4. Spec 030 credential status and raw-data disclosure contract, plus Spec 031 config/profile auth-source declaration owner contract.
5. Parent Spec 035 `Must Have` 3-6, `Acceptance Criteria` 2, 4, 6, and 7, `Closure Evidence` 3-4.

## Dependency Cut

1. TUI and REPL consume PRD 000 projection and existing shared command semantics.
2. Onboard consumes Spec 030 credential status/disclosure facts and Spec 031 config/profile auth-source facts; it does not define config schema or migration, become a secret store, promise central redaction, or become an app/plugin lifecycle owner.
3. Recorded release fixtures may drive deterministic tests, but final QA must also consume a live runtime projection source.
4. `runtime.surface_approval` is only the Spec 035 surface IPC transport for TUI/REPL approval button decisions. Its durable work terminal means the runtime owner applied, rejected, superseded, or failed that transport request; it is not permission allow/deny truth. Approval truth remains the `AgentLoop`/session owner facts for the approval lineage. The request `target_owner_id` is an internal owner-generation fence and must not be displayed or documented as a user-facing owner identity.
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
2. Credential source와 local auth 상태를 표시한다. 현재 wizard는 environment reference만 입력받으며 raw credential은 받지 않는다. Raw credential의 입력·저장은 별도 auth owner 경계이며 wizard의 구현 기능으로 주장하지 않는다.
3. Show channel/app/plugin readiness and missing requirements using PRD 003 states.
4. Support cancel and restart without claiming partial configuration is complete.

## Verification

1. State-machine tests cover every flow and invalid transition.
2. Command parity tests feed identical commands to CLI and REPL routers and compare normalized outcomes.
3. TUI tests consume recorded owner projections, then manual QA drives the compiled TUI against a live isolated workspace.
4. Wizard 검사는 secret-ref-only 입력과 raw secret 거절, 취소·재개·상태 표시를 검증한다. Auth owner의 credential 저장 정책은 Spec030에서 검증하며 wizard가 재정의하지 않는다.
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
3. Onboard wizard QA는 새로 확보한 사용자 소유 data directory의 명시적 `--config`와 별도 `--workspace`로 정상·취소 흐름을 확인한다. Workspace만 바꾸면 config/auth는 격리되지 않으며 실제 사용자 config/auth를 읽거나 복사하지 않는다. 생성된 검증용 config만 기존 값 보존·raw secret 부재를 확인한다.
4. Save terminal transcripts or screenshots, normalized projection artifacts, exit codes, and cleanup receipts.

## Closure Evidence

The following historical artifact names are retained for auditability, but every listed file is **Unavailable** in this checkout and therefore does not prove current closure.

1. **Unavailable** - TUI flow and comparison index `.omo/evidence/spec031/prd005/tui/phase2/current/index.md` is absent; rerun terminal QA before using it as evidence.
2. **Unavailable** - REPL command parity matrix `.omo/evidence/spec031/prd005/repl/command-parity.json` is absent; regenerate it before asserting command parity.
3. **Unavailable** - onboard config and secret audit manifest `.omo/evidence/spec031/prd005/onboard/manifest.json` is absent; rerun onboard QA before using it as evidence.
4. **Unavailable** - live-versus-recorded source audit `.omo/evidence/spec031/prd005/runtime-source-audit.md` is absent; regenerate it before asserting runtime-source coverage.

## Exit Criteria

1. TUI exposes all required interactive states and actions from runtime projection.
2. REPL preserves CLI command semantics and priority behavior.
3. Wizard는 credential source reference와 상태 projection으로 readiness를 안내하며 raw secret을 입력받거나 보존하지 않는다.
4. Invalid, cancelled, and interrupted flows are evidenced.
5. Focused gates and terminal QA pass.
