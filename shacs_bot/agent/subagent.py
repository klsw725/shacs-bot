"""백그라운드 테스크 실행을 위한 Subagent 관리자"""
import asyncio
import uuid
from pathlib import Path

from loguru import logger

from shacs_bot.agent.tools.filesystem import ReadFileTool
from shacs_bot.agent.tools.registry import ToolRegistry
from shacs_bot.bus.networks import MessageBus
from shacs_bot.config.schema import ExecToolConfig
from shacs_bot.providers.base import LLMProvider


class SubagentManager:
    """백그라운드 subagent 관리를 담당하는 클래스입니다."""

    def __init__(
            self,
            provider: LLMProvider,
            workspace: Path,
            network: MessageBus,
            model: str | None = None,
            temperature: float = 0.7,
            max_tokens: int = 4096,
            brave_api_key: str | None = None,
            exec_config: ExecToolConfig | None = None,
            restrict_to_workspace: bool = False,
    ):
        self._provider = provider
        self._workspace = workspace
        self._network = network
        self._model = model or provider.get_default_model()
        self._temperature = temperature
        self._max_tokens = max_tokens
        self._brave_api_key = brave_api_key
        self._exec_config = exec_config or ExecToolConfig()
        self._restrict_to_workspace = restrict_to_workspace

        self._running_tasks: dict[str, asyncio.Task[None]] = {}
        self._session_tasks: dict[str, set[str]] = {}   # session_key -> {task_id, ...}

    async def spawn(
            self,
            task: str,
            label: str | None = None,
            origin_channel: str = "cli",
            origin_chat_id: str = "direct",
            session_key: str | None = None,
    ) -> str:
        """새로운 서브에이전트를 생성하여 주어진 작업을 실행합니다."""
        task_id: str = str(uuid.uuid4())[:8]
        display_label: str = label or task[:30] + ("..." if len(task) > 30 else "")
        origin: dict[str, str] = {"channel": origin_channel, "chat_id": origin_chat_id}

        bg_task = asyncio.create_task(
            self._run_subagent(task_id, task, display_label, origin)
        )
        self._running_tasks[task_id] = bg_task

        if session_key:
            self._session_tasks.setdefault(session_key, set()).add(task_id)

        def _cleanup(t: asyncio.Task) -> None:
            self._running_tasks.pop(task_id, None)
            if session_key and (ids := self._session_tasks.get(session_key)):
                ids.discard(task_id)
                if not ids:
                    del self._session_tasks[session_key]

        bg_task.add_done_callback(_cleanup)

        logger.info(f"서브에이전트 [{task_id}] 생성됨: {display_label}")
        return f"서브에이전트 [{display_label}]이(가) 시작되었습니다 (id: {task_id}). 완료되면 알려드리겠습니다."

    async def _run_subagent(
            self,
            task_id: str,
            task: str,
            label: str,
            origin: dict[str, str],
    ) -> None:
        """서브에이전트를 실행합니다. 그리고 완료되면 결과를 보고합니다."""
        logger.info(f"서브에이전트 [{task_id}] 실행 시작: {label}")

        try:
            # 서브에이전트 도구를 설정합니다. (no message tool, no spawn tool)
            allowed_dir: Path | None = self._workspace if self._restrict_to_workspace else None

            tools: ToolRegistry = ToolRegistry()
            tools.register(
                ReadFileTool(allowed_dir=self._workspace)
            )

        except Exception as e:
            error_msg: str = f"Error: {str(e)}"