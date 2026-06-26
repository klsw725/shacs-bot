# user-extensible hooks and plugins 아키텍처 명세

Status: Foundation plus initial diagnostics-only hook runtime slice complete; full Spec 25 remains open. Hermes의 hooks/plugins 제품 의미론을 `shacs-bot`의 Rust self-hosted runtime에 맞게 재해석해 owner boundary를 고정했고, 현재 구현은 `plugin.json`/`plugin.toml` manifest discovery, descriptor-only projection, safety diagnostics, management CLI, enabled plugin hook의 diagnostics-only runtime dispatch까지 닫았다. Live tool/command/MCP/skill execution과 behavior-affecting hook 적용은 아직 완료 기준에 남아 있다.

## 문서 목적

기존 `004-tool-runtime`, `005-skill-system`, `012-runtime-services`, `014-observability-diagnostics-and-inspection`는 core primitive 또는 current architecture를 이미 닫았거나 별도 owner boundary로 고정했다. 따라서 user-extensible hooks/plugins는 그 문서들을 다시 여는 보강이 아니라 새 owner spec으로 둔다.

Hermes reference에서 가져올 것은 다음 제품 의미론이다.

- third-party 또는 user-local extension은 기본 opt-in이다.
- core tool/schema를 계속 늘리지 않고 capability는 edge surface에서 확장한다.
- lifecycle hook은 관측과 제한된 변환을 제공하되 session truth와 permission authority를 직접 갖지 않는다.
- plugin은 tool, hook, command, bundled skill 같은 여러 표면을 제공할 수 있지만, 각 표면은 기존 owner boundary를 통과한다.
- gateway startup hook처럼 agent turn 또는 side effect를 유발하는 hook은 사용자가 명시적으로 설치/활성화한 경우에만 동작한다.

그대로 가져오지 않을 것은 Python in-process plugin loader, pip/Nix 배포 모델, hosted plugin marketplace, 조직/fleet governance, provider family 무제한 확장이다. 초기 구현은 로컬 manifest, command-backed hook/tool bridge, MCP/server declaration, Markdown skill bundle처럼 audit 가능한 extension부터 시작한다.

---

## 상위 기준과의 관계

| spec | 025가 소비하는 것 | 025가 소유하는 것 |
|---|---|---|
| 004 tool runtime | `ToolRegistry`, validation, execution result, interrupt, checkpoint | plugin-provided tool이 기존 runtime executor를 우회하지 않는 규칙 |
| 005 skill system | Markdown skill discovery, `PluginProvided` source kind, read-only injection | plugin bundle이 skill을 제공할 때의 opt-in, namespace, conflict/readiness 계약 |
| 008 config/runtime layout | config path, profile/user data dir, env/secret convention | `plugins.*`, `hooks.*` config key 의미와 local extension root layout |
| 010 host safety | permission, secret, redaction, protected target | plugin/hook이 permission ceiling을 높일 수 없고 secret을 자동 획득하지 못한다는 계약 |
| 012 runtime services | gateway/channel lifecycle, process-local service events | gateway hook event catalog와 non-blocking dispatch 의미 |
| 013 UI/session UX | CLI/TUI/local API projection and commands | plugin/hook inspect, enable/disable command, loaded extension projection 의미 |
| 014 diagnostics | logs, diagnostics, redaction evidence | extension load/skip/error, hook dispatch, plugin tool mapping evidence |
| 020 tool search | provider-visible tool surface and deferrable catalog | plugin-provided non-core tools가 Tool Search deferrable set에 들어가는 조건 |
| 022 auto approval | permission mode and capability taxonomy | plugin action admission이 inherited permission ceiling 아래에서만 실행되는 규칙 |

025는 닫힌 spec의 closure를 취소하지 않는다. 닫힌 spec은 core primitive의 현재 경계를 유지하고, 025는 그 primitive를 사용자 확장 표면에서 어떻게 소비할지 소유한다.

---

## 범위

이 문서는 다음을 정의한다.

- plugin manifest와 discovery root.
- enabled/disabled/not-enabled/blocked activation state.
- extension surface taxonomy.
- lifecycle hook event catalog와 dispatch semantics.
- plugin-provided tool, command, skill, MCP/server declaration boundary.
- hook return value가 행동을 바꿀 수 있는 제한된 지점.
- permission, secret, redaction, diagnostics, replay safety.
- CLI/TUI/local API projection과 구현 PRD 분할.

