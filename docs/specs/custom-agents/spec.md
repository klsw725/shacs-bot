# SPEC: 커스텀 에이전트 아키텍처

> **Prompt**: Codex subagents 패턴을 참고하여 TOML 기반 선언적 에이전트 정의 + 에이전트별 모델 라우팅 + 에이전트별 MCP 서버 + 동시성 제한 도입. 에이전트가 도구 조직의 1급 단위가 되는 아키텍처.
>
> **선행**: [스킬 격리 실행 (skill-isolation)](../skill-isolation/spec.md) — ApprovalGate, 출처 기반 신뢰 모델, spawn_skill 인프라. 이 스펙은 스킬 격리를 **확장**한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`custom-agents.md`](prds/custom-agents.md) | TOML 기반 선언적 에이전트 + 모델 라우팅 + MCP + 동시성 + ApprovalGate 확장 |
| [`agent-install.md`](prds/agent-install.md) | Git 기반 에이전트 + 스킬 번들 설치 (`/agent install\|list\|remove\|update`) |

---

## 전제: shacs-bot ≠ Codex

shacs-bot은 Discord/Telegram/MoChat 등에서 동작하는 **장시간 상주 개인 비서 봇**이다. Codex는 로컬 CLI **코딩 에이전트**다.

따라서 Codex 패턴을 직접 복사가 아닌 "번역"해야 한다:

| Codex (로컬 CLI) | shacs-bot (상주 봇) |
|---|---|
| 프로세스 1회 실행 → 종료 | 프로세스 장시간 상주 |
| MCP 세션 = 프로세스 수명 | MCP 세션 관리가 복잡 (서브에이전트별 독립) |
| `~/.codex/agents/` (로컬) | `~/.shacs-bot/agents/` (서버) |
| `.codex/agents/` (프로젝트) | `{workspace}/agents/` (워크스페이스) |
| `spawn_agents_on_csv` (배치) | 해당 없음 (대화형 봇) |
| 사용자 = 개발자 (TOML 직접 편집) | 사용자 = 봇 운영자 (TOML 편집 가능) |
| 닉네임 풀 (CLI UI) | 해당 없음 (백엔드) |

**가져오는 것**: TOML 기반 선언적 에이전트, 에이전트별 모델/MCP, max_threads, sandbox_mode, 우선순위 체계 (workspace > user > built-in)

**가져오지 않는 것**: spawn_agents_on_csv, 닉네임 풀, IDE 통합, max_depth (현재 구조에서 불필요 — 서브에이전트에 spawn 미등록으로 재귀 자동 방지)

---

## TL;DR

> **목적**: 하드코딩된 3개 서브에이전트 역할을 TOML 기반 선언적 에이전트 시스템으로 확장. 에이전트별 모델 라우팅으로 비용 최적화, 에이전트별 MCP 연결로 도구 격리, 동시성 제한으로 비용 안전장치. 스킬 격리의 출처 기반 신뢰 모델과 ApprovalGate를 커스텀 에이전트에도 적용.
>
> **Deliverables**:
> - `agent/agents.py` — AgentDefinition + AgentRegistry (TOML 로드 + built-in 통합)
> - `config/schema.py` — 기존 AgentsConfig에 max_threads 추가
> - `agent/subagent.py` — 모델 라우팅 + 동시성 제한 + 에이전트별 MCP + 출처 기반 ApprovalGate
> - `agent/approval.py` — skill_name → entity_name 일반화
> - `agent/tools/spawn.py` — 동적 역할 지원
> - `agent/context.py` — 에이전트 목록 시스템 프롬프트
>
> **Estimated Effort**: Medium (1-2일)

---

## Context

### 현재 서브에이전트 시스템

```python
# subagent.py
SUBAGENT_ROLES: dict[str, SubagentRole] = {
    "researcher": SubagentRole(system_prompt=RESEARCHER_PROMPT, allowed_tools=[...], max_iterations=10),
    "analyst": SubagentRole(system_prompt=ANALYST_PROMPT, allowed_tools=[...], max_iterations=10),
    "executor": SubagentRole(system_prompt=EXECUTOR_PROMPT, allowed_tools=[], max_iterations=15),
}
```

**문제**:
1. 역할이 하드코딩 — 사용자가 특화 에이전트를 추가할 수 없음
2. 모든 에이전트가 동일 모델 — 탐색에 고급 모델 낭비
3. MCP 도구는 메인 에이전트에만 연결 — 서브에이전트는 MCP 도구 접근 불가
4. 동시성 제한 없음 — LLM이 spawn을 무제한 호출 가능
5. 일반 서브에이전트에는 ApprovalGate 미적용 — 스킬만 승인 게이트 통과

