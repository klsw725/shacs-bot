"""백그라운드 서브에이전트를 생성하기 위한 Spawn 도구."""

from pathlib import Path
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
            "skill_path": {
                "type": "string",
                "description": "실행할 스킬의 SKILL.md 경로. 스킬 사용 시 반드시 지정.",
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
        self,
        task: str,
        label: str | None = None,
        role: str = "executor",
        skill_path: str | None = None,
        **kwargs: Any,
    ) -> str:
        if skill_path:
            skill_name: str = Path(skill_path).parent.name
            return await self._manager.spawn_skill(
                task=task,
                label=label or skill_name,
                skill_name=skill_name,
                skill_path=skill_path,
                origin_channel=self._original_channel,
                origin_chat_id=self._original_chat_id,
                session_key=self._session_key,
                origin_metadata=self._original_metadata,
            )
        return await self._manager.spawn(
            task=task,
            label=label,
            role=role,
            origin_channel=self._original_channel,
            origin_chat_id=self._original_chat_id,
            session_key=self._session_key,
            origin_metadata=self._original_metadata,
        )


class ListTasksTool(Tool):
    name = "list_tasks"
    description = "현재 백그라운드에서 실행 중인 작업 목록을 조회합니다."
    parameters = {
        "type": "object",
        "properties": {},
        "required": [],
    }

    def __init__(self, manager: SubagentManager):
        self._manager = manager
        self._session_key = "cli:direct"

    def set_context(
        self,
        channel: str,
        chat_id: str,
        metadata: dict[str, Any] | None = None,
        session_key: str | None = None,
    ) -> None:
        self._session_key = session_key or f"{channel}:{chat_id}"

    async def execute(self, **kwargs: Any) -> str:
        tasks = self._manager.list_running_tasks(session_key=self._session_key)
        if not tasks:
            return "현재 실행 중인 백그라운드 작업이 없습니다."

        lines: list[str] = []
        for t in tasks:
            lines.append(f"- [{t['task_id']}] {t['label']} ({t['role']}) - {t['elapsed']} 경과")
        return f"실행 중인 백그라운드 작업 {len(tasks)}개:\n" + "\n".join(lines)


class CancelTaskTool(Tool):
    name = "cancel_task"
    description = "특정 백그라운드 작업을 중지합니다. task_id는 list_tasks로 확인할 수 있습니다."
    parameters = {
        "type": "object",
        "properties": {
            "task_id": {"type": "string", "description": "중지할 작업의 ID"},
        },
        "required": ["task_id"],
    }

    def __init__(self, manager: SubagentManager):
        self._manager = manager

    async def execute(self, task_id: str, **kwargs: Any) -> str:
        success, label = await self._manager.cancel_task(task_id)
        if success:
            return f"작업 '{label}'이(가) 중지되었습니다."
        return "해당 작업을 찾을 수 없거나 이미 완료되었습니다."
