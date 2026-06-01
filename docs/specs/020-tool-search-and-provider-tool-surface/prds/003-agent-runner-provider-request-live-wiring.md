# PRD 003. agent runner provider request live wiring

상태: 완료

완료 근거:

1. `crates/shacs-core/src/runtime/runner.rs`는 provider iteration마다 `spec.tools.definitions()`를 다시 읽고 `assemble_tool_surface`와 `spec.tool_search_runtime_input()`으로 provider-visible tool surface를 조립한다.
2. 활성화된 iteration의 `ProviderRequest.tools`는 visible tools와 `tool_search`, `tool_describe`, `tool_call` bridge schemas만 포함하고, deferred `mcp_` schema는 provider request에 직접 노출하지 않는다.
3. 현재 iteration의 deferred catalog만 bridge dispatcher에 전달하며, 다음 provider iteration에서는 registry definitions로 catalog를 다시 만든다.
4. 활성 catalog가 있는 bridge calls는 `dispatch_bridge_tool_calls`로 라우팅하고, direct visible/core calls는 기존 `RuntimeToolExecutor` 경로로 실행한다.
5. bridge assistant history는 provider가 반환한 bridge tool call 그대로 보존하고, bridge result는 original bridge call id와 bridge name으로 상관시킨다. resolved `tool_call`의 `tools_used`는 가능한 경우 underlying tool name으로 기록한다.
6. provider adapter나 provider-native Tool Search beta 없이 canonical provider tool schema만으로 동작함을 runner test에서 고정했다.

검증:

1. `cargo test --manifest-path crates/shacs-core/Cargo.toml runtime_runner` 통과: runtime_agent runner 필터 20 passed.
2. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check` 통과.
3. `cargo check --manifest-path crates/shacs-core/Cargo.toml` 통과.
4. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings` 통과.
5. `cargo test --manifest-path crates/shacs-core/Cargo.toml` 통과: lib 16, app_environment 11, runtime 17, runtime_agent 47, runtime_loop 103, tools 72, doctest 0.

## 목표

이 문서는 Tool Search의 네 번째 구현 PRD다.

목표는 pure assembler와 bridge dispatcher를 `AgentRunner` provider iteration에 실제로 연결하는 것이다.

현재 `crates/shacs-core/src/runtime/runner.rs`는 provider request를 만들 때 `ProviderRequest { tools: spec.tools.definitions(), ... }` 흐름을 사용한다.

이 PRD는 그 값을 assembled provider tools로 바꾸고, 같은 iteration의 bridge calls가 같은 catalog scope를 보도록 runner state를 연결한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. 선행 PRD는 `prds/000-tool-search-config-and-runtime-plumbing.md`, `prds/001-pure-surface-assembler-and-deferred-catalog.md`, `prds/002-bridge-dispatcher-and-executor-unwrap.md`다.
3. provider runtime 계약은 `docs/specs/003-provider-runtime/SPEC.md`를 따른다.
4. context assembly 입력은 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 소비한다.
5. 구현 경계는 `crates/shacs-core/src/runtime/runner.rs`다.

## Dependency Cut

1. PRD 000은 runtime config와 context window input을 제공한다.
2. PRD 001은 assembled tools와 optional current catalog를 제공한다.
3. PRD 002는 bridge call을 current catalog로 dispatch하는 실행 경계를 제공한다.
4. 003은 `ProviderRequest.tools` canonical list를 provider adapter에 넘기는 계약을 제공한다.
5. provider adapter는 assembled tools를 기존 canonical tools처럼 wire format으로 바꾼다.
6. Anthropic native Tool Search beta나 provider-specific defer flag는 필수가 아니다.

## 범위

1. `AgentRunner` provider request assembly 변경.
2. `ProviderRequest.tools`에 assembled tool list 사용.
3. per-iteration deferred catalog scope 저장.
4. bridge tool call routing.
5. provider-agnostic behavior 보존.
6. activation diagnostics hook 위치 정의.
7. non-activated pass-through behavior 검증.

## 범위 제외

1. config parser 변경.
2. assembler classification rule 변경.
3. bridge dispatcher scope rule 변경.
4. provider-native beta 구현.
5. provider별 tool schema 최적화.
6. MCP default-deny policy 변경.
7. trajectory replay release evidence 통합.

## 구현 요구사항