### 현재 도구 수

| 항목 | 개수 |
|---|---|
| built-in 도구 | 8개 |
| 관리 도구 | 4개 (message, spawn, list_tasks, cancel_task) |
| 조건부 도구 | 2개 (cron, media_generate) |
| MCP 도구 | config에 따라 0~N개 |
| **합계** | 14 + MCP |

도구 40개+ 시점에서 에이전트별 도구 분리가 본격적으로 유의미하나, 아키텍처는 지금 설계하여 점진적으로 도구가 늘어날 때 자연스럽게 수용할 수 있도록 한다.

### 스킬 격리와의 관계

이 스펙은 스킬 격리(skill-isolation)의 **확장**이다. 공유하는 인프라:

| 인프라 | 스킬 격리에서 도입 | 이 스펙에서 확장 |
|---|---|---|
| `ApprovalGate` | workspace 스킬 전용 | workspace 에이전트에도 적용 |
| 출처 기반 신뢰 | builtin/workspace 스킬 | builtin/user/workspace 에이전트 |
| `SubagentManager` | `spawn_skill()` + `_run_skill()` | `spawn()` + `_run_subagent()` 강화 |
| 도구 필터링 | 스킬: spawn 미등록 (재귀 방지) | 에이전트: sandbox_mode + allowed_tools |

### 참고 아키텍처: Codex Subagents

- **Custom Agent TOML**: `name`, `description`, `developer_instructions` (필수) + `model`, `sandbox_mode`, `mcp_servers`, `allowed_tools` (선택)
- **우선순위**: 프로젝트 `.codex/agents/` > 사용자 `~/.codex/agents/` > built-in
- **동시성**: `agents.max_threads` (기본 6)
- **MCP 격리**: 에이전트 TOML에 `[mcp_servers]` 정의 → 해당 에이전트에서만 연결

---

## 보안 모델: 2중 방어 레이어

스킬 격리에서 도입한 출처 기반 신뢰 모델을 커스텀 에이전트에도 동일하게 적용한다.

### 출처별 신뢰 수준

| 출처 | 예시 | 신뢰 수준 | ApprovalGate |
|---|---|---|---|
| `builtin` | 코드 내장 (researcher, analyst, executor) | 신뢰 | ❌ 미적용 |
| `user` | `~/.shacs-bot/agents/*.toml` | 신뢰 | ❌ 미적용 (운영자가 직접 작성) |
| `workspace` | `{workspace}/agents/*.toml` | **비신뢰** | ✅ 적용 (외부 주입 가능) |

`workspace` 에이전트는 스킬과 동일한 위협 모델을 갖는다: `developer_instructions`에 악의적 프롬프트가 있을 수 있고, `allowed_tools`를 `"full"`로 지정하여 파괴적 도구를 실행할 수 있음.

### 방어 레이어

```
Layer 1: sandbox_mode (도구 등록 필터)
  → 어떤 도구가 서브에이전트에 등록되는지 결정
  → "read-only"면 write_file, edit_file 자체가 등록 안 됨
  → TOML에서 선언, 정적

Layer 2: ApprovalGate (도구 호출 검사)
  → 등록된 도구의 실제 호출을 승인/거부
  → workspace 에이전트에만 적용
  → 기존 skill_approval 모드 (auto/manual/off) 재사용
  → 런타임, 동적
```

두 레이어는 **독립적이며 중첩 적용**된다:
- `sandbox_mode = "full"` + workspace → 모든 도구 등록, 하지만 ApprovalGate가 각 호출 검사
- `sandbox_mode = "read-only"` + builtin → 읽기 도구만 등록, 승인 불필요

### ApprovalGate 일반화

현재 `ApprovalGate.__init__`의 `skill_name` 파라미터를 `entity_name`으로 일반화:

```python
# 현재
ApprovalGate(skill_name="youtube-summary", ...)

# 변경 후
ApprovalGate(entity_name="youtube-summary", ...)  # 스킬
ApprovalGate(entity_name="reviewer", ...)          # 에이전트
```

사용자에게 보이는 승인 메시지도 일반화:
```
🛡 에이전트 'reviewer'이 실행하려 합니다:    (커스텀 에이전트)
🛡 스킬 'youtube-summary'이 실행하려 합니다:  (스킬 — 기존과 동일)
```

