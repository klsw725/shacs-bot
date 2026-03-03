"""백그라운드 테스크 실행을 위한 Subagent 관리자"""
import asyncio
import json
import uuid
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.agent.context import ContextBuilder
from shacs_bot.agent.skills import SkillsLoader
from shacs_bot.agent.tools.filesystem import ReadFileTool, WriteFileTool, EditFileTool, ListDirTool
from shacs_bot.agent.tools.registry import ToolRegistry
from shacs_bot.agent.tools.shell import ExecTool
from shacs_bot.agent.tools.web import WebSearchTool, WebFetchTool
from shacs_bot.bus.events import InboundMessage
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
            reasoning_effort: str | None = None,
            brave_api_key: str | None = None,
            web_proxy: str | None = None,
            exec_config: ExecToolConfig | None = None,
            restrict_to_workspace: bool = False,
    ):
        self._provider = provider
        self._workspace = workspace
        self._network = network
        self._model = model or provider.get_default_model()
        self._temperature = temperature
        self._max_tokens = max_tokens
        self._reasoning_effort = reasoning_effort
        self._brave_api_key = brave_api_key
        self._web_proxy = web_proxy
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
                ReadFileTool(workspace=self._workspace, allowed_dir=allowed_dir)
            )
            tools.register(
                WriteFileTool(workspace=self._workspace, allowed_dir=allowed_dir)
            )
            tools.register(
                EditFileTool(workspace=self._workspace, allowed_dir=allowed_dir)
            )
            tools.register(
                ListDirTool(workspace=self._workspace, allowed_dir=allowed_dir)
            )
            tools.register(ExecTool(
                working_dir=str(self._workspace),
                timeout=self._exec_config.timeout,
                restrict_to_workspace=self._restrict_to_workspace,
                path_append=self._exec_config.path_append
            ))
            tools.register(WebSearchTool(
                api_key=self._brave_api_key,
                proxy=self._web_proxy
            ))
            tools.register(WebFetchTool(proxy=self._web_proxy))

            system_prompt: str = self._build_subagent_prompt()
            messages: list[dict[str, Any]] = [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": task
                }
            ]

            # 에이전트 루프 동작 (반복 최대 횟수)
            max_iterations: int = 15
            final_result: str | None = None

            for iter in range(max_iterations):
                response = await self._provider.chat(
                    messages=messages,
                    tools=tools.get_definitions(),
                    model=self._model,
                    temperature=self._temperature,
                    max_tokens=self._max_tokens,
                    reasoning_effort = self._reasoning_effort
                )
                if response.has_tool_calls:
                    # 도구 호출 어시스턴트 메시지 추가
                    tool_call_dicts: list[dict[str, Any]] = [
                        {
                            "id": tool_call.id,
                            "type": "function",
                            "function": {
                                "name": tool_call.name,
                                "arguments": json.dumps(tool_call.arguments, ensure_ascii=False),
                            },
                        }
                        for tool_call in response.tool_calls
                    ]
                    messages.append({
                        "role": "assistant",
                        "content": response.content or "",
                        "tool_calls": tool_call_dicts
                    })

                    # Execute tools
                    for tool_call in response.tool_calls:
                        args_str: str = json.dumps(tool_call.arguments, ensure_ascii=False)

                        logger.debug("서브에이전트 [{}] 실행: {} 인자: {}", task_id, tool_call.name, args_str)
                        result: str = await tools.execute(tool_call.name, tool_call.arguments)
                        messages.append({
                            "role": "tool",
                            "tool_call_id": tool_call.id,
                            "name": tool_call.name,
                            "content": result
                        })

                else:
                    final_result = response.content
                    break

                if final_result is None:
                    final_result = "작업은 완료되었지만 최종 응답이 생성되지 않았습니다."

                logger.info("서브에이전트 [{}]가 성공적으로 완료되었습니다", task_id)
                await self._announce_result(task_id=task_id, label=label, task=task, result=final_result, origin=origin, status="ok")
        except Exception as e:
            error_msg: str = f"Error: {str(e)}"
            logger.error("서브에이전트 [{}] 실패: {}", task_id, e)
            await self._announce_result(task_id=task_id, label=label, task=task, result=error_msg, origin=origin, status="error")

    def _build_subagent_prompt(self) -> str:
        """하위 에이전트를 위한 목적에 맞는 시스템 프롬프트를 작성"""
        time_ctx:str = ContextBuilder._build_runtime_context(None, None)
        parts: list[str] = [f"""
            # Subagent
            
            {time_ctx}
            
            당신은 메인 에이전트에 의해 특정 작업을 수행하기 위해 생성된 서브에이전트입니다.
            할당된 작업에 집중하세요. 당신의 최종 응답은 메인 에이전트에게 보고됩니다.
            ## Workspace
            {self._workspace}
        """]

        skills_summary: str = SkillsLoader(self._workspace).build_skills_summary()
        if skills_summary:
           parts.append(f"## 스킬을 사용하려면 read_file로 SKILL.md를 읽으세요.\n\n{skills_summary}")

        return "\n\n".join(parts)

    async def _announce_result(
            self,
            task_id: str,
            label: str,
            task: str,
            result: str,
            origin: dict[str, str],
            status: str
    ) -> None:
        """내부 네트워크를 통해 서브에이전트의 결과를 메인 에이전트에 알립니다."""
        status_text: str = "성공적으로 완료되었습니다." if status == "ok" else "failed"
        announce_content = f"""
            [Subagent '{label}' {status_text}]
    
            Task: {task}
            
            Result:
            {result}
            
            이 내용을 사용자에게 자연스럽게 요약하세요. 간단하게 1~2문장으로 작성하고, “subagent”나 작업 ID 같은 기술적인 세부 사항은 언급하지 마세요. 
        """

        # 메인 에이전트를 트리거하기 위해 시스템 메시지로 주입
        await self._network.publish_inbound(InboundMessage(
            channel="system",
            sender_id="subagent",
            chat_id=f"{origin['channel']}:{origin['chat_id']}",
            content=announce_content
        ))
        logger.debug("Subagent [{}]가 {}:{}에 결과를 알렸습니다.", task_id, origin['channel'], origin['chat_id'])

    async def cancel_by_session(self, session_key: str) -> int:
        """주어진 세션에 속한 모든 서브에이전트를 취소하고, 취소된 개수를 반환합니다."""
        tasks: list[asyncio.Task[None]] = [self._running_tasks[tid] for tid in self._session_tasks.get(session_key, [])
                                        if (tid in self._running_tasks) and not self._running_tasks[tid].done()]
        for task in tasks:
            task.cancel()

        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)

        return len(tasks)

    def get_running_count(self) -> int:
        """현재 실행 중인 서브에이전트의 수를 반환하라."""
        return len(self._running_tasks)