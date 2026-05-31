# PRD 002. bridge dispatcher and executor unwrap

## 목표

이 문서는 Tool Search의 세 번째 구현 PRD다.

목표는 `tool_search`, `tool_describe`, `tool_call` bridge call을 current catalog 안에서만 처리하고, valid `tool_call`을 underlying `RuntimeToolCall`로 해석하는 것이다.

bridge는 새로운 tool execution engine이 아니다.

실제 validation, permission, concurrency, interrupt 처리는 기존 `ToolRegistry`와 `RuntimeToolExecutor` 경계를 사용해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. 선행 PRD는 `prds/001-pure-surface-assembler-and-deferred-catalog.md`다.
3. tool runtime 계약은 `docs/specs/004-tool-runtime/SPEC.md`를 따른다.
4. safety 계약은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 따른다.
5. 구현 경계는 `crates/shacs-core/src/tools/registry.rs`와 `crates/shacs-core/src/runtime/tool_execution.rs`다.

## Dependency Cut

1. PRD 001은 current iteration catalog와 bridge schemas를 제공한다.
2. `crates/shacs-core/src/tools/registry.rs`는 `ToolRegistry::prepare_call()`과 `execute()` 경계를 소유한다.
3. `crates/shacs-core/src/runtime/tool_execution.rs`는 `RuntimeToolCall`, `RuntimeToolMessage`, `RuntimeToolExecutor`를 소유한다.
4. bridge dispatcher는 current catalog membership을 확인한 뒤 underlying call로 unwrap한다.
5. dispatcher는 registry validation을 복제하지 않는다.
6. dispatcher는 provider adapter별 beta semantics를 요구하지 않는다.

## 범위

1. `tool_search` argument parsing과 result shape.
2. `tool_describe` exact name lookup.
3. `tool_call` name과 arguments parsing.
4. current catalog membership enforcement.
5. recursive bridge call rejection.
6. core tool과 out-of-scope tool rejection.
7. underlying `RuntimeToolCall` 생성.
8. existing executor path로의 전달.
9. ask-user interrupt와 concurrency semantics 보존.

## 범위 제외

1. provider request에 bridge schemas를 넣는 live wiring.
2. assembler activation 판단 변경.
3. MCP wrapper registration 정책 변경.
4. subagent registry construction 변경.
5. 새로운 approval UI.
6. raw full schema diagnostics 추가.
7. destructive replay execution.

## 구현 요구사항

1. bridge dispatcher는 current runner iteration의 catalog를 필수 입력으로 받아야 한다.
2. current catalog가 없으면 bridge execution은 fail-closed error를 반환한다.
3. `tool_search`는 non-empty `query`를 요구한다.
4. `tool_search.limit`은 normalized default와 max limit을 따라야 한다.
5. `tool_search` result는 name, short description, source, rank 또는 score만 포함한다.
6. `tool_describe`는 exact tool name만 받는다.
7. `tool_describe`는 current catalog entry의 full schema를 provider-visible tool result로 반환한다.
8. `tool_call`은 `name`과 `arguments`를 받아야 한다.
9. `arguments`가 JSON string이면 object로 parse할 수 있다.
10. `arguments` parse 실패나 non-object 값은 실행 전에 error result가 되어야 한다.
11. `tool_call`은 `tool_search`, `tool_describe`, `tool_call`을 호출할 수 없다.
12. `tool_call`은 core 또는 visible tool을 호출할 수 없다.
13. `tool_call`은 current deferred catalog에 없는 name을 호출할 수 없다.
14. valid call은 underlying name과 arguments로 `RuntimeToolCall`을 만들어야 한다.
15. underlying validation은 `ToolRegistry::prepare_call()` 또는 기존 equivalent path를 사용한다.
16. execution은 `RuntimeToolExecutor`를 통해 수행한다.
17. ask-user interrupt는 bridge layer에서 삼키지 않고 기존 interrupt 경계로 올린다.
18. concurrency grouping은 bridge name이 아니라 underlying tool metadata를 따라야 한다.

## 데이터/상태 모델

1. `BridgeToolCall`은 original call id, bridge name, bridge arguments를 담는다.
2. `ResolvedDeferredToolCall`은 original call id, underlying name, underlying arguments, scope digest를 담는다.
3. `BridgeToolResult`는 provider가 기대하는 call id correlation을 유지한다.
4. `ToolCallScopeError`는 missing catalog, unknown name, recursive bridge, direct-call-required, invalid arguments를 구분한다.
5. `ToolSearchMatch`는 PRD 001의 search result shape를 그대로 사용한다.
6. `ToolDescribeResult`는 selected name과 full parameter schema를 담는다.
7. dispatcher state는 durable session state가 아니다.

## 정상 시퀀스

1. 모델이 `tool_search`를 호출한다.
2. dispatcher가 current catalog를 검색한다.
3. dispatcher가 bounded matches를 bridge tool result로 반환한다.
4. 모델이 match 중 하나를 `tool_describe`로 요청한다.
5. dispatcher가 current catalog entry의 full schema를 반환한다.
6. 모델이 `tool_call`로 name과 arguments를 전달한다.
7. dispatcher가 arguments를 object로 정규화한다.
8. dispatcher가 current catalog membership을 확인한다.
9. dispatcher가 underlying `RuntimeToolCall`을 만든다.
10. `RuntimeToolExecutor`가 기존 validation과 execution을 수행한다.
11. runner가 provider call id correlation을 유지해 tool message를 추가한다.

## 실패 시퀀스

1. catalog가 없으면 bridge call은 실행 scope를 알 수 없어 실패한다.
2. `tool_search.query`가 비어 있으면 검색하지 않는다.
3. `tool_describe.name`이 catalog에 없으면 schema를 반환하지 않는다.
4. `tool_call.name`이 catalog에 없으면 실행하지 않는다.
5. `tool_call.name`이 bridge name이면 recursive bridge error를 반환한다.
6. `tool_call.name`이 visible core tool이면 직접 호출하라는 error를 반환한다.
7. `tool_call.arguments`가 invalid JSON이면 실행하지 않는다.
8. registry validation 실패는 기존 tool error path로 반환한다.
9. permission denial이나 side-effect denial은 기존 runtime semantics를 유지한다.
10. scope digest mismatch가 감지되면 stale catalog로 보고 실행하지 않는다.

## 검증 관점

1. current catalog 밖의 name이 search, describe, call에서 거부되는지 확인한다.
2. recursive bridge call이 거부되는지 확인한다.
3. core tool bridge call이 거부되는지 확인한다.
4. JSON string arguments와 object arguments가 모두 처리되는지 확인한다.
5. invalid arguments가 underlying executor에 도달하지 않는지 확인한다.
6. underlying validation error가 기존 형태를 유지하는지 확인한다.
7. ask-user interrupt가 기존 interrupt 경계로 전달되는지 확인한다.
8. concurrent execution 판단이 underlying metadata를 사용하는지 확인한다.

## 완료 기준

1. bridge dispatcher가 current catalog만 대상으로 동작한다.
2. valid `tool_call`은 underlying `RuntimeToolCall`로 unwrap된다.
3. 실행은 기존 `ToolRegistry`와 `RuntimeToolExecutor` 경계를 통과한다.
4. recursive, core, out-of-scope call은 실행 전에 거부된다.
5. provider-native beta 없이 provider-agnostic bridge execution이 가능하다.
