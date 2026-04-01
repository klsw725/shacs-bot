"""동적 도구 관리를 위한 도구 레지스트리"""

from pathlib import Path
from typing import Any

from shacs_bot.agent.hooks import (
    AFTER_TOOL_EXECUTE,
    BEFORE_TOOL_EXECUTE,
    HookContext,
    HookRegistry,
    NoOpHookRegistry,
)
from shacs_bot.agent.tools.base import Tool
from shacs_bot.config.schema import ExecToolConfig


class ToolRegistry:
    """
    에이전트 도구를 위한 레지스트리

    동적 도구 등록 및 실행을 허용합니다.
    """

    def __init__(self, hooks: HookRegistry | None = None):
        self._tools: dict[str, Tool] = {}
        self._hooks: HookRegistry = hooks or NoOpHookRegistry()

    def register(self, tool: Tool) -> None:
        """도구를 등록합니다."""
        self._tools[tool.name] = tool

    def unregister(self, name: str) -> None:
        """이름으로 도구 등록을 취소합니다."""
        self._tools.pop(name, None)

    def get(self, name: str) -> Tool | None:
        """이름으로 도구를 가져옵니다."""
        return self._tools.get(name, None)

    def has(self, name: str) -> bool:
        """도구가 등록되어 있는지 확인합니다."""
        return name in self._tools

    def get_definitions(self) -> list[dict[str, Any]]:
        """OpenAI 형식의 모든 도구 정의를 가져옵니다."""
        return [tool.to_schema() for tool in self._tools.values()]

    async def execute(
        self,
        name: str,
        params: dict[str, Any],
        session_key: str | None = None,
        channel: str | None = None,
    ) -> str:
        """
        이름과 주어진 매개변수로 도구를 실행합니다.

        Args:
            name: 도구 이름입니다.
            params: 도구 매개변수입니다.

        Returns:
            도구 실행 결과 문자열입니다.

        Raises:
            KeyError: 도구를 찾을 수 없는 경우입니다.
        """
        _HINT = "\n\n[위의 오류를 분석하고 다른 접근 방식을 시도해 보세요.]"

        tool: Tool | None = self._tools.get(name)
        if not tool:
            return f"에러: 도구 '{name}'을(를) 찾을 수 없습니다. 가능한 도구: {', '.join(self.tool_names)}"

        from shacs_bot.observability.tracing import span as otel_span

        await self._hooks.emit(
            HookContext(
                event=BEFORE_TOOL_EXECUTE,
                session_key=session_key,
                channel=channel,
                payload={"tool": name, "params_keys": list(params.keys())},
            )
        )

        result: str
        with otel_span("tool_execution", {"tool.name": name}) as s:
            try:
                errors: list[str] = tool.validate_params(params)
                if errors:
                    result = f"에러: 도구 '{name}'의 매개변수가 유효하지 않습니다: " + "; ".join(
                        errors
                    )
                else:
                    result = await tool.execute(**params)
                    if s:
                        s.set_attribute(
                            "tool.success",
                            not isinstance(result, str) or not result.startswith("Error"),
                        )
                        s.set_attribute(
                            "tool.result_length", len(result) if isinstance(result, str) else 0
                        )
                    if isinstance(result, str) and result.startswith("Error"):
                        result = result + _HINT
            except Exception as e:
                result = f"{name} 실행 중 에러 발생: {str(e)}" + _HINT

        await self._hooks.emit(
            HookContext(
                event=AFTER_TOOL_EXECUTE,
                session_key=session_key,
                channel=channel,
                payload={
                    "tool": name,
                    "result_length": len(result) if isinstance(result, str) else 0,
                    "is_error": isinstance(result, str)
                    and (result.startswith("에러") or result.startswith("Error")),
                },
            )
        )
        return result

    @property
    def tool_names(self) -> list[str]:
        """등록된 모든 도구 이름을 가져옵니다."""
        return list(self._tools.keys())

    def __len__(self) -> int:
        return len(self._tools)

    def __contains__(self, name: str) -> bool:
        return name in self._tools


def create_default_tools(
    workspace: Path,
    restrict_to_workspace: bool = False,
    exec_config: ExecToolConfig | None = None,
    brave_api_key: str | None = None,
    web_proxy: str | None = None,
) -> list[Tool]:
    """AgentLoop과 SubagentManager가 공유하는 기본 도구 세트를 생성합니다."""
    from shacs_bot.agent.tools.filesystem import (
        ReadFileTool,
        WriteFileTool,
        EditFileTool,
        ListDirTool,
    )
    from shacs_bot.agent.tools.history import SearchHistoryTool
    from shacs_bot.agent.tools.shell import ExecTool
    from shacs_bot.agent.tools.web import WebSearchTool, WebFetchTool

    allowed_dir: Path | None = workspace if restrict_to_workspace else None
    cfg: ExecToolConfig = exec_config or ExecToolConfig()

    return [
        ReadFileTool(workspace=workspace, allowed_dir=allowed_dir),
        WriteFileTool(workspace=workspace, allowed_dir=allowed_dir),
        EditFileTool(workspace=workspace, allowed_dir=allowed_dir),
        ListDirTool(workspace=workspace, allowed_dir=allowed_dir),
        ExecTool(
            working_dir=str(workspace),
            timeout=cfg.timeout,
            restrict_to_workspace=restrict_to_workspace,
            path_append=cfg.path_append,
        ),
        WebSearchTool(api_key=brave_api_key, proxy=web_proxy),
        WebFetchTool(proxy=web_proxy),
        SearchHistoryTool(workspace=workspace),
    ]
