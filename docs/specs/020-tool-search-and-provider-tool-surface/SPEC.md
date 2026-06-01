# tool search and provider tool surface 아키텍처 명세

Status: Draft. 이 문서는 Hermes-style Tool Search를 구현하기 전에 provider-visible tool surface, deferred tool catalog, bridge tool scope 계약을 고정한다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`, `docs/specs/011-subagent-runtime/SPEC.md`, `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`, `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`를 바탕으로 Tool Search 기능의 owner boundary를 정의한다.

목표는 다음과 같다.

- 많은 MCP tool schema를 매 provider 호출마다 전부 노출하지 않고, 필요할 때 검색, 설명, 호출하는 progressive disclosure 계약을 고정한다.
- provider-specific beta 기능에 의존하지 않는 provider-agnostic bridge surface를 우선한다.
- `ToolRegistry`와 `RuntimeToolExecutor`의 기존 실행/검증 경계를 우회하지 않도록 bridge dispatch scope를 정의한다.
- core tool은 항상 직접 노출하고, 초기 deferrable 범위는 MCP wrapper tool로 제한한다.
- subagent, MCP default-deny, diagnostics, release gate에서 Tool Search가 권한 확장 경로가 되지 않게 한다.

이 문서는 generic memory search, session search, code search, vector database, marketplace tool discovery를 소유하지 않는다. 범위는 provider에 노출되는 tool schema 표면과 deferred tool bridge semantics에 한정한다.

---

## 상위 기준과의 관계

- 003은 `ProviderRequest { messages, tools, model, settings, tool_choice }`와 provider adapter shaping을 소유한다. 020은 provider adapter가 받을 canonical `tools` 목록을 어떻게 조립할지 소유한다.
- 004는 `ToolRegistry`, `RuntimeToolExecutor`, `RuntimeToolCall`, `ToolResult`, `RuntimeToolMessage` 실행 경계를 소유한다. 020은 bridge call을 실제 underlying tool call로 해석하는 표면 계약만 소유하고, 실제 실행은 004 경계를 소비한다.
- 008은 config discovery, profile, runtime layout, JSON/camelCase `tools.*` 관례를 소유한다. 020은 `tools.toolSearch` key의 의미, 기본값, activation semantics를 소유하고 008의 config layout 관례를 소비한다.
- 009는 context assembly와 provider input의 현재 분산 경계를 소유한다. 020은 그중 provider-visible tool surface selection만 분리해 formal owner로 둔다.
- 010은 host safety, permission, secret, redaction을 소유한다. 020은 Tool Search가 permission이나 side-effect 권한을 넓힐 수 없다는 요구를 소비한다.
- 011은 subagent tool registry 제한을 소유한다. 020은 child runner의 deferred catalog가 child에게 부여된 tool surface를 넘지 않아야 한다고 요구한다.
- 014는 diagnostics와 inspect surface를 소유한다. 020은 Tool Search activation, deferred count, bridge unwrap observability에 필요한 evidence만 요구한다.
- 018은 memory/session search evidence, trajectory/replay, evaluation ledger를 소유한다. 020의 tool catalog search는 agent tool schema disclosure를 위한 별도 검색이며 018의 memory evidence를 대체하지 않는다. bridge와 underlying tool mapping은 018의 replay/ledger evidence가 destructive tool replay 없이 해석할 수 있어야 한다.

따라서 이 문서는 Anthropic 전용 Tool Search beta, 원격 tool marketplace, 조직 관리자 승인 workflow, public plugin ranking service를 다루지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- provider-visible tool surface assembly
- deferred tool catalog의 입력, rebuild, scope 규칙
- `tool_search`, `tool_describe`, `tool_call` bridge tool 의미
- Tool Search activation mode와 threshold semantics
- core tool과 deferrable tool 분류 기준
- bridge call unwrap과 underlying tool execution scope
- observability, diagnostics, verification 요구사항

이 문서는 다음을 정의하지 않는다.

- 개별 MCP server 구현
- memory/session/code semantic retrieval
- embedding provider, vector store, ANN index 제품 선택
- provider-native Tool Search beta header 또는 provider-specific defer flag
- permission policy engine 전체 재설계
- tool marketplace, 원격 설치, 팀 단위 tool catalog governance

---

## 핵심 정의

### provider-visible tool surface

provider-visible tool surface는 한 provider 호출에서 모델이 직접 볼 수 있는 tool schema 목록이다. 현재 구현에서는 `ToolRegistry::definitions()` 전체가 이 표면이지만, Tool Search가 활성화되면 visible core tools와 bridge tools만 포함하고 일부 MCP schema는 deferred catalog 뒤로 숨긴다.

provider-visible tool surface는 session truth가 아니다. 이것은 현재 provider call을 위해 조립된 입력 표면이며, tool 실행 가능 권한 자체를 새로 부여하지 않는다.

### deferrable tool

deferrable tool은 provider-visible tool surface에서 직접 schema를 제거하고 deferred catalog로 이동할 수 있는 tool이다. 1차 구현의 deferrable tool은 현재 session registry에 등록된 `mcp_` prefix wrapper tool에 한정한다.

core/builtin tool은 deferrable이 아니다. 현재 registry에 등록된 core/builtin tool 중 최소한 `read_file`, `write_file`, `edit_file`, `list_dir`, `glob`, `grep`, `exec`, `web_search`, `web_fetch`, `ask_user`, `message`, `spawn`, `my`, `image_generate` 같은 built-in tool은 Tool Search activation과 무관하게 직접 노출해야 한다. 등록 조건 자체는 각 owner spec, config, permission gate가 결정하며, Tool Search는 등록되지 않은 tool을 새로 노출하지 않는다.

### deferred tool catalog

deferred tool catalog는 현재 provider iteration에서 deferrable로 분류된 tool schema를 검색 가능한 entry로 바꾼 임시 catalog다. catalog entry는 최소한 tool name, description, parameter names, full schema, source kind, scope digest를 가진다.

catalog는 durable session state가 아니다. 매 provider-visible tool surface assembly마다 현재 `ToolRegistry` definitions와 current runner scope에서 다시 만든다. session-keyed map, persistent index, stale cache는 1차 구현 범위가 아니다.

### bridge tools

Tool Search가 활성화되면 deferrable tool schema 대신 아래 세 bridge tool을 provider-visible surface에 넣는다.

```text
tool_search(query, limit?)
tool_describe(name)
tool_call(name, arguments)
```

`tool_search`는 deferred catalog에서 후보를 반환한다. `tool_describe`는 선택한 deferred tool의 full schema를 tool result로 제공한다. `tool_call`은 deferred tool을 실행하기 위한 bridge이지만, 실행은 반드시 underlying real tool name으로 unwrap되어 004의 tool runtime 경계를 통과해야 한다.

---

## 핵심 계약

### Provider-agnostic first

1차 구현은 provider-native Tool Search 기능을 요구하지 않는다. OpenAI-compatible, Anthropic, Codex, Azure adapter는 `ProviderRequest.tools`에 들어온 canonical tool list를 기존처럼 provider wire format으로 변환한다.

provider-native Tool Search를 나중에 추가하더라도 020의 provider-agnostic bridge semantics를 깨면 안 된다. provider-native path는 동일한 scope, diagnostics, fallback contract를 만족할 때 optional optimization으로만 사용할 수 있다.

### Core tools never defer

core/builtin tool은 Tool Search activation과 무관하게 직접 schema를 노출한다. Tool Search가 core tool을 숨기면 recovery, file inspection, user clarification, message delivery, safety-related execution path가 모델에서 사라질 수 있다.

unknown tool이나 분류할 수 없는 tool은 안전하게 visible로 남긴다. 분류 실패가 silent drop으로 이어지면 안 된다.

### MCP default-deny scope preservation

MCP `enabledTools` default-deny는 Tool Search에도 그대로 적용되어야 한다. catalog는 현재 registry에 실제로 등록된 MCP wrapper tool에서만 만들어야 하며, disabled capability나 등록 실패 capability는 search, describe, call 어느 경로에서도 나타나면 안 된다.

subagent에서는 child registry에 포함된 tool만 catalog에 들어간다. parent가 가진 tool을 child bridge가 검색하거나 호출할 수 있으면 권한 확장이다.

### Bridge unwrap must use existing execution boundary

`tool_call`은 registry 전체를 직접 실행하는 backdoor가 아니다. bridge dispatch는 current deferred set에 포함된 name만 허용하고, 그 뒤 `RuntimeToolCall`을 underlying tool name과 arguments로 해석해야 한다.

parameter casting, schema validation, tool execution, error normalization, ask-user interrupt, concurrency/exclusive batching, checkpoint, `ToolEvent`는 004의 기존 경계를 사용한다.

provider에 돌려줄 tool message는 provider가 발급한 bridge call id와 상관관계를 유지해야 한다. 그러나 observability와 `tools_used`는 가능한 한 underlying tool name을 보여야 한다. trajectory, replay, evaluation ledger도 bridge call id와 underlying tool name mapping을 남겨야 하며, replay는 기록된 destructive underlying call을 실제로 재실행하면 안 된다.

### Activation and fallback

Tool Search 설정은 최소한 다음 값을 가진다.

```text
enabled: auto | on | off
thresholdPct: 0..100
searchDefaultLimit: 1..maxSearchLimit
maxSearchLimit: 1..50
```

기본값은 `enabled=auto`, `thresholdPct=10`, `searchDefaultLimit=5`, `maxSearchLimit=20`이다. `auto`는 deferrable schema token 추정치가 active context window의 threshold 이상일 때 활성화한다. context window를 알 수 없으면 `auto`는 pass-through로 처리하고, `enabled=on`만 deferrable tool 존재 여부로 활성화한다.

surface assembly 실패는 전체 tool schema pass-through로 fail-open할 수 있다. permission, scope, validation 실패는 fail-closed여야 한다.

---

## 검색 계약

1차 검색은 embedding/vector DB를 요구하지 않는다. catalog search는 tool name, description, top-level parameter names를 대상으로 하는 deterministic keyword ranking이면 충분하다.

권장 baseline은 BM25와 name substring fallback이다. BM25가 positive hit를 내지 못하더라도 query가 tool name substring과 일치하면 후보를 반환해야 한다. 이는 `github_*`처럼 공통 prefix가 많은 MCP catalog에서 zero-IDF 또는 query mismatch로 전체 검색이 비는 상황을 줄이기 위한 장치다.

검색 결과는 full schema를 한 번에 많이 반환하면 안 된다. `tool_search`는 name, short description, source, score 또는 rank 정도만 반환하고, full parameters는 `tool_describe`가 반환한다.

---

## 정상 시퀀스

1. runner가 provider 호출 직전에 현재 registry definitions를 가져온다.
2. Tool Surface Assembler가 core visible set과 deferrable set을 분리한다.
3. 설정과 threshold를 평가한다.
4. 비활성 상태이면 기존 definitions를 그대로 provider request에 넣는다.
5. 활성 상태이면 visible core tools와 bridge tools만 provider request에 넣고, current deferred catalog scope를 runner iteration에 보관한다.
6. 모델이 `tool_search`를 호출하면 current catalog를 검색하고 후보 목록을 tool result로 반환한다.
7. 모델이 `tool_describe`를 호출하면 current catalog 안의 해당 tool schema를 반환한다.
8. 모델이 `tool_call`을 호출하면 bridge가 current deferred set membership을 확인한다.
9. membership이 유효하면 underlying tool name으로 unwrap하여 기존 tool runtime 경계에서 실행한다.
10. runner는 tool result를 provider에 돌려주고 다음 iteration을 수행한다.

---

## 실패 시퀀스

1. deferrable catalog가 비어 있으면 Tool Search는 비활성으로 처리한다.
2. threshold 미만이면 pass-through로 처리한다.
3. `tool_search` query가 비어 있으면 provider-visible error result를 반환한다.
4. `tool_describe` name이 current deferred catalog에 없으면 provider-visible error result를 반환한다.
5. `tool_call` name이 bridge tool이거나 core tool이면 직접 호출하라는 error result를 반환한다.
6. `tool_call` name이 current deferred set 밖이면 scope violation error를 반환하고 실행하지 않는다.
7. underlying tool parameter validation이 실패하면 기존 `ToolRegistry` validation error를 반환한다.
8. underlying tool이 ask-user interrupt를 만들면 기존 interrupt 경계로 올라가야 한다.
9. bridge 내부 오류가 scope 판단을 불가능하게 만들면 실행은 fail-closed한다.

---

## PRD 분할

1. `prds/000-tool-search-config-and-runtime-plumbing.md`: `tools.toolSearch` config, 기본값과 clamp, boolean shorthand 유지 여부, runtime propagation, context window input.
2. `prds/001-pure-surface-assembler-and-deferred-catalog.md`: pure surface assembler, visible/deferred split, deferred catalog entry, activation 판단, deterministic search.
3. `prds/002-bridge-dispatcher-and-executor-unwrap.md`: `tool_search`, `tool_describe`, `tool_call` dispatch, current catalog scope, underlying `RuntimeToolCall` unwrap.
4. `prds/003-agent-runner-provider-request-live-wiring.md`: `AgentRunner` provider request 연결, assembled `ProviderRequest.tools`, per-iteration catalog scope, bridge routing.
5. `prds/004-mcp-default-deny-and-subagent-scope-regression.md`: MCP `enabledTools` default-deny, raw/wrapped allow-list, disabled capability 부재, child registry-only catalog.
6. `prds/005-diagnostics-trajectory-replay-and-release-evidence-integration.md`: activation diagnostics, bridge-to-underlying evidence, `tools_used`, replay safety, release gate evidence.

---

## 완료 기준

- Tool Search config가 default-safe 형태로 파싱되고 runtime config에 전달된다.
- provider-visible tool surface assembly가 `off`, `auto`, `on` mode와 threshold를 검증한다.
- core tools는 Tool Search activation과 무관하게 visible로 남는다.
- enabled MCP wrapper tools만 deferred catalog에 들어간다.
- `tool_search`, `tool_describe`, `tool_call` bridge tools가 current deferred scope만 대상으로 동작한다.
- `tool_call`은 underlying tool execution을 기존 registry validation과 runtime executor 경계로 수행한다.
- subagent child registry에서 out-of-scope parent tool이 검색 또는 호출되지 않는다.
- diagnostics와 tool events가 activation, deferred count, underlying tool name을 추적할 수 있다.
- docs와 tests가 provider-native beta 지원을 구현 완료처럼 과장하지 않는다.