---

## 변경 1: AgentDefinition + AgentRegistry

**신규 파일**: `shacs_bot/agent/agents.py`

에이전트 정의 데이터클래스와 TOML 기반 레지스트리.

### AgentDefinition

```python
@dataclass(frozen=True)
class AgentDefinition:
    name: str                                          # 필수: 에이전트 식별자
    description: str                                   # 필수: LLM이 참고하는 설명
    developer_instructions: str                        # 필수: 시스템 프롬프트 핵심 지시사항
    model: str | None = None                           # 선택: None이면 메인 모델 상속
    sandbox_mode: str = "full"                         # "read-only" | "workspace-write" | "full"
    max_iterations: int = 15
    allowed_tools: list[str] = field(default_factory=list)       # 비어있으면 sandbox_mode에 따라 결정
    mcp_servers: dict[str, MCPServerConfig] = field(default_factory=dict)  # 에이전트 전용 MCP 서버
    source: str = "builtin"                            # "builtin" | "user" | "workspace"
```

### sandbox_mode → allowed_tools 자동 매핑

`allowed_tools`가 비어있고 `sandbox_mode`가 지정된 경우:

```
read-only       → ["read_file", "list_dir", "exec", "web_search", "web_fetch", "search_history"]
workspace-write → 위 + ["write_file", "edit_file"]
full            → []  (전체 허용)
```

`read-only`의 `exec`는 허용하되, ApprovalGate(workspace 에이전트)가 파괴적 명령 차단.

### AgentRegistry

```python
class AgentRegistry:
    def __init__(self, workspace: Path, user_agents_dir: Path | None = None):
        # 로드 순서 (후순위가 override):
        # 1. BUILTIN_AGENTS (하드코딩, 최저 우선순위)
        # 2. ~/.shacs-bot/agents/*.toml (사용자)
        # 3. {workspace}/agents/*.toml (워크스페이스, 최우선)

    def get(self, name: str) -> AgentDefinition | None: ...
    def list_agents(self) -> list[AgentDefinition]: ...
    def build_agents_summary(self) -> str: ...  # 시스템 프롬프트용 XML
    def reload(self) -> None: ...               # 핫 리로드 (향후 /agents reload 명령어용)
```

### TOML 스키마

```toml
# ~/.shacs-bot/agents/reviewer.toml

name = "reviewer"
description = "PR 리뷰에 집중하는 읽기 전용 에이전트"

developer_instructions = """
코드를 오너처럼 리뷰하세요.
정확성, 보안, 동작 회귀, 누락된 테스트 커버리지를 우선 확인하세요.
"""

# 선택 필드
model = "claude-sonnet-4-20250514"
sandbox_mode = "read-only"
max_iterations = 10
allowed_tools = ["read_file", "list_dir", "exec"]

# 에이전트 전용 MCP 서버
[mcp_servers.docs]
url = "https://docs.example.com/mcp"
tool_timeout = 30
enabled_tools = ["search_docs", "get_page"]
```

### 포맷 결정: TOML

| 근거 | |
|---|---|
| `tomllib` (Python 3.11+ stdlib) | 의존성 0 |
| 코드베이스에서 pyyaml 명시적 폐기 (skill-isolation v3) | YAML 제외 |
| `pyproject.toml` 이미 사용 | 관례 일치 |
| Codex와 동일 포맷 | 호환성 |
| 멀티라인 문자열 `"""..."""` | developer_instructions에 적합 |

### 모델 라우팅 제약

TOML의 `model` 필드는 **메인 프로바이더가 지원하는 모델만** 허용한다. `SubagentManager`는 단일 `LLMProvider` 인스턴스를 사용하므로, 메인이 Anthropic인데 `model = "gpt-4o-mini"`를 지정하면 실패한다.

이는 의도된 제약이다. 프로바이더별 에이전트 분기는 복잡도 대비 이득이 작다. 동일 프로바이더 내에서 고급/저급 모델을 선택하는 것이 주 사용례 (예: `claude-sonnet-4-20250514` vs `claude-haiku-4-5-20251001`).

### Built-in → AgentDefinition 변환

기존 `SUBAGENT_ROLES`의 3개 역할(researcher, analyst, executor)을 `BUILTIN_AGENTS: dict[str, AgentDefinition]`으로 변환. 프롬프트 내용은 동일 유지.

---