이 문서는 dynamic library loading, general-purpose scripting engine, public marketplace, organization admin policy rollout, new provider family expansion, third-party plugin code의 완전 sandbox implementation을 정의하지 않는다.

---

## Implemented Foundation Boundary

현재 구현 완료 범위는 사용자가 plugin 상태와 선언 surface를 안전하게 관찰하고 config activation gate를 조작할 수 있는 foundation, 그리고 enabled plugin hook을 agent runtime에서 diagnostics-only로 dispatch하는 첫 executable slice다. 이 범위는 전체 Spec 25 완료가 아니라 behavior-affecting plugin system을 열기 전의 안전한 기반이다.

- `plugin.json` discovery, digest, state projection은 지원한다.
- `plugin.toml`은 `plugin.json`과 같은 discovery/config gate를 통과하며, TOML manifest는 snake_case 필드 이름을 기본으로 받는다.
- Hook은 event catalog, output validation, timeout/error diagnostics를 제공하며, enabled plugin의 typed hook entrypoint는 agent runtime에서 diagnostics-only로 dispatch될 수 있다.
- Plugin tool, command, skill, MCP declaration은 descriptor-only metadata로만 projection된다.
- Plugin command-backed process execution, MCP server startup, dynamic library/WASM/Python/scripting runtime은 구현하지 않았다.
- Runtime hook dispatch output은 redacted evidence와 digest로만 남기며 tool calls, model content, permissions, provider-visible tools, commands, skills, MCP server를 mutate하지 않는다.
- `disabled`, `blocked`, `not_enabled`, untrusted workspace-local plugin은 active tool/skill/hook/command/MCP surface를 만들지 않는다.
- `plugins list/inspect/doctor/enable/disable`과 `hooks list/inspect` CLI는 redaction-safe projection을 제공한다. `enable`/`disable`은 config만 수정하며 running session/toolset을 mutate하지 않는다.
- Replay는 plugin live dispatch를 허용하지 않고 recorded/redacted evidence만 해석하는 경계를 유지한다.

Full Spec 25 완료까지 남은 핵심 범위:

- 제한된 behavior-affecting hook output의 실제 runtime 적용.
- Command-backed plugin tool execution과 MCP-backed handler startup.
- Plugin-provided skill activation과 plugin command router integration.
- Execution env allow-list materialization, output limit, redaction evidence의 end-to-end 적용.
- Behavior-affecting hook/tool/command/MCP/skill execution wiring beyond diagnostics-only hook dispatch.

---

## 핵심 정의

### plugin

Plugin은 사용자가 로컬 runtime에 추가로 설치한 extension package다. 초기 package 단위는 directory + manifest이며, manifest는 UTF-8 text config여야 한다. Plugin은 session truth, permission truth, provider auth, tool execution authority를 직접 소유하지 않는다.

### hook

Hook은 runtime lifecycle의 특정 event를 관찰하거나 제한된 변환을 제안하는 extension callback이다. Hook은 event를 직접 확정하지 않는다. Hook output은 owner boundary가 허용한 곳에서만 소비된다.

### extension surface

초기 extension surface는 다음 범주로 나눈다.

- `tool`: provider-visible 또는 Tool Search-deferred tool schema와 command/MCP-backed handler declaration.
- `hook`: lifecycle event observer 또는 제한된 veto/transform callback.
- `command`: CLI/TUI/local API 또는 slash command로 진입하는 user command.
- `skill`: plugin package가 제공하는 Markdown skill bundle.
- `mcp`: plugin이 묶어서 제공하는 MCP server declaration template.
- `asset`: plugin-owned static data file.

각 surface는 해당 owner spec을 통과해야 한다. 예를 들어 tool은 004 runtime executor와 010/022 permission을 통과하고, skill은 005 read-only 주입 규칙을 따른다.

### extension state

Plugin은 discovery와 activation을 분리한다.

