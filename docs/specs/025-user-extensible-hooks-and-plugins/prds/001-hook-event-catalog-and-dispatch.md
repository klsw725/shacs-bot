# PRD 001: hook event catalog and dispatch

## 목표

Hook event catalog와 dispatch runtime을 고정한다. Hook은 기본적으로 관측자이며, 행동 변경은 명시적으로 허용된 event에서만 가능하다.

## 범위

- hook event enum/schema
- observer-only dispatch
- `tool:before`, `llm:before`, `command:before`, `channel:inbound`, transform hook의 제한된 output handling
- timeout/error isolation
- dispatch diagnostics

## 비범위

- plugin tool registration
- shell language choice
- gateway-specific formatting
- organization audit policy

## 구현 요구사항

1. Hook event payload는 bounded, redacted, versioned shape여야 한다.
2. Hook callback은 timeout을 가져야 한다.
3. Hook failure는 해당 hook diagnostic으로 남고 runtime turn을 crash시키면 안 된다.
4. `tool:before`는 block만 제안할 수 있고 allow/approval을 생성할 수 없다.
5. `llm:before` context injection은 system prompt를 mutate하지 않고 current user input side context로만 들어가야 한다.
6. Transform hook은 output size limit과 redaction pass를 통과해야 한다.
7. 여러 hook의 conflict는 deterministic order와 first-block-wins 같은 명시 규칙을 가져야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`다.
2. runtime lifecycle event는 `docs/specs/012-runtime-services/SPEC.md`를 소비한다.
3. tool event 경계는 `docs/specs/004-tool-runtime/SPEC.md`를 소비한다.
4. permission과 approval ceiling은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`와 `docs/specs/022-auto-approval-permissions/SPEC.md`를 소비한다.
5. diagnostics/redaction은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.

## Dependency Cut

1. PRD 000의 enabled plugin discovery가 선행되어야 한다.
2. Observer-only hook dispatch가 먼저 구현되고 behavior-affecting hook은 별도 gate 뒤에 열린다.
3. Hook은 session truth, permission truth, provider auth를 직접 소유하지 않는다.
4. Hook failure는 hook-level diagnostic이며 runtime crash가 아니다.
5. shell language choice와 gateway-specific formatting은 이 PRD 범위가 아니다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| hook event enum과 payload | `crates/shacs-core/src/runtime/plugin.rs` 또는 `runtime/hooks.rs` | event serialization snapshot |
| observer dispatch | `crates/shacs-core/src/runtime/runner.rs` | failing hook does not crash turn |
| behavior-affecting output validation | `crates/shacs-core/src/runtime/hooks.rs` | `tool:before` block cannot approve |
| diagnostics projection | `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs` | last error and timeout count |

## 데이터/상태 모델

1. `HookEventKind`: observer-only event와 behavior-affecting event를 type/metadata로 구분한다.
2. `HookEventPayload`: version, event id, redacted input, source plugin, size limit metadata를 가진다.
3. `HookDispatchResult`: `observed`, `blocked`, `rewritten`, `injected_context`, `failed`, `timed_out`, `ignored_invalid_output`을 구분한다.
4. `HookSubscription`: plugin name, event kind, entrypoint, timeout, enabled state를 가진다.
5. `HookDiagnostics`: dispatch count, last success, last error, timeout count, invalid output count를 저장한다.

## 정상 시퀀스

1. enabled plugin이 observer-only `tool:after` hook을 선언한다.
2. tool result 정규화 후 hook payload가 redacted shape로 만들어진다.
3. dispatcher가 timeout 안에서 callback을 실행한다.
4. callback output은 observer-only event이므로 runtime behavior를 바꾸지 않는다.
5. diagnostics는 dispatch success와 digest를 남긴다.

## 실패 시퀀스

1. hook callback이 timeout을 넘기면 해당 hook만 `timed_out`으로 기록된다.
2. invalid output은 ignored diagnostic으로 남고 runtime behavior를 바꾸지 않는다.
3. `tool:before` hook이 allow/approval을 반환해도 permission approval로 소비하지 않는다.
4. `llm:before` injection이 system prompt mutation을 요구하면 거부한다.
5. 여러 hook이 conflict하면 deterministic rule에 따라 block 또는 skip하고 evidence를 남긴다.

## 검증 관점

1. 첫 failing test는 observer hook panic/timeout이 turn failure로 번지지 않는지 확인한다.
2. `tool:before` block은 tool error로 표현되지만 approval을 생성하지 않는지 확인한다.
3. `llm:before` injection은 ephemeral user-side context로만 들어가는지 확인한다.
4. diagnostics snapshot은 last error와 timeout count를 redaction-safe하게 보여야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml hook`
4. projection을 건드렸다면 `cargo test --manifest-path crates/shacs-cli/Cargo.toml hook`

## 완료 기준

- Hook dispatch가 observer-only와 behavior-affecting hook을 구분한다.
- Misbehaving hook은 격리된다.
- Hook output은 owner boundary를 통과하지 않고 session truth를 변경하지 않는다.