## 변경 2: 동시성 설정

**파일**: `shacs_bot/config/schema.py`

**기존** `AgentsConfig` 클래스에 필드 추가 (새 클래스가 아님):

```python
class AgentsConfig(Base):
    """Agent configuration."""
    defaults: AgentDefaults = Field(default_factory=AgentDefaults)
    max_threads: int = 6   # 동시 실행 서브에이전트 최대 수
```

```jsonc
// ~/.shacs-bot/config.json
{
  "agents": {
    "defaults": { "model": "...", "workspace": "..." },
    "maxThreads": 6
  }
}
```

camelCase (JSON) ↔ snake_case (Python) 매핑은 기존 Pydantic `alias_generator=to_camel` 패턴 따름.

`max_threads`는 **모든 서브에이전트(일반 + 스킬)에 공통 적용**한다. `_running_tasks` dict는 이미 스킬과 일반 서브에이전트를 모두 포함하므로 자연스럽게 통합된다.

---

## 변경 3: SubagentManager 통합

**파일**: `shacs_bot/agent/subagent.py`

### 3-1. 모델 라우팅

```python
async def _run_subagent(self, ..., agent_def: AgentDefinition | None = None):
    model = agent_def.model if (agent_def and agent_def.model) else self._model
    response = await self._provider.chat_with_retry(messages=messages, tools=..., model=model)
```

### 3-2. 동시성 제한

```python
async def spawn(self, ...):
    running = sum(1 for t in self._running_tasks.values() if not t.asyncio_task.done())
    if running >= self._max_threads:
        return f"동시 실행 제한 초과 (최대 {self._max_threads}개). 기존 작업이 완료된 후 다시 시도하세요."
```

`spawn_skill()`에도 동일한 동시성 체크를 적용한다.

### 3-3. 에이전트별 MCP 연결

서브에이전트는 단명(short-lived)이므로 메인 프로세스의 장기 MCP 세션과 다른 전략:

```python
async def _run_subagent(self, ...):
    if agent_def and agent_def.mcp_servers:
        async with AsyncExitStack() as mcp_stack:
            await connect_mcp_servers(agent_def.mcp_servers, tools, mcp_stack)
            await self._chat_loop(messages, tools, model, max_iterations, ...)
        # AsyncExitStack 종료 → MCP 세션 자동 정리
    else:
        await self._chat_loop(messages, tools, model, max_iterations, ...)
```

**핵심**: 서브에이전트의 MCP 연결은 해당 서브에이전트 수명과 동일. 메인 MCP 세션과 완전 독립. `AsyncExitStack` context manager로 자동 정리.

**MCP 연결 실패 시**: 기존 `connect_mcp_servers` 동작과 동일 — 개별 서버 실패는 경고 로그만 남기고 나머지 도구로 계속 진행. 서브에이전트 전체를 실패시키지 않음.

**MCP 연결 오버헤드**: stdio MCP는 프로세스 spawn + 초기화에 수초 소요. HTTP MCP는 무시 가능. 서브에이전트가 수십 초~수분 실행되므로 초기화 비용은 수용 가능.

### 3-4. 커스텀 에이전트 ApprovalGate 적용

`_run_subagent()`에서 에이전트 출처가 `workspace`이면 ApprovalGate를 적용한다:

```python
async def _run_subagent(self, ..., agent_def: AgentDefinition | None = None):
    # 출처 기반 승인 게이트 (스킬 격리와 동일한 패턴)
    needs_approval = (
        agent_def is not None
        and agent_def.source == "workspace"
        and self._skill_approval != "off"
    )
    approval_gate = None
    if needs_approval:
        approval_gate = ApprovalGate(
            mode=self._skill_approval,  # auto/manual/off — 기존 설정 재사용
            entity_name=agent_def.name,
            ...
        )
```

### 3-5. SUBAGENT_ROLES 제거

기존 `SUBAGENT_ROLES` dict와 `SubagentRole` dataclass를 제거하고 `AgentRegistry`로 대체. 기존 3개 역할의 프롬프트는 `BUILTIN_AGENTS`로 이관.

---

## 변경 4: ApprovalGate 일반화

**파일**: `shacs_bot/agent/approval.py`

`skill_name` 파라미터를 `entity_name`으로 변경. 승인 메시지에서 "스킬" 대신 "에이전트"/"스킬"을 동적으로 표시:

```python
class ApprovalGate:
    def __init__(self, ..., entity_name: str, entity_type: str = "스킬", ...):
        self._entity_name = entity_name
        self._entity_type = entity_type  # "스킬" 또는 "에이전트"
```

기존 `_run_skill()`에서 호출 시 `entity_type="스킬"`, 새로운 `_run_subagent()`에서 `entity_type="에이전트"`.

---

## 변경 5: SpawnTool 동적 역할

**파일**: `shacs_bot/agent/tools/spawn.py`

```python
"role": {
    "type": "string",
    "description": "에이전트 이름. 사용 가능한 에이전트는 시스템 프롬프트의 <agents> 참조.",
    # enum 제거 — 동적 에이전트 지원
}
```

기존 `enum: ["researcher", "analyst", "executor"]` 제거. TOML에서 정의된 에이전트를 동적으로 사용할 수 있도록.

---

## 변경 6: 시스템 프롬프트 에이전트 목록

**파일**: `shacs_bot/agent/context.py`

`ContextBuilder.__init__`에 `agent_registry: AgentRegistry | None = None` 파라미터 추가:

```python
class ContextBuilder:
    def __init__(self, workspace: Path, agent_registry: AgentRegistry | None = None):
        ...
        self._agent_registry = agent_registry
```

`build_system_prompt()`에서 에이전트 목록 삽입:

```python
if self._agent_registry:
    agents_summary = self._agent_registry.build_agents_summary()
    if agents_summary:
        parts.append(f"""# 에이전트

사용 가능한 서브에이전트입니다. spawn 도구의 role 파라미터로 지정하세요.

{agents_summary}""")
```

```xml
<agents>
  <agent name="researcher" source="builtin">
    <description>웹 검색과 URL 크롤링을 통해 정보를 수집</description>
  </agent>
  <agent name="reviewer" source="user">
    <description>PR 리뷰에 집중하는 읽기 전용 에이전트</description>
    <model>claude-sonnet-4-20250514</model>
  </agent>
</agents>
```

---

## 실행 흐름

```
spawn(task="...", role="reviewer")
  → AgentRegistry.get("reviewer")
    ├─ built-in에 있으면 built-in 반환
    ├─ ~/.shacs-bot/agents/reviewer.toml → 커스텀 반환 (built-in override)
    └─ {workspace}/agents/reviewer.toml → 커스텀 반환 (최우선)
  → SubagentManager.spawn()
    ├─ max_threads 체크 (일반 + 스킬 합산) → 초과 시 거부
    ├─ AgentDefinition에서 model 결정 (동일 프로바이더 내)
    ├─ allowed_tools 또는 sandbox_mode에 따라 도구 필터링 [Layer 1]
    ├─ source == "workspace" → ApprovalGate 적용 [Layer 2]
    ├─ mcp_servers 있으면 에이전트 전용 MCP 연결
    └─ _run_subagent() 실행
        ├─ 에이전트 모델로 LLM 호출
        ├─ 필터링된 도구만 사용
        ├─ 도구 호출 시 ApprovalGate 검사 (workspace인 경우)
        ├─ MCP 도구 포함 (있는 경우)
        └─ 완료 → MCP 연결 자동 정리 → 결과 보고
```

---

## 변경 파일 요약

| 파일 | 변경 유형 | 설명 | 규모 |
|---|---|---|---|
| `shacs_bot/agent/agents.py` | 신규 | AgentDefinition + BUILTIN_AGENTS + AgentRegistry | ~130줄 |
| `shacs_bot/config/schema.py` | 수정 | 기존 `AgentsConfig`에 `max_threads` 추가 | ~3줄 |
| `shacs_bot/agent/subagent.py` | 수정 | AgentRegistry 통합, 모델 라우팅, 동시성, MCP, ApprovalGate, SUBAGENT_ROLES 제거 | ~90줄 |
| `shacs_bot/agent/approval.py` | 수정 | `skill_name` → `entity_name` + `entity_type` 일반화 | ~10줄 |
| `shacs_bot/agent/tools/spawn.py` | 수정 | role enum 제거, 동적 역할 | ~15줄 |
| `shacs_bot/agent/loop.py` | 수정 | AgentRegistry 생성 + 전달 | ~10줄 |
| `shacs_bot/agent/context.py` | 수정 | AgentRegistry 주입 + 에이전트 목록 프롬프트 | ~15줄 |
| **합계** | | | ~273줄 |

---

## Must NOT (가드레일)