- `not_enabled`: 발견됐지만 load되지 않는다.
- `enabled`: 다음 runtime/session에서 load 대상이다.
- `disabled`: 명시적으로 거절되어 enabled보다 우선한다.
- `blocked`: manifest, permission, missing secret, unsafe path, version mismatch 때문에 load가 거절됐다.

새 user-installed plugin의 기본 상태는 `not_enabled`다. Project-local plugin은 기본적으로 `not_enabled`이며, trusted workspace gate가 없으면 executable surface에 들어가면 안 된다.

---

## Extension Root와 Manifest

초기 root는 다음 범위를 권장한다.

```text
<user-data>/plugins/<plugin-name>/plugin.json 또는 plugin.toml
<workspace>/.shacs-bot/plugins/<plugin-name>/plugin.json 또는 plugin.toml
```

Project-local root는 supply-chain surface이므로 기본 load 금지다. 사용자가 workspace trust 또는 config gate를 켠 경우에만 enable 후보가 된다.

Manifest 최소 필드:

```text
name
version
description
surfaces
requires_env
permissions
entrypoints
assets
```

Manifest 규칙:

1. `name`은 registry key이며 path와 충돌하지 않아야 한다.
2. `surfaces`는 제공하려는 extension surface를 명시해야 한다.
3. `requires_env`는 secret value가 아니라 required ref metadata만 담는다.
4. `permissions`는 ceiling request다. 허용 확정이 아니다.
5. `entrypoints`는 command-backed tool/hook 또는 MCP declaration처럼 runtime이 실행 경계를 통제할 수 있는 형태여야 한다.
6. Manifest load 실패는 전체 runtime 실패가 아니라 plugin `blocked` diagnostics여야 한다.

---

## Hook Event Catalog

초기 hook event는 Hermes reference를 제품 의미론으로만 받아들이고, `shacs-bot` owner boundary에 맞춰 아래처럼 제한한다.

| event | 시점 | hook output |
|---|---|---|
| `runtime:start` | local runtime process가 시작됨 | observer only |
| `runtime:stop` | runtime stop/restart/recover marker 처리 전후 | observer only |
| `session:start` | 새 session turn 또는 새 session key 생성 | observer only |
| `session:end` | turn cleanup 또는 session finalize | observer only |
| `command:before` | user command dispatch 전 | `allow`, `skip`, `rewrite` 제안 가능 |
| `llm:before` | provider loop 시작 전 | bounded user-message context injection 가능 |
| `llm:after` | final assistant response 확정 후 | observer only |
| `tool:before` | tool runtime executor 진입 전 | `allow` 또는 `block` 제안 가능 |
| `tool:after` | tool result 정규화 후 | observer only |
| `tool:transform_result` | provider에 tool message를 돌려주기 전 | bounded string transform 가능 |
| `subagent:end` | child result가 parent merge 전 도착 | observer only |
| `channel:inbound` | external channel message normalization 후 | `allow`, `skip`, `rewrite` 제안 가능 |

Hook return rule:

1. 대부분 hook은 observer-only다.
2. `tool:before` block은 tool error로 provider에 반환될 수 있지만, permission allow를 만들 수 없다.
3. `llm:before` injection은 system prompt를 바꾸지 않고 현재 user message 옆에 ephemeral context로만 붙는다.
4. `command:before`와 `channel:inbound` rewrite/skip은 MainOrchestrator 또는 command router가 다시 검증해야 한다.
5. Hook이 panic, timeout, invalid output을 만들면 해당 hook만 실패 처리하고 runtime은 계속된다.

---

## Plugin-Provided Tools, Commands, Skills

Plugin tool은 core tool이 아니다.

1. Tool schema는 plugin manifest 또는 plugin-owned schema file에서 온다.
2. Handler는 초기 구현에서 command-backed process, MCP server, 또는 runtime이 아는 safe adapter를 통해 실행한다.
3. Handler output은 004의 `ToolResult`/normalized tool message 경계를 통과한다.
4. Plugin tool은 010/022 permission ceiling을 높일 수 없다.
5. Plugin tool은 기본적으로 020 Tool Search의 deferrable candidate다.
6. Unknown, disabled, blocked plugin tool은 provider-visible schema에 나타나면 안 된다.

Plugin command는 user command surface다. Command는 세션 상태를 직접 바꾸지 않고 command router 또는 MainOrchestrator로 재진입해야 한다.

