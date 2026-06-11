# PRD 006: user-facing configuration plugin tools and closure

## 목표

Tool Search의 남은 closure gap인 user-facing config/diagnostics evidence와 025 plugin-provided tool handoff boundary를 고정한다. 이 PRD는 기존 PRD 000-005를 대체하지 않고, Spec 020이 소유하는 마지막 사용자 관찰성과 closure evidence를 닫는다.

## 범위

- `tools.toolSearch` user-facing config documentation.
- activation mode, threshold, pass-through, bridge active status diagnostics evidence.
- deferred catalog count and source-kind summary.
- 025 plugin-provided tool의 deferrable candidate boundary handoff.
- closure evidence checklist.

## 비범위

- provider-native Tool Search beta.
- remote tool marketplace.
- plugin manifest/discovery 구현.
- vector search or semantic tool ranking.
- core tool deferral.

## 구현 요구사항

1. User-facing config 문서는 `enabled=auto|on|off`, `thresholdPct`, `searchDefaultLimit`, `maxSearchLimit`의 기본값과 clamp behavior를 설명해야 한다.
2. Diagnostics는 현재 turn 기준 Tool Search active/pass-through reason을 보여야 한다.
3. Deferred catalog summary는 source kind별 count를 보여야 하며, 현재 020 구현 범위에서는 MCP wrapper source kind를 안정적으로 구분해야 한다.
4. 025에서 구현될 enabled plugin-provided tool은 core tool이 아니며 기본 deferrable candidate로 분류되어야 한다는 boundary를 문서화해야 한다.
5. Disabled/blocked/not-enabled plugin의 tool exclusion과 exclusion reason diagnostics는 Spec 025의 plugin activation/tool registration 구현 증거로 닫는다.
6. Plugin tool이 bridge `tool_call`로 실행될 때도 004 runtime executor와 010/022 permission boundary를 통과해야 한다는 handoff condition을 남긴다.
7. Closure evidence는 MCP default-deny, child registry-only catalog, user-facing config/diagnostics evidence, bridge-to-underlying replay mapping을 함께 요구해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. PRD 000-005의 완료된 Tool Search foundation을 소비한다.
3. plugin-provided tool source와 activation state는 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`를 소비한다.
4. tool execution boundary는 `docs/specs/004-tool-runtime/SPEC.md`를 소비한다.
5. permission, diagnostics, replay boundary는 010/014/018/022를 소비한다.

## Dependency Cut

1. 이 PRD는 PRD 000-005를 수정하거나 재개장하지 않는다.
2. User-facing config/diagnostics evidence는 020 closure로 소유한다.
3. Plugin manifest/discovery/tool registration/classification/exclusion은 025가 소유한다.
4. Provider-native Tool Search beta, marketplace, vector search, core tool deferral은 비범위다.
5. Plugin tool은 core tool로 승격되지 않고 deferrable candidate로만 통합되어야 한다는 boundary는 020이 고정하고, 구현 증거는 025가 제공한다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| config diagnostics evidence | `crates/shacs-core/src/runtime/runner.rs`, `crates/shacs-core/src/runtime/tool_search.rs` | activation reason snapshot |
| deferred catalog source kind summary | `crates/shacs-core/src/tools/tool_search.rs` | MCP source-kind counts |
| plugin tool deferrable classification | `docs/specs/025-user-extensible-hooks-and-plugins/prds/002-command-backed-plugin-tools.md` | Spec 025 enabled plugin tool deferred |
| exclusion diagnostics | `docs/specs/025-user-extensible-hooks-and-plugins/prds/002-command-backed-plugin-tools.md` | Spec 025 disabled/blocked plugin exclusion reason |
| release evidence | `docs/specs/020-.../prds/007-sequential-implementation-plan.md` | PRD 000-006 closure matrix |

## 데이터/상태 모델

1. `ToolSearchDiagnosticsSummary`: mode, active/pass-through reason, visible count, deferred count, source kind count를 가진다.
2. `DeferredCatalogSourceSummary`: source kind별 count를 가진다.
3. `ToolSearchActivationReason`: `off`, `threshold`, `forced_on`, `no_deferrable_tools`, `bridge_collision`, `unknown_context_window` family를 구분한다.
4. `PluginToolSearchDecision`: enabled deferrable, disabled excluded, blocked excluded, not-enabled excluded를 구분하는 Spec 025-owned model이다.
5. `ToolSearchClosureEvidence`: config, assembler, bridge, runner, scope, diagnostics, user-facing config evidence refs를 가진다.

## 정상 시퀀스

1. runner가 provider request 직전에 Tool Search runtime input을 만든다.
2. assembler가 core visible set과 deferrable set을 계산한다.
3. diagnostics가 activation reason과 source kind count를 기록한다.
4. bridge call은 current deferred scope 안의 underlying tool로 unwrap된 뒤 기존 executor boundary를 통과한다.
5. future enabled plugin tool은 Spec 025 구현에서 source kind plugin으로 deferrable candidate가 되어 이 boundary를 소비한다.

## 실패 시퀀스

1. assembler error가 scope 판단을 불가능하게 만들면 execution은 fail-closed하고 schema surface는 pass-through 가능한 경우에만 pass-through한다.
2. replay는 underlying destructive tool을 live 재실행하지 않는다.
3. disabled/blocked/not-enabled plugin tool exclusion reason은 Spec 025 diagnostics에서 raw plugin args/secret 없이 기록해야 한다.
4. plugin tool이 approval을 만들거나 core tool처럼 분류되면 Spec 025 regression failure다.

## 검증 관점

1. activation reason diagnostics snapshot을 `off`, `forced_on`, `threshold`, `no_deferrable`, `bridge_collision`, `unknown_context_window` case로 고정한다.
2. source kind summary가 MCP wrapper source kind count를 안정적으로 구분하는지 확인한다.
3. PRD 006 release checklist가 PRD 000-005 evidence만으로 pass하지 않고 user-facing config evidence를 요구하는지 확인한다.
4. bridge-to-underlying replay mapping이 destructive tool live rerun 없이 유지되는지 확인한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml tool_search`
4. CLI/API projection을 별도 spec에서 건드렸다면 `cargo test --manifest-path crates/shacs-cli/Cargo.toml tool_search`

