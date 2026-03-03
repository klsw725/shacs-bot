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
            "task": {
                "type": "string",
                "description": "서브에이전트가 완료할 작업"
            },
            "label": {
                "type": "string",
                "description": "작업에 대한 선택적 짧은 레이블(표시용)"
            },
        },
        "required": ["task"]
    }

    def __init__(
            self,
            manager: SubagentManager
    ):
        self._manager = manager
        self._original_channel = "cli"
        self._original_chat_id = "direct"
        self._session_key = "cli:direct"

    def set_context(self, channel: str, chat_id: str) -> None:
        """서브에이전트 알림을 위한 원본 컨텍스트를 설정합니다."""
        self._original_channel = channel
        self._original_chat_id = chat_id
        self._session_key = f"{channel}:{chat_id}"

    async def execute(self, task: str, label: str | None = None, **kwargs: Any) -> str:
        """주어진 작업을 실행하기 위해 서브에이전트를 생성합니다."""
        return await self._manager.spawn(
            task=task,
            label=label,
            origin_channel=self._original_channel,
            origin_chat_id=self._original_chat_id,
            session_key=self._session_key,
        )