- `pyyaml` 의존성 추가 금지 — TOML만 사용
- `Optional[X]` 사용 금지 — `X | None`만 사용 (AGENTS.md 컨벤션)
- `print()` 사용 금지 — `loguru`만 (AGENTS.md 컨벤션)
- 기존 `spawn(task="...", role="executor")` 호출 깨지면 안 됨 (하위 호환)
- 기존 `spawn_skill()` 로직의 **승인 게이트 동작** 변경 금지 — `entity_name` 리네임은 허용하되 동작은 동일
- 메인 에이전트의 MCP 연결에 영향 없어야 함
- 잘못된 TOML → 해당 에이전트만 스킵, 나머지 정상 동작
- 다른 프로바이더의 모델을 TOML에 지정해도 런타임 크래시 없어야 함 — 에러를 서브에이전트 실패로 처리

---

## 검증

1. **하위 호환**: `spawn(role="executor")` → 기존과 동일 동작
2. **TOML 로드**: `~/.shacs-bot/agents/test.toml` 작성 → `spawn(role="test")` 동작
3. **모델 라우팅**: TOML에 `model = "claude-haiku-4-5-20251001"` → 해당 모델로 LLM 호출 (동일 프로바이더)
4. **동시성**: `maxThreads=3` → 4번째 spawn (일반 또는 스킬) 거부 메시지
5. **MCP 연결**: TOML에 `[mcp_servers.x]` → 서브에이전트에서 MCP 도구 사용
6. **MCP 정리**: 서브에이전트 종료 후 MCP 프로세스 정리 확인
7. **MCP 실패**: MCP 연결 실패 → 경고 로그 + MCP 없이 계속 진행
8. **Override**: built-in과 동일 이름 TOML → TOML 우선
9. **에러 내성**: 잘못된 TOML → 경고 로그 + 다른 에이전트 정상
10. **시스템 프롬프트**: `<agents>` 태그에 모든 에이전트 목록 표시
11. **스킬 격리 무영향**: `spawn_skill()` 기존 동작 유지 (entity_name 리네임 외)
12. **workspace 에이전트 승인**: workspace TOML 에이전트 → skill_approval 모드에 따라 ApprovalGate 적용
13. **builtin/user 에이전트 무승인**: builtin, user 에이전트 → ApprovalGate 미적용

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| stdio MCP 연결 오버헤드 (수초) | 확실 | 중간 | 서브에이전트 실행 시간(수십초~수분) 대비 수용 가능. 로그 모니터링. |
| TOML 파싱 실패 | 낮음 | 낮음 | 개별 실패만 스킵, built-in + 나머지 정상. |
| 커스텀 에이전트 모델이 프로바이더 불일치 | 중간 | 중간 | 서브에이전트 실패로 처리. 사용자에게 에러 보고. 메인 에이전트 무영향. |
| maxThreads가 낮아 유용한 작업 차단 | 중간 | 낮음 | 기본값 6. config에서 조정 가능. |
| 동시 MCP 연결 수 과다 | 낮음 | 중간 | maxThreads로 간접 제한. |
| workspace TOML에 악의적 프롬프트 | 중간 | 높음 | ApprovalGate 적용 (Layer 2). sandbox_mode로 도구 제한 (Layer 1). |

---

## 향후 확장 (이번 구현 범위 밖)

### `_run_skill` + `_run_subagent` 통합

커스텀 에이전트에 ApprovalGate가 추가되면, `_run_skill()`과 `_run_subagent()`의 차이는 **프롬프트 소스**뿐이다:
- 스킬: SKILL.md에서 프롬프트
- 에이전트: TOML의 `developer_instructions`에서 프롬프트

향후 하나의 `_run_agent()` 메서드로 통합 가능. 이번에는 기존 `_run_skill()`을 건드리지 않음.

### SKILL.md에서 에이전트 지정

스킬의 프론트매터에 `agent = "reviewer"` 같은 선언으로, 해당 스킬이 특정 커스텀 에이전트의 설정(모델, MCP)으로 실행되도록 하는 확장.

### 핫 리로드

`AgentRegistry.reload()` 메서드는 인터페이스만 정의. 봇 실행 중 TOML 변경 시 재시작 필요. 향후 `/agents reload` 슬래시 명령어로 핫 리로드 지원 가능.

### 프로바이더별 모델 라우팅

현재는 동일 프로바이더 내 모델만 지원. 향후 `Config._match_provider(model)`을 활용하여 프로바이더까지 동적 결정하는 확장 가능.