Plugin skill은 005의 Markdown skill이다.

1. 이름은 `plugin:<plugin-name>/<skill-name>` 형태로 namespace할 수 있어야 한다.
2. 충돌 시 자동 병합하지 않는다.
3. Plugin이 disabled면 해당 skill도 available set에서 제외한다.
4. Skill은 permission이나 tool visibility를 얻지 않는다.
5. Skill body hash와 plugin manifest digest가 inspect evidence로 남아야 한다.

---

## Safety and Permission

핵심 불변식:

1. Plugin은 permission ceiling을 높일 수 없다.
2. Plugin hook은 permission decision을 직접 승인할 수 없다.
3. Plugin은 provider auth나 secret store를 raw로 읽는 기본 권한을 갖지 않는다.
4. Project-local plugin은 explicit trust 없이 실행되면 안 된다.
5. Plugin load, hook dispatch, command-backed handler execution은 timeout과 output limit을 가져야 한다.
6. Plugin이 만든 user-message injection은 ephemeral이고 session history 원문을 mutate하지 않는다.
7. Plugin action은 diagnostics/replay에서 redacted evidence로 해석 가능해야 하며, replay가 destructive plugin command를 live-dispatch하면 안 된다.

---

## Observability and UI Projection

Inspect surface는 최소한 아래를 보여야 한다.

- discovered plugin count, enabled/disabled/blocked count.
- plugin name, version, source root, manifest digest.
- provided surfaces.
- missing env/config refs.
- blocked reason.
- hook dispatch count and last error summary.
- plugin tool names and Tool Search deferrable status.

CLI/TUI/local API command는 최소한 다음 의미를 제공해야 한다.

```text
plugins list
plugins inspect <name>
plugins enable <name>
plugins disable <name>
plugins doctor
hooks list
hooks inspect <plugin-or-hook>
```

명령 이름은 013이 최종 UX로 조정할 수 있지만, 의미는 동일해야 한다.

---

## PRD 분할

1. `prds/000-plugin-manifest-discovery-and-config-gates.md`: manifest schema, root discovery, enabled/disabled/not-enabled/blocked state, workspace trust gate.
2. `prds/001-hook-event-catalog-and-dispatch.md`: hook event enum, observer-only dispatch, veto/transform 가능한 제한 event, timeout/error isolation.
3. `prds/002-command-backed-plugin-tools.md`: command/MCP-backed plugin tool registration, schema validation, permission ceiling, Tool Search deferrable integration.
4. `prds/003-plugin-skills-and-commands.md`: plugin-provided Markdown skill namespace, command router integration, conflict and disabled plugin behavior.
5. `prds/004-permission-secret-and-replay-safety.md`: secret ref handling, project trust, destructive replay prevention, diagnostics redaction.
6. `prds/005-user-facing-management-and-diagnostics.md`: CLI/TUI/local API list/inspect/enable/disable/doctor projection and release evidence gate.
7. `prds/006-sequential-implementation-plan.md`: PRD 000-005의 안전한 구현 순서, opt-in discovery부터 executable surface closure까지의 gate.

---

## 전체 완료 기준

- Plugin discovery와 activation state가 config와 workspace trust gate를 따른다.
- Broken plugin, missing env, unsafe path는 runtime 전체를 실패시키지 않고 blocked diagnostics로 남는다.
- Hook catalog와 output validation은 observer-only 기본값과 제한된 veto/transform event를 구분하고, live callback dispatch가 timeout/error isolation을 통과한다.
- Hook failure와 timeout은 runtime을 crash시키지 않는다.
- Plugin tool descriptor와 live handler execution은 기존 tool runtime, permission, Tool Search scope를 우회하지 않는다.
- Plugin-provided skill은 read-only이고 permission을 얻지 못한다.
- CLI projection은 loaded/blocked/missing-env/hook metadata 상태를 redaction-safe하게 보여준다. TUI/local API projection은 후속 slice다.
- Replay와 diagnostics는 destructive plugin command를 실제 재실행하지 않고 evidence만 해석한다.
- 문서는 Python plugin loader, public marketplace, organization governance를 구현 완료처럼 주장하지 않는다.
