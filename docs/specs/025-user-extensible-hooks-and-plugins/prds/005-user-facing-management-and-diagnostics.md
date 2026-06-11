# PRD 005: user-facing management and diagnostics

## 목표

사용자가 local CLI/TUI/API에서 plugin과 hook 상태를 이해하고 opt-in/out할 수 있는 관리 표면과 release evidence gate를 고정한다.

## 범위

- plugin list/inspect/enable/disable/doctor projection
- hook list/inspect projection
- blocked/missing-env/last-error summary
- loaded tool/skill/command surface summary
- release evidence checklist

## 비범위

- visual layout
- hosted dashboard
- remote install/update UX

## 구현 요구사항

1. `plugins list`는 enabled, disabled, not enabled, blocked를 구분해야 한다.
2. `plugins inspect`는 source root, manifest digest, surfaces, missing refs, blocked reason을 보여야 한다.
3. `plugins enable/disable`은 config만 바꾸고 running session의 system prompt/toolset을 조용히 mutate하지 않아야 한다. 필요한 경우 다음 session부터 적용하거나 명시 reload command가 있어야 한다.
4. `hooks inspect`는 event subscription, last dispatch, last error, timeout count를 보여야 한다.
5. Release gate는 discovery, hook dispatch, plugin tool, plugin skill/command, safety/replay, UI projection evidence bucket을 모두 요구해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`다.
2. UI/command rendering은 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 소비한다.
3. config mutation은 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 소비한다.
4. diagnostics bundle은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.
5. release evidence는 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`를 소비한다.

## Dependency Cut

1. PRD 000 discovery state가 있어야 list/inspect가 의미를 가진다.
2. PRD 001-004가 구현되기 전에도 management surface는 partial 상태를 표시해야 한다.
3. enable/disable은 config state transition이며 running prompt/toolset silent mutation이 아니다.
4. Rendering 세부는 013이 소유하고 이 PRD는 projection data와 action semantics만 소유한다.
5. hosted dashboard, remote install/update UX는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| plugin list/inspect commands | `crates/shacs-cli/src/lib.rs` | blocked/not-enabled/enabled projection snapshot |
| config enable/disable mutation | `crates/shacs-config/src/lib.rs`, CLI config writer path | disabled wins and next-session semantics |
| hooks inspect | `crates/shacs-core/src/runtime/plugin.rs`, `crates/shacs-cli/src/lib.rs` | last error/timeout count snapshot |
| diagnostics bundle | `crates/shacs-core/src/runtime/diagnostics.rs` 또는 existing diagnostics path | redaction-valid extension evidence |

## 데이터/상태 모델

1. `PluginListItem`: name, version, source root, state, surface summary, blocked reason digest를 가진다.
2. `PluginInspectView`: manifest digest, required refs, permission request, provided surfaces, last diagnostics를 가진다.
3. `HookInspectView`: event kind, source plugin, enabled state, dispatch count, last error, timeout count를 가진다.
4. `PluginManagementAction`: enable, disable, doctor, reload-needed를 구분한다.
5. `PluginReleaseEvidence`: discovery, hook, tool, skill/command, safety/replay, projection evidence bucket을 가진다.

## 정상 시퀀스

1. 사용자가 `plugins list`를 실행한다.
2. CLI는 discovery snapshot과 config state를 읽어 enabled/disabled/not-enabled/blocked를 표시한다.
3. 사용자가 `plugins enable <name>`을 실행하면 config만 갱신하고 reload/next-session 안내를 표시한다.
4. 다음 session 또는 explicit reload 후 enabled plugin surfaces가 각 PRD gate를 통과해 반영된다.
5. `plugins doctor`는 missing refs, invalid manifest, unsafe path, hook/tool readiness를 요약한다.

## 실패 시퀀스

1. unknown plugin enable 요청은 config를 오염시키지 않고 not-found error를 반환한다.
2. blocked plugin enable 요청은 blocked reason과 필요한 조치를 보여준다.
3. running session toolset/prompt를 silent mutate하려는 경로는 거부한다.
4. diagnostics redaction 실패 시 raw plugin args/env를 표시하지 않는다.
5. release gate evidence가 부족하면 supported/closed로 표시하지 않는다.

## 검증 관점

1. 첫 failing test는 `plugins list`가 네 state를 구분하는지 확인한다.
2. enable/disable config mutation이 next-session semantics를 유지하는지 확인한다.
3. `hooks inspect`는 last error와 timeout count를 표시해야 한다.
4. release evidence checklist가 모든 bucket 없이는 pass하지 않는지 확인한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-cli/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-cli/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-cli/Cargo.toml plugin`
4. diagnostics를 건드렸다면 `cargo test --manifest-path crates/shacs-core/Cargo.toml plugin`

## 완료 기준

- 사용자는 어떤 plugin/hook이 왜 load되지 않았는지 알 수 있다.
- Enable/disable은 prompt cache와 session consistency를 깨지 않는다.
- Release evidence는 redaction-valid owner `025` refs를 요구한다.