1. runner는 provider request를 만들기 직전에 current registry definitions를 가져와 assembler에 넘겨야 한다.
2. 기존 `ProviderRequest { tools: spec.tools.definitions(), ... }` 직접 사용은 assembled result 사용으로 바뀌어야 한다.
3. `ProviderRequest.tools` type은 provider-agnostic canonical tool list로 유지한다.
4. assembled result가 pass-through이면 provider tools는 기존 definitions와 같아야 한다.
5. assembled result가 activated이면 provider tools는 visible tools와 bridge schemas만 포함해야 한다.
6. runner는 activated iteration의 deferred catalog를 해당 provider round의 tool execution scope에 보관해야 한다.
7. catalog scope는 다음 provider iteration에서 current registry definitions로 다시 만들어야 한다.
8. model이 bridge tool을 호출하면 runner는 normal registry lookup 전에 bridge dispatcher로 라우팅해야 한다.
9. bridge가 underlying call로 unwrap한 뒤에는 기존 runtime tool execution path를 사용해야 한다.
10. model이 visible core tool을 직접 호출하면 기존 execution path를 사용해야 한다.
11. provider adapter는 assembled `ProviderRequest.tools`만 보고 기존 방식으로 변환한다.
12. provider-native Tool Search beta가 없어도 전체 기능이 동작해야 한다.
13. activation, deferred count, scope digest는 후속 diagnostics가 읽을 수 있게 runner event context에 남겨야 한다.

## 데이터/상태 모델

1. `ProviderToolSurfaceForIteration`은 provider tools와 optional deferred catalog를 묶는다.
2. `CurrentToolSearchScope`는 catalog, activation reason, scope digest를 가진다.
3. `ToolDispatchDecision`은 bridge dispatch와 normal dispatch를 구분한다.
4. `ProviderRequest.tools`는 assembled canonical definitions list다.
5. runner iteration state는 provider call을 넘겨 durable catalog cache가 되면 안 된다.
6. bridge mapping evidence는 후속 PRD 005에서 trajectory와 diagnostics로 확장한다.

## 정상 시퀀스

1. runner가 user turn의 provider loop iteration을 시작한다.
2. runner가 current registry definitions를 읽는다.
3. runner가 Tool Search runtime input과 definitions를 assembler에 넘긴다.
4. assembler가 provider-visible tools와 optional catalog를 반환한다.
5. runner가 `ProviderRequest.tools`에 assembled tools를 넣는다.
6. provider adapter가 canonical tools를 기존 방식으로 wire format으로 변환한다.
7. 모델이 bridge tool call을 반환한다.
8. runner가 call name을 보고 bridge dispatcher로 라우팅한다.
9. dispatcher가 underlying call로 unwrap한다.
10. runner가 기존 `RuntimeToolExecutor` path를 실행한다.
11. 다음 provider iteration에서는 registry definitions를 다시 읽고 surface를 새로 조립한다.

## 실패 시퀀스

1. assembler가 pass-through를 반환하면 runner는 기존 tools list를 provider request에 넣는다.
2. activated catalog 없이 bridge call이 도착하면 fail-closed bridge error를 반환한다.
3. provider가 unknown bridge name을 반환하면 기존 unknown tool error path를 사용한다.
4. bridge dispatcher가 scope violation을 반환하면 underlying execution을 시작하지 않는다.
5. normal core tool call은 bridge catalog와 무관하게 기존 path로 실행한다.
6. provider adapter가 bridge schemas를 특별 취급해야 기능이 동작하는 구조가 되면 안 된다.
7. iteration 사이에 catalog를 재사용해 stale tool이 실행되면 안 된다.

## 검증 관점

1. Tool Search off에서 `ProviderRequest.tools`가 기존 definitions와 같은지 확인한다.
2. Tool Search activated에서 MCP schema가 provider request에서 빠지고 bridge schemas가 들어가는지 확인한다.
3. bridge tool call이 dispatcher로 라우팅되는지 확인한다.
4. visible core tool call이 기존 executor path로 가는지 확인한다.
5. 다음 iteration에서 catalog가 다시 만들어지는지 확인한다.
6. provider adapter별 native beta 없이 같은 canonical tools가 전달되는지 확인한다.
7. activation summary가 diagnostics hook에 전달될 수 있는지 확인한다.

## 완료 기준

1. runner가 `spec.tools.definitions()` 직접 surface를 provider request에 넣지 않는다.
2. `ProviderRequest.tools`는 assembled tool list를 사용한다.
3. bridge dispatch와 normal dispatch가 runner에서 분리된다.
4. catalog scope는 provider iteration 단위로만 유효하다.
5. OpenAI-compatible, Anthropic, Codex, Azure adapter가 provider-native beta 없이 canonical tool list를 소비할 수 있다.