## 완료 기준

- 사용자는 Tool Search가 왜 켜졌거나 pass-through 되었는지 diagnostics evidence에서 알 수 있다.
- Spec 025가 구현할 plugin tool은 core tool이 아니라 Tool Search deferrable boundary를 소비해야 한다는 handoff가 문서화되어 있다.
- Release gate가 PRD 000-005 evidence와 user-facing config evidence를 함께 요구한다.
- 문서와 테스트는 provider-native beta, marketplace, core tool deferral을 구현 완료처럼 주장하지 않는다.

## 구현 상태

상태: Closed for Spec 020-owned scope. User-facing diagnostics에 필요한 activation/source-kind runtime evidence와 PRD 006 release checklist를 구현했고, plugin-provided tool manifest/registration/classification/exclusion 구현은 Spec 025 소유로 handoff했다.

구현 증거:

1. `crates/shacs-core/src/tools/tool_search.rs`의 `DeferredToolCatalog`가 deferred catalog entry의 `source_kind`별 count를 계산한다.
2. `crates/shacs-core/src/runtime/tool_search.rs`의 `ToolSearchDiagnosticsSummary`가 `deferred_source_counts`를 포함한다.
3. `crates/shacs-core/src/runtime/runner.rs`의 `tool_search_activation` event detail/result가 activation reason과 source-kind summary를 runtime diagnostics evidence로 관찰 가능하게 만든다.
4. `crates/shacs-core/src/runtime/diagnostics_release.rs`에 PRD 006용 release checklist를 추가해 `UserFacingConfig` bucket을 요구한다.

검증 증거:

1. `crates/shacs-core/tests/tools.rs`는 activation reason family와 `mcp_tool` source-kind count를 검증한다.
2. `crates/shacs-core/tests/runtime_agent.rs`는 runtime activation diagnostics event가 `deferred_source_counts`를 노출하는지 검증한다.
3. `crates/shacs-core/tests/runtime_loop.rs`는 PRD 006 release checklist가 PRD 000-005 evidence만으로 pass하지 않고 user-facing config evidence를 추가로 요구하는지 검증한다.

Spec 025 handoff:

1. 현재 `ToolRegistry`/`ToolDefinition`에는 plugin-provided tool source metadata가 없다. `crates/shacs-core/src/tools/base.rs`의 `ToolDefinition`은 name/description/parameters만 갖고, `crates/shacs-core/src/tools/registry.rs`의 `definitions()`도 schema만 반환한다.
2. 현재 deferrable classification은 `mcp_` prefix 기반이다. enabled/disabled/blocked plugin tool state가 registry에 없으므로 plugin exclusion의 실제 runtime 검증은 Spec 025에서 구현한다.
3. Spec 025 PRD 000(plugin manifest discovery/config gates)와 PRD 002(command-backed plugin tools)는 enabled plugin tool을 source kind `plugin_tool`로 catalog에 반영하고 disabled/blocked/not-enabled tool을 제외해야 한다.
4. CLI/local API inspect projection은 013/014 또는 별도 UI/API work에서 이 runtime evidence를 사용자-facing surface로 연결할 수 있지만 Spec 020 closure blocker는 아니다.
