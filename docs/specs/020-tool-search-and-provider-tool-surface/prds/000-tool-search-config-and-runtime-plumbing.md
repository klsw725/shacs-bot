# PRD 000. tool search config and runtime plumbing

## 목표

이 문서는 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`의 첫 구현 PRD다.

목표는 Tool Search를 켜거나 끄는 설정과 runtime 전달 경로를 먼저 고정하는 것이다.

이 단계는 provider-visible tool surface를 바꾸지 않는다.

assembler, catalog search, bridge execution은 후속 PRD가 소유한다.

구현자는 이 PRD만으로 `tools.toolSearch` 설정이 config에서 runner 입력까지 안정적으로 전달되는지 확인할 수 있어야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. config layout 기준은 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 따른다.
3. provider request 호출 위치는 `docs/specs/003-provider-runtime/SPEC.md`를 따른다.
4. context window 입력은 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 소비한다.
5. 구현 경계는 `crates/shacs-config/src/lib.rs`의 `ToolsConfig` 패턴과 `crates/shacs-core/src/runtime/runner.rs`의 runtime config 소비 경로다.

## Dependency Cut

1. 008은 config discovery, profile merge, camelCase JSON 관례를 제공한다.
2. 020은 `tools.toolSearch` key의 의미와 기본값을 소유한다.
3. 003은 provider 호출 직전 runner가 필요한 입력을 받는 경계를 제공한다.
4. 009는 활성 context window를 계산하거나 알 수 없다고 표시하는 입력 경계를 제공한다.
5. 이 PRD는 `ToolRegistry::definitions()` 결과를 읽지 않는다.
6. 이 PRD는 provider adapter wire format을 바꾸지 않는다.

## 범위

1. `tools.toolSearch` 설정 shape 정의.
2. `enabled`, `thresholdPct`, `searchDefaultLimit`, `maxSearchLimit` 기본값 정의.
3. config 값 clamp와 malformed value 처리 기준 정의.
4. boolean shorthand 유지 여부와 해석 기준 정의.
5. runtime config로의 전달 경로 정의.
6. provider iteration에서 사용할 context window 입력 연결 기준 정의.
7. diagnostics가 참조할 normalized config summary 정의.

## 범위 제외

1. provider-visible tool surface assembly.
2. visible tool과 deferred tool 분리.
3. deferred catalog 생성과 검색.
4. `tool_search`, `tool_describe`, `tool_call` bridge schema 주입.
5. bridge dispatch와 underlying tool 실행.
6. provider-native Tool Search beta header.
7. 조직 관리자 정책, fleet 설정, marketplace governance.

## 구현 요구사항

1. `crates/shacs-config/src/lib.rs`는 `ToolsConfig` 아래 `tool_search`에 해당하는 camelCase JSON key `toolSearch`를 받아야 한다.
2. config 구조는 최소한 `enabled`, `thresholdPct`, `searchDefaultLimit`, `maxSearchLimit`를 표현해야 한다.
3. 기본값은 `enabled=auto`, `thresholdPct=10`, `searchDefaultLimit=5`, `maxSearchLimit=20`이다.
4. `enabled`는 `off`, `on`, `auto`만 정상 문자열로 인정한다.
5. boolean shorthand를 유지한다면 `true`는 `auto`, `false`는 `off`로 해석한다.
6. boolean shorthand를 제거한다면 migration note 없이 silent semantic change가 생기지 않게 SPEC 또는 PRD에 명시해야 한다.
7. `thresholdPct`는 0 이상 100 이하로 clamp한다.
8. `maxSearchLimit`은 1 이상 50 이하로 clamp한다.
9. `searchDefaultLimit`은 1 이상 normalized `maxSearchLimit` 이하로 clamp한다.
10. malformed config는 process 시작 실패 대신 safe default와 warning diagnostics를 우선한다.
11. runtime config는 `crates/shacs-core/src/runtime/runner.rs`가 provider request를 만들기 전에 읽을 수 있어야 한다.
12. `auto` 판단을 위해 runner는 현재 provider input의 context window 값을 assembler에 넘길 준비만 해야 한다.
13. context window를 알 수 없는 경우를 표현할 수 있어야 한다.
14. 이 PRD 단계에서 `ProviderRequest.tools` 값은 기존과 같아야 한다.

## 데이터/상태 모델

1. `ToolSearchConfig`는 normalized runtime config다.
2. `ToolSearchMode`는 `Off`, `On`, `Auto`를 가진다.
3. `threshold_pct`는 normalized integer 또는 작은 unsigned type으로 표현한다.
4. `search_default_limit`은 bridge search default limit으로 전달될 값이다.
5. `max_search_limit`은 user supplied limit 상한이다.
6. `ToolSearchRuntimeInput`은 config와 context window option을 함께 담는다.
7. `context_window_tokens`는 알 수 없을 수 있다.
8. config diagnostics는 raw secret이나 provider credential을 포함하지 않는다.

## 정상 시퀀스

1. 사용자가 config에 `tools.toolSearch`를 생략한다.
2. config loader가 기본값을 적용한다.
3. runtime layout이 normalized config를 core runtime에 전달한다.
4. runner가 provider iteration을 시작한다.
5. runner가 현재 model 또는 context assembly에서 context window 입력을 얻는다.
6. context window를 알 수 없으면 `None`으로 전달할 준비를 한다.
7. runner가 후속 assembler에 넘길 `ToolSearchRuntimeInput`을 만들 수 있다.
8. 이 PRD 단계에서는 기존 `spec.tools.definitions()` 흐름이 그대로 유지된다.

## 실패 시퀀스

1. `enabled` 값이 알 수 없는 문자열이면 safe default `auto`를 사용하고 warning evidence를 남긴다.
2. 숫자 값이 범위를 벗어나면 clamp된 값을 사용한다.
3. `maxSearchLimit`이 0이면 1로 올린다.
4. `searchDefaultLimit`이 `maxSearchLimit`보다 크면 `maxSearchLimit`로 낮춘다.
5. context window를 알 수 없으면 runtime input에 unknown을 보존한다.
6. config normalization 실패가 permission failure처럼 처리되면 안 된다.
7. 설정 문제 때문에 tool execution 권한이 넓어지면 안 된다.

## 검증 관점

1. config 생략 시 기본값이 적용되는지 확인한다.
2. `off`, `on`, `auto` 문자열이 모두 파싱되는지 확인한다.
3. boolean shorthand를 유지한다면 `true`와 `false` 해석을 확인한다.
4. 각 숫자 field clamp를 확인한다.
5. malformed config가 safe default와 diagnostics로 이어지는지 확인한다.
6. runner가 context window unknown을 표현할 수 있는지 확인한다.
7. 이 PRD 적용 후 provider-visible tools가 바뀌지 않는지 확인한다.

## 완료 기준

1. `tools.toolSearch` config가 default-safe 형태로 파싱된다.
2. normalized config가 runtime runner 경계까지 전달된다.
3. context window input이 후속 assembler에 넘길 수 있는 형태로 준비된다.
4. `ProviderRequest.tools` behavior는 아직 기존과 동일하다.
5. 문서와 테스트가 provider-native beta를 구현 완료처럼 표현하지 않는다.

## 구현 상태

상태: 완료.

구현 증거:

1. `crates/shacs-config/src/lib.rs`에 `tools.toolSearch` camelCase config와 `ToolSearchConfig` / `ToolSearchMode` normalization을 추가했다.
2. `enabled` 문자열 `off` / `on` / `auto`와 boolean shorthand를 파싱하고, malformed `toolSearch` 또는 malformed field는 config load 실패 없이 safe default로 정규화한다.
3. `thresholdPct`, `searchDefaultLimit`, `maxSearchLimit` clamp를 config test로 고정했다.
4. `crates/shacs-core/src/runtime/runner.rs`의 `AgentRunSpec`은 Tool Search config와 optional `context_window_tokens`를 `ToolSearchRuntimeInput`으로 만들 수 있다.
5. `AgentLoopConfig`와 CLI `AgentLoopChatCompletionAdapter::loop_config()`가 normalized Tool Search config를 전달한다.
6. runner provider request는 기존 `tools: spec.tools.definitions()` 경로를 유지하며, focused core test가 Tool Search 설정을 켠 상태에서도 provider-visible tools pass-through를 확인한다.

검증 증거:

1. `cargo fmt --manifest-path crates/shacs-cli/Cargo.toml -- --check`
2. `cargo test --manifest-path crates/shacs-config/Cargo.toml tool_search`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml runtime_runner_executes_tool_loop_and_accumulates_usage`
4. `cargo test --manifest-path crates/shacs-cli/Cargo.toml agent_loop_adapter_loop_config_carries_tool_search_config`
5. `cargo check --manifest-path crates/shacs-core/Cargo.toml`
6. `cargo check --manifest-path crates/shacs-cli/Cargo.toml`
7. `cargo clippy --manifest-path crates/shacs-config/Cargo.toml --all-targets -- -D warnings`
8. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
9. `cargo clippy --manifest-path crates/shacs-cli/Cargo.toml --all-targets -- -D warnings`
