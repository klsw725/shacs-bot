"""Heartbeat 서비스 - 작업을 확인하기 위해 에이전트를 주기적으로 깨우는 기능"""

import asyncio
from pathlib import Path
from typing import Callable, Any, Coroutine

from loguru import logger

from shacs_bot.agent.hooks import (
    BACKGROUND_JOB_COMPLETED,
    HEARTBEAT_DECIDED,
    HookContext,
    HookRegistry,
    NoOpHookRegistry,
)
from shacs_bot.providers.base import LLMProvider, LLMResponse
from shacs_bot.workflow.runtime import WorkflowRuntime


class HeartbeatService:
    """
    작업이 있는지 확인하기 위해 에이전트를 주기적으로 깨우는 heartbeat 서비스.

    1단계 (판단): HEARTBEAT.md를 읽고, 가상 도구 호출(virtual tool call)을 통해
    LLM에게 활성 작업이 있는지 묻는다. 이를 통해 자유 형식 텍스트 파싱이나
    신뢰하기 어려운 HEARTBEAT_OK 토큰에 의존하지 않게 된다.

    2단계 (실행): 1단계에서 `run`이 반환된 경우에만 실행된다.
    `on_execute` 콜백이 전체 에이전트 루프를 통해 작업을 실행하고,
    전달할 결과를 반환한다.
    """

    _HEARTBEAT_TOOL = [
        {
            "type": "function",
            "function": {
                "name": "heartbeat",
                "description": "Report heartbeat decision after reviewing tasks.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["skip", "run"],
                            "description": "skip = nothing to do, run = has active tasks",
                        },
                        "tasks": {
                            "type": "string",
                            "description": "Natural-language summary of active tasks (required for run)",
                        },
                    },
                    "required": ["action"],
                },
            },
        }
    ]

    def __init__(
        self,
        workspace: Path,
        provider: LLMProvider,
        model: str,
        on_execute: Callable[[str, str], Coroutine[Any, Any, str]] | None = None,
        on_notify: Callable[[str], Coroutine[Any, Any, None]] | None = None,
        interval_s: int = 30 * 60,
        enabled: bool = True,
        hooks: HookRegistry | None = None,
        workflow_runtime: WorkflowRuntime | None = None,
    ):
        self._workspace: Path = workspace
        self._provider: LLMProvider = provider
        self._model: str = model
        self._on_execute: Callable[[str, str], Coroutine[Any, Any, str]] | None = on_execute
        self._on_notify: Callable[[str], Coroutine[Any, Any, None]] | None = on_notify
        self._interval_s: int = interval_s
        self._enabled: bool = enabled
        self._hooks: HookRegistry = hooks or NoOpHookRegistry()
        self._workflow_runtime: WorkflowRuntime | None = workflow_runtime

        self._running: bool = False
        self._task: asyncio.Task[None] | None = None

    @property
    def heartbeat_file(self) -> Path:
        return self._workspace / "HEARTBEAT.md"

    async def start(self):
        """heartbeat 서비스 시작."""
        if not self._enabled:
            logger.info("Heartbeat 불가능")
            return

        if self._running:
            logger.warning("Heartbeat가 이미 동작 중입니다.")
            return

        self._running = True

        self._task = asyncio.create_task(self._run_loop())

        logger.info("Heartbeat가 시작되었습니다. ({}s 마다)", self._interval_s)

    async def _run_loop(self):
        """메인 heartbeat 루프"""
        while self._running:
            try:
                await asyncio.sleep(self._interval_s)
                if self._running:
                    await self._tick()
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error("Heartbeat 에러: {}", e)

    async def _tick(self):
        """싱글 heartbeat 틱 실행"""
        content: str | None = self._read_heartbeat_file()
        workflow_id: str = ""
        if not content:
            logger.debug("Heartbeat: Heartbeat.md가 없거나 비어있습니다.")

        logger.info("Heartbeat: 태스크 확인 중...")

        try:
            action, tasks = await self._decide(content or "")
            await self._hooks.emit(
                HookContext(
                    event=HEARTBEAT_DECIDED,
                    payload={"action": action, "tasks_preview": tasks[:100] if tasks else ""},
                )
            )
            if action != "run":
                logger.info("Heartbeat: OK (보고할께 없습니다)")
                return

            logger.info("Heartbeat: 태스크를 찾았습니다. 실행 중...")
            if self._on_execute:
                workflow_id = self._create_workflow(tasks)
                self._start_workflow(workflow_id)
                response: str = await self._on_execute(tasks, workflow_id)
                self._annotate_result(workflow_id, response)
                await self._hooks.emit(
                    HookContext(
                        event=BACKGROUND_JOB_COMPLETED,
                        payload={"result_preview": response[:120] if response else ""},
                    )
                )
                self._complete_workflow(workflow_id)
                if response and self._on_notify:
                    logger.info("Heartbeat: 완료됨, 응답을 전달합니다.")
                    await self._on_notify(response)
                    self._mark_notified(workflow_id)
        except Exception as exc:
            self._fail_workflow(workflow_id, str(exc))
            logger.exception("Heartbeat 실행 실패")

    def _create_workflow(self, tasks: str) -> str:
        if self._workflow_runtime is None:
            return ""
        record = self._workflow_runtime.store.create(
            source_kind="heartbeat",
            goal=tasks,
            metadata={"heartbeatFile": str(self.heartbeat_file)},
        )
        self._workflow_runtime.store.upsert(record)
        return record.workflow_id

    def _start_workflow(self, workflow_id: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.start(workflow_id)

    def _complete_workflow(self, workflow_id: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.complete(workflow_id)

    def _annotate_result(self, workflow_id: str, result: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.annotate_result(workflow_id, result)

    def _mark_notified(self, workflow_id: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.mark_notify_delegated(workflow_id)

    def _fail_workflow(self, workflow_id: str, error: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.fail(workflow_id, last_error=error)

    def _read_heartbeat_file(self) -> str | None:
        if self.heartbeat_file.exists():
            try:
                return self.heartbeat_file.read_text(encoding="utf-8")
            except Exception:
                return None

        return None

    async def _decide(self, content: str) -> tuple[str, str]:
        """
        1단계: 가상 도구 호출(virtual tool call)을 통해 LLM에게 skip/run 여부를 결정하도록 요청한다.

        (action, tasks)를 반환하며, action 값은 'skip' 또는 'run'이다.
        """
        response: LLMResponse = await self._provider.chat(
            messages=[
                {
                    "role": "system",
                    "content": "당신은 하트비트 에이전트입니다. 결정을 보고하기 위해 heartbeat 도구를 호출하세요.",
                },
                {
                    "role": "user",
                    "content": (
                        "다음 HEARTBEAT.md 내용을 검토하고 활성 작업이 있는지 판단하세요.\n\n"
                        f"{content}"
                    ),
                },
            ],
            tools=self._HEARTBEAT_TOOL,
            model=self._model,
        )
        if not response.has_tool_calls:
            return "skip", ""

        args: dict[str, Any] = response.tool_calls[0].arguments
        return args.get("action", "skip"), args.get("tasks", "")

    def stop(self):
        """heartbeat 서비스 정지"""
        self._running = False
        if self._task:
            self._task.cancel()
            self._task = None

    async def trigger_now(self) -> str | None:
        """수동으로 하트비트를 트리거합니다."""
        content: str | None = self._read_heartbeat_file()
        if not content:
            return None

        action, tasks = await self._decide(content)
        if action != "run" or not self._on_execute:
            return None

        workflow_id = self._create_workflow(tasks)
        return await self._on_execute(tasks, workflow_id)
