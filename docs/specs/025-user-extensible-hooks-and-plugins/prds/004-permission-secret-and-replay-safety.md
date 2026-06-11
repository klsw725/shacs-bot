# PRD 004: permission secret and replay safety

## 목표

Plugin/hook extension이 local self-hosted runtime의 permission, secret, replay safety를 약화하지 않도록 공통 gate를 고정한다.

## 범위

- plugin permission ceiling
- secret ref metadata and env injection allow-list
- protected target handling
- project-local plugin trust
- diagnostics redaction
- replay live-dispatch prohibition

## 비범위

- full sandbox implementation
- signed plugin ecosystem
- organization compliance workflow

## 구현 요구사항

1. Plugin은 permission ceiling을 낮출 수는 있어도 높일 수 없다.
2. Secret은 manifest에 raw value로 저장되지 않아야 한다.
3. Command-backed handler env는 explicit allow-list/ref만 포함해야 한다.
4. Project-local plugin은 untrusted workspace에서 executable surface로 load되면 안 된다.
5. Diagnostics는 args/result/env를 redaction pass 후 저장해야 한다.
6. Replay는 plugin tool/hook side effect를 live-dispatch하지 않고 recorded evidence를 사용해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`다.
2. host safety와 secret handling은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 소비한다.
3. approval and inherited ceilings는 `docs/specs/022-auto-approval-permissions/SPEC.md`를 소비한다.
4. diagnostics/redaction은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.
5. replay/evidence는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`를 소비한다.

## Dependency Cut

1. 이 PRD는 모든 executable plugin surface가 소비해야 할 common safety gate다.
2. Permission request와 permission grant는 반드시 분리한다.
3. Secret refs는 metadata이며 raw value 저장소가 아니다.
4. Replay는 live side effect를 만들지 않는다.
5. full sandbox, signed plugin ecosystem, organization compliance workflow는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| permission ceiling | `crates/shacs-config/src/permissions.rs`, `crates/shacs-core/src/runtime/plugin.rs` | plugin cannot raise ceiling |
| secret ref allow-list | `crates/shacs-config/src/lib.rs`, `crates/shacs-core/src/runtime/plugin.rs` | raw secret not persisted |
| diagnostics redaction | `crates/shacs-core/src/runtime/diagnostics.rs` 또는 existing diagnostics module | args/env redacted snapshot |
| replay no-live-dispatch | `crates/shacs-core/src/runtime/` | replay uses recorded evidence |

## 데이터/상태 모델

1. `PluginPermissionCeiling`: requested capabilities와 effective inherited ceiling을 구분한다.
2. `PluginSecretRef`: env/config key reference와 required/optional metadata만 가진다.
3. `PluginExecutionEnv`: explicit allow-list로 구성된 env projection이며 raw config dump가 아니다.
4. `PluginSafetyDiagnostic`: denied permission, missing secret ref, untrusted workspace, replay suppressed를 구분한다.
5. `PluginReplayRecord`: live command가 아니라 recorded digest/result/error/evidence를 가진다.

## 정상 시퀀스

1. enabled plugin tool이 required env ref를 선언한다.
2. runtime이 allow-list에 포함된 ref만 execution env로 materialize한다.
3. permission request는 inherited ceiling 안에서만 평가된다.
4. execution result와 env/args summary는 redaction pass 후 diagnostics에 남는다.
5. replay는 recorded evidence를 표시하고 handler를 다시 실행하지 않는다.

## 실패 시퀀스

1. plugin이 inherited ceiling보다 높은 permission을 요구하면 blocked/denied diagnostic이 된다.
2. missing secret ref는 raw secret prompt가 아니라 missing ref state가 된다.
3. untrusted workspace-local plugin은 executable surface가 blocked된다.
4. diagnostics redaction 실패 시 raw evidence 저장을 중단한다.
5. replay가 live-dispatch를 시도하면 fail-closed한다.

## 검증 관점

1. 첫 failing test는 plugin이 permission ceiling을 높일 수 없음을 확인한다.
2. raw secret이 manifest, diagnostics, replay record에 남지 않는지 fixture로 확인한다.
3. untrusted workspace plugin executable surface blocked regression을 둔다.
4. replay no-live-dispatch regression은 destructive handler fixture로 검증한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml plugin_safety`
4. config를 건드렸다면 `cargo test --manifest-path crates/shacs-config/Cargo.toml permission`

## 완료 기준

- Secret leakage regression과 replay destructive dispatch regression이 존재한다.
- Plugin safety failure는 blocked diagnostic으로 보이고 runtime crash가 아니다.
