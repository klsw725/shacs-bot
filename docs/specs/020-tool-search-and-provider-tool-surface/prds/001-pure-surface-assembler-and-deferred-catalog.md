# PRD 001. pure surface assembler and deferred catalog

## 목표

이 문서는 Tool Search의 두 번째 구현 PRD다.

목표는 현재 registry definitions를 입력으로 받아 provider-visible tool list와 deferred catalog를 산출하는 pure assembler를 정의하는 것이다.

이 단계는 bridge tool schema와 catalog entry를 만들지만, runner에 live wiring하지 않는다.

`tool_search`, `tool_describe`, `tool_call` 실행도 이 PRD 범위가 아니다.

pure 함수로 먼저 분리해야 후속 runner 연결과 regression test가 provider side effect 없이 가능하다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. 선행 PRD는 `prds/000-tool-search-config-and-runtime-plumbing.md`다.
3. tool definition 입력은 `docs/specs/004-tool-runtime/SPEC.md`의 registry 경계를 따른다.
4. 구현 경계는 `crates/shacs-core/src/tools/registry.rs`의 `ToolRegistry::definitions()` 결과다.
5. provider request 연결은 후속 PRD 003이 소유한다.

## Dependency Cut

1. PRD 000은 normalized config와 context window input을 제공한다.
2. 004는 tool schema definition shape와 registry owner boundary를 제공한다.
3. 020은 visible과 deferred 분류 기준을 소유한다.
4. assembler는 `ToolRegistry` 실행 API를 호출하지 않는다.
5. assembler는 현재 definitions slice와 runtime input만 소비하는 pure 연산이어야 한다.
6. bridge name collision은 assembler가 판단하되 underlying tool을 삭제하지 않는다.

## 범위

1. pure surface assembler 함수 계약.
2. visible set과 deferred set 분리.
3. `mcp_` prefix 기반 deferrable rule.
4. bridge name collision pass-through.
5. schema token estimate 계산.
6. `off`, `on`, `auto` activation 판단.
7. deferred catalog entry 생성.
8. deterministic ranking과 search baseline.
9. bridge tool schema definition 생성.

## 범위 제외

1. `AgentRunner` provider request live wiring.
2. bridge dispatcher 실행.
3. underlying `RuntimeToolCall` unwrap.
4. MCP default-deny 구현 변경.
5. subagent child registry construction 변경.
6. persistent catalog cache와 vector index.
7. provider-native Tool Search beta integration.

## 구현 요구사항

1. assembler 입력은 definitions, normalized Tool Search config, context window option이어야 한다.
2. definitions는 `ToolRegistry::definitions()`가 돌려주는 현재 순서를 보존해야 한다.
3. `enabled=off`는 항상 pass-through 결과를 반환한다.
4. `enabled=on`은 deferrable tool이 하나 이상 있으면 활성화한다.
5. `enabled=auto`는 deferrable schema token estimate가 context window threshold 이상일 때 활성화한다.
6. `auto`에서 context window를 알 수 없으면 pass-through다.
7. deferrable tool의 1차 기준은 tool name이 `mcp_`로 시작하는 것이다.
8. core, builtin, unknown tool은 visible로 남긴다.
9. bridge tool names는 `tool_search`, `tool_describe`, `tool_call`로 예약한다.
10. current registry에 bridge name과 같은 tool이 있으면 Tool Search는 pass-through로 처리한다.
11. bridge name collision이 있어도 기존 tool definition을 삭제하거나 rename하면 안 된다.
12. token estimate는 serialized schema character count의 `ceil(chars / 4)`를 사용한다.
13. 활성화되면 provider-visible result는 visible tools와 bridge tool schemas를 포함한다.
14. 비활성화되면 original definitions를 그대로 반환한다.
15. `tool_search` result용 catalog search는 name, description, top-level parameter names를 대상으로 한다.
16. ranking은 deterministic keyword scorer를 사용하고, name substring fallback을 제공한다.
17. search result는 full schema를 포함하지 않는다.

## 데이터/상태 모델

1. `ToolSurfaceAssemblyInput`은 definitions, config, context window를 담는다.
2. `ToolSurfaceAssembly`는 provider tools, activation state, catalog option을 담는다.
3. `ActivationState`는 `PassThrough`, `Activated`, `CollisionPassThrough`, `ThresholdPassThrough`를 구분할 수 있어야 한다.
4. `DeferredToolCatalog`는 entries, scope digest, default limit, max limit을 가진다.
5. `DeferredToolCatalogEntry`는 name, description, parameter names, full schema, source kind, source name을 가진다.
6. `ToolSearchMatch`는 name, short description, source, rank 또는 score를 가진다.
7. `scope_digest`는 current catalog membership과 ordering을 요약한다.
8. raw full schema는 catalog 내부에는 보존되지만 diagnostics summary에는 그대로 쓰지 않는다.

## 정상 시퀀스

1. caller가 current `ToolRegistry::definitions()` 결과를 assembler에 넘긴다.
2. assembler가 bridge name collision을 검사한다.
3. collision이 없으면 visible과 deferrable을 분리한다.
4. assembler가 deferrable schema JSON 문자 수를 합산한다.
5. assembler가 `ceil(chars / 4)`로 token estimate를 계산한다.
6. assembler가 config mode와 context window로 activation을 결정한다.
7. 활성화되지 않으면 original definitions를 반환한다.
8. 활성화되면 deferrable entries로 catalog를 만든다.
9. assembler가 visible definitions 뒤에 bridge schemas를 붙인다.
10. caller는 result 안의 catalog를 후속 runner iteration scope로 전달할 수 있다.

## 실패 시퀀스

1. deferrable catalog가 비어 있으면 pass-through다.
2. bridge name collision이 있으면 pass-through다.
3. schema serialization이 특정 tool에서 실패하면 해당 tool은 visible로 남기거나 전체 pass-through로 안전하게 처리한다.
4. context window가 unknown이면 `auto`는 pass-through다.
5. malformed description이나 parameters가 있어도 tool이 silent drop되면 안 된다.
6. search query가 positive score를 만들지 못해도 name substring fallback을 시도한다.
7. catalog search 내부 오류는 bridge dispatcher에서 provider-visible error로 바꿀 수 있는 error family를 반환해야 한다.

## 검증 관점

1. pure assembler가 같은 입력에 같은 output을 내는지 확인한다.
2. `off`, `on`, `auto` activation을 각각 확인한다.
3. `ceil(chars / 4)` estimate가 threshold 판단에 쓰이는지 확인한다.
4. `mcp_` tool만 deferred로 이동하는지 확인한다.
5. core와 unknown tool이 visible로 남는지 확인한다.
6. bridge name collision에서 original definitions가 보존되는지 확인한다.
7. catalog entry가 full schema를 내부에 보관하는지 확인한다.
8. search result가 full schema를 노출하지 않는지 확인한다.
9. deterministic ranking과 substring fallback을 확인한다.

## 완료 기준

1. assembler가 `ToolRegistry::definitions()` 결과만으로 surface와 catalog를 만들 수 있다.
2. activation mode와 threshold behavior가 test로 고정된다.
3. bridge schemas와 deferred catalog shape가 후속 dispatcher에서 쓸 수 있게 정의된다.
4. 비활성 경로는 기존 definitions order를 보존한다.
5. provider-native beta는 optional future로만 남는다.
