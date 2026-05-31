# PRD 004. mcp default deny and subagent scope regression

## 목표

이 문서는 Tool Search의 다섯 번째 구현 PRD다.

목표는 Tool Search가 MCP default-deny와 subagent registry scope를 약화하지 않는지 regression으로 고정하는 것이다.

Tool Search는 discovery 기능이지만 권한 부여 기능이 아니다.

disabled MCP capability, parent-only tool, child registry 밖 tool은 search, describe, call 어느 경로에서도 보이면 안 된다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. 선행 PRD는 `prds/001-pure-surface-assembler-and-deferred-catalog.md`, `prds/002-bridge-dispatcher-and-executor-unwrap.md`, `prds/003-agent-runner-provider-request-live-wiring.md`다.
3. MCP boundary는 `docs/specs/004-tool-runtime/SPEC.md`와 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 따른다.
4. subagent boundary는 `docs/specs/011-subagent-runtime/SPEC.md`를 따른다.
5. 구현 경계는 `crates/shacs-core/src/tools/mcp.rs`와 `crates/shacs-core/src/runtime/subagent.rs`다.

## Dependency Cut

1. `crates/shacs-core/src/tools/mcp.rs`는 MCP wrappers와 `register_mcp_capabilities` default-deny filtering을 소유한다.
2. `crates/shacs-core/src/runtime/subagent.rs`는 child registry construction을 소유한다.
3. Tool Search catalog는 registry에 이미 등록된 definitions만 입력으로 받는다.
4. Tool Search는 disabled raw MCP capability를 직접 조회하지 않는다.
5. Tool Search는 parent runner registry를 child runner catalog에 섞지 않는다.
6. 이 PRD는 policy를 새로 만들지 않고 regression coverage를 고정한다.

## 범위

1. MCP `enabledTools` default-deny 보존 검증.
2. raw capability name allow-list 검증.
3. wrapped `mcp_` name allow-list 검증.
4. disabled capability의 search, describe, call 부재 검증.
5. child registry-only catalog 검증.
6. parent-only tool exclusion 검증.
7. bridge call scope violation error 검증.

## 범위 제외

1. MCP server protocol 재구현.
2. dynamic MCP capability reload.
3. user consent UI 변경.
4. subagent spawn policy 변경.
5. parent와 child 간 tool delegation 기능.
6. team admin approval workflow.
7. marketplace tool install flow.

## 구현 요구사항

1. `enabledTools` 기본값은 empty allow-list인 default-deny로 유지해야 한다.
2. `enabledTools`가 비어 있으면 해당 MCP capability는 registry에 등록되지 않아야 한다.
3. allow-list는 raw capability name과 wrapped `mcp_<server>_<kind>_<name>` 형태를 모두 검증해야 한다.
4. `*` allow-list behavior가 기존에 있다면 Tool Search catalog도 registry 결과만 따라야 한다.
5. disabled capability는 `ToolRegistry::definitions()`에 없어야 한다.
6. disabled capability는 deferred catalog에도 없어야 한다.
7. disabled capability name을 `tool_describe`로 요청해도 schema가 반환되면 안 된다.
8. disabled capability name을 `tool_call`로 요청해도 underlying execution이 시작되면 안 된다.
9. subagent child runner는 `subagent.rs`가 만든 child registry definitions만 assembler에 넘겨야 한다.
10. child catalog는 parent-only tool을 검색 결과로 반환하면 안 된다.
11. child `tool_call`은 parent-only tool name을 current catalog 밖으로 보고 fail-closed해야 한다.
12. parent runner catalog와 child runner catalog의 scope digest는 서로 독립적으로 계산한다.
13. tests는 raw MCP name과 wrapped MCP name을 모두 포함해야 한다.

## 데이터/상태 모델

1. `McpCapabilityAllowList`는 기존 MCP config에서 온 raw/wrapped allow-list다.
2. `RegisteredMcpTool`은 default-deny filtering을 통과한 wrapper tool만 나타낸다.
3. `ChildToolRegistryScope`는 subagent에게 실제 부여된 registry definitions다.
4. `DeferredToolCatalog.scope_digest`는 current registry scope를 요약한다.
5. `ToolCallScopeError::OutOfScope`는 disabled 또는 parent-only tool call에 쓰일 수 있다.
6. diagnostics에는 disabled capability의 raw schema를 포함하지 않는다.

## 정상 시퀀스

1. 사용자가 MCP server를 설정하지만 `enabledTools`를 비워 둔다.
2. MCP registration이 capability를 registry에 등록하지 않는다.
3. assembler가 registry definitions를 입력으로 받는다.
4. deferred catalog에는 disabled capability가 없다.
5. 사용자가 특정 raw capability를 allow-list에 넣는다.
6. MCP registration이 대응 wrapper tool을 registry에 등록한다.
7. assembler가 wrapper name이 `mcp_`로 시작하면 deferred candidate로 분류한다.
8. subagent runner가 child registry를 만든다.
9. child runner assembler가 child registry definitions만 사용한다.
10. child bridge search는 child에게 허용된 MCP tool만 반환한다.

## 실패 시퀀스

1. disabled MCP name을 search query로 넣어도 match가 없어야 한다.
2. disabled MCP name을 describe하면 unknown 또는 out-of-scope error를 반환한다.
3. disabled MCP name을 call하면 execution을 시작하지 않는다.
4. parent-only MCP name을 child bridge에서 search해도 match가 없어야 한다.
5. parent-only MCP name을 child bridge에서 describe하면 schema를 반환하지 않는다.
6. parent-only MCP name을 child bridge에서 call하면 out-of-scope error를 반환한다.
7. allow-list parsing 실패가 Tool Search catalog에서 capability를 되살리면 안 된다.
8. scope digest mismatch가 있으면 stale catalog로 보고 실행하지 않는다.

## 검증 관점

1. empty `enabledTools`에서 MCP tool이 catalog에 없는지 확인한다.
2. raw allow-list name이 등록과 deferred catalog로 이어지는지 확인한다.
3. wrapped allow-list name이 등록과 deferred catalog로 이어지는지 확인한다.
4. disabled capability가 search result에 없는지 확인한다.
5. disabled capability describe와 call이 실패하는지 확인한다.
6. child catalog가 parent-only tool을 포함하지 않는지 확인한다.
7. child bridge call이 parent-only underlying execution을 시작하지 않는지 확인한다.

## 완료 기준

1. MCP default-deny가 Tool Search activation 뒤에도 유지된다.
2. raw/wrapped allow-list behavior가 regression test로 고정된다.
3. disabled capability는 search, describe, call에서 모두 부재한다.
4. subagent catalog는 child registry-only scope를 사용한다.
5. parent-only tool은 child bridge로 호출될 수 없다.
