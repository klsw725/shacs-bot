"""백그라운드 서브에이전트를 생성하기 위한 Spawn 도구."""

from typing import Any

from shacs_bot.agent.subagent import SubagentManager
from shacs_bot.agent.tools.base import Tool


class SpawnTool(Tool):
    """백그라운드 태스크 실행을 위해 서브에이전트를 생성하는 도구입니다."""

    name = "spawn"
    description = """
        백그라운드에서 작업을 처리할 서브에이전트를 생성합니다.
        독립적으로 실행할 수 있는 복잡하거나 시간이 오래 걸리는 작업에 사용하세요.
        서브에이전트는 작업을 완료한 후 결과를 보고합니다.
    """
    parameters = {
        "type": "object",
        "properties": {
            "task": {"type": "string", "description": "서브에이전트가 완료할 작업"},
            "label": {"type": "string", "description": "작업에 대한 선택적 짧은 레이블(표시용)"},
            "role": {
                "type": "string",
                "description": "작업 목적에 맞는 역할을 반드시 지정하세요. analyst: 파일/데이터를 분석하거나 요약할 때 (읽기 전용). researcher: 웹에서 정보를 찾거나 조사할 때 (읽기 전용). executor: 파일을 생성/수정하거나 명령을 실행할 때.",
                "enum": ["researcher", "analyst", "executor"],
            },
        },
        "required": ["task"],
    }

    def __init__(self, manager: SubagentManager):
        self._manager = manager
        self._original_channel = "cli"
        self._original_chat_id = "direct"
        self._session_key = "cli:direct"
        self._original_metadata: dict[str, Any] = {}

    def set_context(
        self,
        channel: str,
        chat_id: str,
        metadata: dict[str, Any] | None = None,
        session_key: str | None = None,
    ) -> None:
        """서브에이전트 알림을 위한 원본 컨텍스트를 설정합니다."""
        self._original_channel = channel
        self._original_chat_id = chat_id
        self._session_key = session_key or f"{channel}:{chat_id}"
        self._original_metadata = metadata or {}

    async def execute(
        self, task: str, label: str | None = None, role: str = "executor", **kwargs: Any
    ) -> str:
        return await self._manager.spawn(
            task=task,
            label=label,
            role=role,
            origin_channel=self._original_channel,
            origin_chat_id=self._original_chat_id,
            session_key=self._session_key,
            origin_metadata=self._original_metadata,
        )
