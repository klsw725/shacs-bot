# PRD 000: plugin manifest discovery and config gates

## 목표

User-extensible plugin의 첫 구현 웨이브는 실행보다 discovery와 opt-in gate를 먼저 고정한다. Runtime이 plugin directory를 발견하더라도 사용자가 enable하지 않은 extension은 load/execution surface에 들어가면 안 된다.

## 범위

- plugin root discovery
- manifest schema와 digest
- `not_enabled`, `enabled`, `disabled`, `blocked` state
- user-data root와 workspace-local root 구분
- workspace trust gate
- config merge와 diagnostics projection

## 비범위

- plugin tool 실행
- hook dispatch
- remote install/update
- public marketplace
- dynamic library/WASM/scripting runtime

## 구현 요구사항

1. Runtime은 user-data plugin root와 workspace-local plugin root를 분리해 scan해야 한다.
2. Workspace-local plugin은 explicit trust gate 없이는 executable surface로 load되지 않아야 한다.
3. Manifest parse 실패는 plugin `blocked` state로 남고 runtime startup을 실패시키지 않아야 한다.
4. `plugins.enabled`와 `plugins.disabled`가 동시에 같은 name을 포함하면 disabled가 우선해야 한다.
5. 새로 발견된 plugin은 기본 `not_enabled`다.
6. Manifest digest, source root, state, blocked reason이 inspect 가능해야 한다.
7. `requires_env`는 secret value가 아니라 required ref metadata로만 저장되어야 한다.
8. Missing env/config ref는 `blocked` 또는 `not_ready` diagnostic으로 남고 raw secret을 요구하지 않아야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`다.
2. config/root layout은 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 소비한다.
3. permission/trust/redaction 기준은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`와 `docs/specs/022-auto-approval-permissions/SPEC.md`를 소비한다.
4. diagnostics projection은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.
5. 이 PRD는 004 tool runtime, 005 skill system, hook dispatch를 구현하지 않는다.

## Dependency Cut

1. 이 PRD는 plugin discovery와 activation state만 소유한다.
2. Executable surface는 후속 PRD 001-003이 별도 gate를 통과한 뒤에만 열린다.
3. Workspace-local plugin trust는 discovery 결과의 state를 바꿀 수 있지만 permission을 부여하지 않는다.
4. Broken manifest는 runtime startup failure가 아니라 plugin-level blocked diagnostic이다.
5. public marketplace, remote install/update, dynamic ABI는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| plugin config와 root discovery | `crates/shacs-config/src/lib.rs`, `crates/shacs-core/src/runtime/` | config omission/default root regression |
| manifest parse와 digest | `crates/shacs-core/src/runtime/plugin.rs` 또는 신규 `crates/shacs-plugins` | valid/invalid manifest fixtures |
| activation state merge | `crates/shacs-core/src/runtime/plugin.rs` | disabled wins over enabled |
| diagnostics projection | `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs` | blocked reason redaction snapshot |

## 데이터/상태 모델

1. `PluginManifest`: `name`, `version`, `description`, `surfaces`, `requires_env`, `permissions`, `entrypoints`, `assets`, `digest`를 가진다.
2. `PluginSourceRoot`: user-data root와 workspace-local root를 구분하고 canonical path를 보존한다.
3. `PluginActivationState`: `not_enabled`, `enabled`, `disabled`, `blocked`를 가진다. `disabled`는 항상 `enabled`보다 우선한다.
4. `PluginBlockedReason`: parse error, unsafe path, untrusted workspace, missing ref, unsupported manifest version을 구분한다.
5. `PluginDiscoverySnapshot`: active runtime state가 아니라 현재 scan 결과의 read model이다.

## 정상 시퀀스

1. 사용자가 user-data plugin root에 manifest를 둔다.
2. runtime이 root를 scan하고 manifest를 parse한다.
3. config에 enable이 없으면 plugin은 `not_enabled`로 표시된다.
4. 사용자가 enable config를 추가한 뒤 다음 session/reload에서 `enabled` 후보가 된다.
5. diagnostics는 manifest digest, source root, surfaces, missing refs 없음 상태를 보여준다.

## 실패 시퀀스

1. manifest parse가 실패하면 해당 plugin은 `blocked`가 된다.
2. workspace-local plugin이 trust gate를 통과하지 못하면 executable surface가 열리지 않는다.
3. 같은 plugin이 enabled와 disabled에 모두 있으면 disabled가 우선한다.
4. missing env/config ref는 raw secret 요구가 아니라 missing ref diagnostic으로 남는다.
5. 어떤 실패도 tool, hook, skill, command registration으로 이어지면 안 된다.

## 검증 관점

1. 첫 failing test는 valid manifest가 `not_enabled` discovery snapshot으로 보이는지 확인한다.
2. invalid manifest, unsafe symlink, missing env ref, disabled-over-enabled case를 fixture로 둔다.
3. CLI/API inspect snapshot은 raw secret 없이 blocked reason을 보여야 한다.
4. TDD 순서는 parse/state merge test를 먼저 통과시키고 runtime registration은 후속 PRD에서 열어야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml plugin_manifest`
4. CLI projection을 건드렸다면 `cargo test --manifest-path crates/shacs-cli/Cargo.toml plugin`

## 완료 기준

- Discovery는 load/execution과 분리된다.
- Config gate가 없는 third-party plugin은 tool/hook/command surface에 나타나지 않는다.
- Broken manifest가 전체 runtime을 깨지 않는다.
- Diagnostics는 plugin state와 blocked reason을 redaction-safe하게 보여준다.
