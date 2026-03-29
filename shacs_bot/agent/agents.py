"""TOML 기반 선언적 에이전트 정의 및 레지스트리."""

from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from loguru import logger

from shacs_bot.config.schema import MCPServerConfig

# ── sandbox_mode → allowed_tools 매핑 ──────────────────────────

_READONLY_TOOLS: list[str] = [
    "read_file", "list_dir", "exec", "web_search", "web_fetch", "search_history",
]
_WORKSPACE_WRITE_TOOLS: list[str] = [
    *_READONLY_TOOLS, "write_file", "edit_file",
]


@dataclass(frozen=True)
class AgentDefinition:
    """에이전트 정의 — TOML 또는 built-in에서 로드."""

    name: str
    description: str
    developer_instructions: str
    model: str | None = None
    sandbox_mode: str = "full"  # "read-only" | "workspace-write" | "full"
    max_iterations: int = 15
    allowed_tools: list[str] = field(default_factory=list)
    mcp_servers: dict[str, MCPServerConfig] = field(default_factory=dict)
    source: str = "builtin"  # "builtin" | "user" | "workspace"

    def get_effective_tools(self) -> list[str]:
        """allowed_tools가 비어있으면 sandbox_mode에 따라 결정."""
        if self.allowed_tools:
            return self.allowed_tools
        if self.sandbox_mode == "read-only":
            return list(_READONLY_TOOLS)
        if self.sandbox_mode == "workspace-write":
            return list(_WORKSPACE_WRITE_TOOLS)
        return []  # full → 전체 허용


# ── built-in 에이전트 정의 ──────────────────────────────────────

_RESEARCHER_PROMPT = """\
당신은 정보 수집 전문 에이전트입니다.

## 임무
웹 검색과 URL 크롤링을 통해 정보를 수집하고 정리합니다.

## 행동 규칙
- 여러 소스를 교차 확인하여 정확성을 높이세요
- 출처를 명시하세요 (URL, 날짜)
- 사실과 의견을 구분하세요
- 검색 결과가 부족하면 다른 키워드로 재시도하세요

## 결과 보고
- 핵심 발견사항을 구조적으로 정리
- 출처 목록 포함
- 불확실한 부분은 명시

## 제약
- 읽기 전용: 파일을 생성, 수정, 삭제할 수 없습니다
- 조사 결과만 보고하세요. 임의로 행동하지 마세요.\
"""

_ANALYST_PROMPT = """\
당신은 분석/요약 전문 에이전트입니다.

## 임무
문서, 파일, 데이터를 읽고 분석하여 인사이트를 제공합니다.

## 행동 규칙
- 원본 내용을 정확히 파악한 후 분석하세요
- 핵심 포인트를 추출하고 구조화하세요
- 비교 요청 시 기준을 명확히 하세요
- 분석 근거를 항상 제시하세요

## 결과 보고
- 요약 → 상세 분석 → 결론 순서
- 표나 목록을 활용하여 가독성 확보
- 원문 인용 시 해당 위치 명시

## 제약
- 읽기 전용: 파일을 생성, 수정, 삭제할 수 없습니다
- 분석 결과만 보고하세요. 임의로 행동하지 마세요.\
"""

_EXECUTOR_PROMPT = """\
당신은 작업 실행 전문 에이전트입니다.

## 임무
파일 작업, 명령 실행, 스킬 기반 작업을 수행합니다.

## 행동 규칙
- 파일을 수정하기 전에 반드시 먼저 읽으세요
- 작업 전후로 결과를 확인하세요
- 한 번에 하나의 변경에 집중하세요
- 요청 범위를 벗어나는 변경을 하지 마세요

## 결과 보고
- 무엇을 했는지 간결하게
- 변경된 파일 목록
- 확인 결과 (성공/실패)

## 제약
- 위험한 명령은 실행하지 마세요 (rm -rf, format 등)
- 할당된 작업에만 집중하세요\
"""


BUILTIN_AGENTS: dict[str, AgentDefinition] = {
    "researcher": AgentDefinition(
        name="researcher",
        description="웹 검색과 URL 크롤링을 통해 정보를 수집하고 정리하는 읽기 전용 에이전트",
        developer_instructions=_RESEARCHER_PROMPT,
        sandbox_mode="read-only",
        allowed_tools=list(_READONLY_TOOLS),
        max_iterations=10,
    ),
    "analyst": AgentDefinition(
        name="analyst",
        description="문서, 파일, 데이터를 읽고 분석하여 인사이트를 제공하는 읽기 전용 에이전트",
        developer_instructions=_ANALYST_PROMPT,
        sandbox_mode="read-only",
        allowed_tools=list(_READONLY_TOOLS),
        max_iterations=10,
    ),
    "executor": AgentDefinition(
        name="executor",
        description="파일 작업, 명령 실행, 스킬 기반 작업을 수행하는 에이전트",
        developer_instructions=_EXECUTOR_PROMPT,
        sandbox_mode="full",
        max_iterations=15,
    ),
}


# ── AgentRegistry ───────────────────────────────────────────────


class AgentRegistry:
    """TOML 파일과 built-in에서 에이전트 정의를 로드하고 조회한다."""

    def __init__(self, workspace: Path, user_agents_dir: Path | None = None):
        self._workspace = workspace
        self._user_agents_dir = user_agents_dir or Path("~/.shacs-bot/agents").expanduser()
        self._agents: dict[str, AgentDefinition] = {}
        self._load()

    def _load(self) -> None:
        # 1. built-in (최저 우선순위)
        self._agents.update(BUILTIN_AGENTS)

        # 2. 사용자 에이전트 (~/.shacs-bot/agents/*.toml)
        self._load_from_dir(self._user_agents_dir, source="user")

        # 3. 워크스페이스 에이전트 ({workspace}/agents/*.toml) — 최우선
        self._load_from_dir(self._workspace / "agents", source="workspace")

    def _load_from_dir(self, directory: Path, source: str) -> None:
        if not directory.exists():
            return
        for toml_file in sorted(directory.glob("*.toml")):
            try:
                agent = self._parse_toml(toml_file, source)
                if agent:
                    self._agents[agent.name] = agent
                    logger.info("커스텀 에이전트 로드: {} ({}, {})", agent.name, source, toml_file)
            except Exception as e:
                logger.warning("에이전트 TOML 파싱 실패 {}: {}", toml_file, e)

    def _parse_toml(self, path: Path, source: str) -> AgentDefinition | None:
        with open(path, "rb") as f:
            data = tomllib.load(f)

        name = data.get("name")
        description = data.get("description")
        instructions = data.get("developer_instructions")

        if not all([name, description, instructions]):
            logger.warning("에이전트 TOML에 필수 필드 누락 (name, description, developer_instructions): {}", path)
            return None

        # MCP 서버 파싱
        mcp_servers: dict[str, MCPServerConfig] = {}
        for srv_name, srv_data in data.get("mcp_servers", {}).items():
            if isinstance(srv_data, dict):
                mcp_servers[srv_name] = MCPServerConfig(**srv_data)

        return AgentDefinition(
            name=name,
            description=description,
            developer_instructions=instructions,
            model=data.get("model"),
            sandbox_mode=data.get("sandbox_mode", "full"),
            max_iterations=data.get("max_iterations", 15),
            allowed_tools=data.get("allowed_tools", []),
            mcp_servers=mcp_servers,
            source=source,
        )

    def get(self, name: str) -> AgentDefinition | None:
        return self._agents.get(name)

    def list_agents(self) -> list[AgentDefinition]:
        return list(self._agents.values())

    def reload(self) -> None:
        """TOML 파일을 다시 로드한다."""
        self._agents.clear()
        self._load()

    def build_agents_summary(self) -> str:
        """시스템 프롬프트에 포함할 에이전트 목록 XML."""
        if not self._agents:
            return ""

        lines: list[str] = ["<agents>"]
        for agent in self._agents.values():
            name = agent.name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            desc = agent.description.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            lines.append(f'  <agent name="{name}" source="{agent.source}">')
            lines.append(f"    <description>{desc}</description>")
            if agent.model:
                lines.append(f"    <model>{agent.model}</model>")
            lines.append("  </agent>")
        lines.append("</agents>")
        return "\n".join(lines)
