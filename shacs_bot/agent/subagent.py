"""백그라운드 테스크 실행을 위한 Subagent 관리자"""

import asyncio
import json
import uuid
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.agent.agents import BUILTIN_AGENTS, AgentDefinition, AgentRegistry
from shacs_bot.agent.approval import ALWAYS_ALLOW, ApprovalGate
from shacs_bot.agent.context import ContextBuilder
from shacs_bot.agent.hooks import HookRegistry, NoOpHookRegistry
from shacs_bot.agent.skills import SkillsLoader
from shacs_bot.agent.tools.registry import ToolRegistry, create_default_tools
from shacs_bot.bus.events import InboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.config.schema import ExecToolConfig
from shacs_bot.providers.base import LLMProvider, LLMResponse
from shacs_bot.utils.helpers import build_assistant_message
from shacs_bot.workflow.runtime import WorkflowRuntime


@dataclass
class SubagentTask:
    task_id: str
    workflow_id: str
    label: str
    role: str
    original_task: str
    started_at: datetime
    asyncio_task: asyncio.Task[None]


class SubagentManager:
    """백그라운드 subagent 관리를 담당하는 클래스입니다."""

    def __init__(
        self,
        provider: LLMProvider,
        workspace: Path,
        bus: MessageBus,
        model: str | None = None,
        brave_api_key: str | None = None,
        web_proxy: str | None = None,
        exec_config: ExecToolConfig | None = None,
        restrict_to_workspace: bool = False,
        max_threads: int = 6,
        agent_registry: AgentRegistry | None = None,
        hooks: HookRegistry | None = None,
        workflow_runtime: WorkflowRuntime | None = None,
    ):
        self._provider: LLMProvider = provider
        self._workspace: Path = workspace
        self._bus: MessageBus = bus
        self._model: str = model or provider.get_default_model()
        self._brave_api_key: str | None = brave_api_key
        self._web_proxy: str | None = web_proxy
        self._exec_config: ExecToolConfig = exec_config or ExecToolConfig()
        self._restrict_to_workspace: bool = restrict_to_workspace
        self._max_threads: int = max_threads
        self._registry: AgentRegistry | None = agent_registry
        self._hooks: HookRegistry = hooks or NoOpHookRegistry()
        self._workflow_runtime: WorkflowRuntime | None = workflow_runtime

        self._skill_approval: str = "auto"
        self._running_tasks: dict[str, SubagentTask] = {}
        self._session_tasks: dict[str, set[str]] = {}  # session_key -> {task_id, ...}

    @property
    def skill_approval(self) -> str:
        return self._skill_approval

    @skill_approval.setter
    def skill_approval(self, value: str) -> None:
        self._skill_approval = value

    def _check_threads(self) -> str | None:
        """동시 실행 제한 체크. 초과 시 에러 메시지 반환, 여유 있으면 None."""
        running = sum(1 for t in self._running_tasks.values() if not t.asyncio_task.done())
        if running >= self._max_threads:
            return f"동시 실행 제한 초과 (현재 {running}/{self._max_threads}개). 기존 작업이 완료된 후 다시 시도하세요."
        return None

    def _resolve_agent(self, role: str) -> AgentDefinition:
        """role 이름으로 에이전트 정의를 조회한다. 없으면 executor 폴백."""
        if self._registry:
            agent_def = self._registry.get(role)
            if agent_def:
                return agent_def
        # 레지스트리 없거나 못 찾으면 built-in 폴백
        return BUILTIN_AGENTS.get(role, BUILTIN_AGENTS["executor"])

    async def spawn(
        self,
        task: str,
        label: str | None = None,
        role: str = "executor",
        origin_channel: str = "cli",
        origin_chat_id: str = "direct",
        session_key: str | None = None,
        origin_metadata: dict[str, Any] | None = None,
        workflow_id: str | None = None,
    ) -> str:
        """새로운 서브에이전트를 생성하여 주어진 작업을 실행합니다."""
        # 동시성 제한
        if err := self._check_threads():
            return err

        task_id: str = str(uuid.uuid4())[:8]
        display_label: str = label or task[:30] + ("..." if len(task) > 30 else "")
        origin: dict[str, Any] = {
            "channel": origin_channel,
            "chat_id": origin_chat_id,
            "metadata": origin_metadata or {},
            "session_key": session_key,
        }

        agent_def: AgentDefinition = self._resolve_agent(role)
        active_workflow_id: str = workflow_id or self._create_workflow(
            task_id=task_id,
            role=role,
            task=task,
            origin=origin,
            label=display_label,
            extra_metadata={},
        )

        bg_task: asyncio.Task[None] = asyncio.create_task(
            self._run_subagent(
                task_id,
                active_workflow_id,
                task,
                display_label,
                origin,
                agent_def=agent_def,
            )
        )
        self._running_tasks[task_id] = SubagentTask(
            task_id=task_id,
            workflow_id=active_workflow_id,
            label=display_label,
            role=role,
            original_task=task[:100],
            started_at=datetime.now(),
            asyncio_task=bg_task,
        )

        if session_key:
            self._session_tasks.setdefault(session_key, set()).add(task_id)

        def _cleanup(t: asyncio.Task[None]) -> None:
            self._running_tasks.pop(task_id, None)
            if session_key and (ids := self._session_tasks.get(session_key)):
                ids.discard(task_id)
                if not ids:
                    del self._session_tasks[session_key]

        bg_task.add_done_callback(_cleanup)

        logger.info(
            "서브에이전트 [{}] 생성됨: {} (역할: {}, 모델: {})",
            task_id,
            display_label,
            role,
            agent_def.model or "기본",
        )
        return f"서브에이전트 [{display_label}]이(가) 시작되었습니다 (id: {task_id}). 완료되면 알려드리겠습니다."

    async def spawn_skill(
        self,
        task: str,
        label: str,
        skill_name: str,
        skill_path: str,
        origin_channel: str = "cli",
        origin_chat_id: str = "direct",
        session_key: str | None = None,
        origin_metadata: dict[str, Any] | None = None,
        workflow_id: str | None = None,
    ) -> str:
        """스킬을 서브에이전트로 실행한다."""
        # 동시성 제한
        if err := self._check_threads():
            return err

        task_id: str = str(uuid.uuid4())[:8]
        origin: dict[str, Any] = {
            "channel": origin_channel,
            "chat_id": origin_chat_id,
            "metadata": origin_metadata or {},
            "session_key": session_key,
        }
        active_workflow_id: str = workflow_id or self._create_workflow(
            task_id=task_id,
            role="skill",
            task=task,
            origin=origin,
            label=label,
            extra_metadata={"skillName": skill_name, "skillPath": skill_path},
        )

        bg_task: asyncio.Task[None] = asyncio.create_task(
            self._run_skill(
                task_id, active_workflow_id, task, label, origin, skill_name, skill_path
            )
        )
        self._running_tasks[task_id] = SubagentTask(
            task_id=task_id,
            workflow_id=active_workflow_id,
            label=label,
            role="skill",
            original_task=task[:100],
            started_at=datetime.now(),
            asyncio_task=bg_task,
        )

        if session_key:
            self._session_tasks.setdefault(session_key, set()).add(task_id)

        def _cleanup(t: asyncio.Task[None]) -> None:
            self._running_tasks.pop(task_id, None)
            if session_key and (ids := self._session_tasks.get(session_key)):
                ids.discard(task_id)
                if not ids:
                    del self._session_tasks[session_key]

        bg_task.add_done_callback(_cleanup)

        logger.info("스킬 서브에이전트 [{}] 생성됨: {} ({})", task_id, label, skill_name)
        return f"스킬 [{label}]을(를) 실행합니다 (id: {task_id}). 완료되면 알려드리겠습니다."

    async def _run_skill(
        self,
        task_id: str,
        workflow_id: str,
        task: str,
        label: str,
        origin: dict[str, Any],
        skill_name: str,
        skill_path: str,
    ) -> None:
        """서브에이전트로 스킬을 실행한다. workspace 스킬은 승인 게이트 적용."""
        logger.info("스킬 서브에이전트 [{}] 실행 시작: {} ({})", task_id, label, skill_name)

        try:
            self._start_workflow(workflow_id)
            # 1. 스킬 내용 로드
            skill_content: str = Path(skill_path).expanduser().read_text(encoding="utf-8")

            # 2. 출처 확인 → 승인 모드 결정
            source: str | None = SkillsLoader(self._workspace).get_skill_source(skill_name)
            effective_mode: str = self._skill_approval

            # 비대화형 채널(cron, system)에서 manual → auto 폴백
            if effective_mode == "manual" and origin.get("channel") in ("cron", "system"):
                effective_mode = "auto"
                logger.info("비대화형 채널에서 manual → auto 폴백 (스킬: {})", skill_name)

            needs_approval: bool = (source != "builtin") and (effective_mode != "off")

            # 3. 도구 생성 — spawn 도구 제외 (재귀 방지)
            all_tools = create_default_tools(
                workspace=self._workspace,
                restrict_to_workspace=self._restrict_to_workspace,
                exec_config=self._exec_config,
                brave_api_key=self._brave_api_key,
                web_proxy=self._web_proxy,
            )
            tools: ToolRegistry = ToolRegistry(hooks=self._hooks)
            for tool in all_tools:
                tools.register(tool)

            # 4. 승인 게이트 (workspace 스킬 + auto/manual)
            approval_gate: ApprovalGate | None = None
            if needs_approval:
                # 세션 히스토리를 가져와서 reasoning-blind 분류기에 전달
                from shacs_bot.agent.session.manager import SessionManager

                sessions = SessionManager(self._workspace)
                session_key: str | None = origin.get("session_key")
                session_history: list[dict[str, Any]] = []
                if session_key:
                    session = sessions.get_or_create(key=session_key)
                    session_history = session.get_history(max_messages=20)

                approval_gate = ApprovalGate(
                    mode=effective_mode,
                    provider=self._provider,
                    model=self._model,
                    session_history=session_history,
                    bus=self._bus,
                    origin=origin,
                    skill_name=skill_name,
                    workspace=self._workspace,
                    hooks=self._hooks,
                )

            # 5. 시스템 프롬프트 구성
            system_prompt: str = self._build_skill_prompt(skill_content, skill_name)

            # 6. chat loop
            messages: list[dict[str, Any]] = [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": task},
            ]
            final_result: str | None = None
            max_iterations: int = 15

            for _ in range(max_iterations):
                response: LLMResponse = await self._provider.chat_with_retry(
                    messages=messages,
                    tools=tools.get_definitions(),
                    model=self._model,
                )
                if response.has_tool_calls:
                    tool_call_dicts: list[dict[str, Any]] = [
                        tc.to_openai_tool_call() for tc in response.tool_calls
                    ]
                    messages.append(
                        build_assistant_message(
                            content=response.content or "",
                            tool_calls=tool_call_dicts,
                            reasoning_content=response.reasoning_content,
                            thinking_blocks=response.thinking_blocks,
                        )
                    )

                    for tool_call in response.tool_calls:
                        # 승인 게이트 검사 (workspace 스킬 + auto/manual)
                        if approval_gate and tool_call.name not in ALWAYS_ALLOW:
                            decision = await approval_gate.check(
                                tool_call.name,
                                tool_call.arguments,
                            )
                            if decision.denied:
                                logger.info(
                                    "스킬 서브에이전트 [{}] 도구 거부: {} ({})",
                                    task_id,
                                    tool_call.name,
                                    decision.reason,
                                )
                                messages.append(
                                    {
                                        "role": "tool",
                                        "tool_call_id": tool_call.id,
                                        "name": tool_call.name,
                                        "content": f"[DENIED] {decision.reason}",
                                    }
                                )
                                continue

                        args_str: str = json.dumps(tool_call.arguments, ensure_ascii=False)
                        logger.debug(
                            "스킬 서브에이전트 [{}] 실행: {} 인자: {}",
                            task_id,
                            tool_call.name,
                            args_str,
                        )
                        result: str = await tools.execute(tool_call.name, tool_call.arguments)
                        messages.append(
                            {
                                "role": "tool",
                                "tool_call_id": tool_call.id,
                                "name": tool_call.name,
                                "content": result,
                            }
                        )
                else:
                    final_result = response.content
                    break

            if final_result is None:
                final_result = self._extract_partial_progress(messages, max_iterations)

            logger.info("스킬 서브에이전트 [{}] 완료", task_id)
            self._annotate_result(workflow_id, final_result)
            self._complete_workflow(workflow_id)
            await self._announce_result(
                workflow_id=workflow_id,
                task_id=task_id,
                label=label,
                task=task,
                result=final_result,
                origin=origin,
                status="ok",
            )
        except asyncio.CancelledError:
            self._fail_workflow(workflow_id, "cancelled")
            raise
        except Exception as e:
            self._fail_workflow(workflow_id, str(e))
            logger.error("스킬 서브에이전트 [{}] 실패: {}", task_id, e)
            await self._announce_result(
                workflow_id=workflow_id,
                task_id=task_id,
                label=label,
                task=task,
                result=f"Error: {e}",
                origin=origin,
                status="error",
            )

    def _build_skill_prompt(self, skill_content: str, skill_name: str) -> str:
        """스킬 서브에이전트의 시스템 프롬프트를 구성한다."""
        time_ctx: str = ContextBuilder.build_runtime_context(None, None)
        return (
            f"당신은 '{skill_name}' 스킬을 실행하는 에이전트입니다.\n\n"
            f"## 환경\n{time_ctx}\n\n"
            f"## Workspace\n{self._workspace}\n\n"
            f"## 스킬 내용\n아래 스킬의 지시사항에 따라 작업을 수행하세요.\n\n"
            f"---\n{skill_content}\n---\n\n"
            f"## 제약\n"
            f"- 할당된 작업에만 집중하세요.\n"
            f"- 위험한 명령(rm -rf, format 등)은 실행하지 마세요.\n"
            f"- 작업 완료 후 결과를 명확히 보고하세요."
        )

    async def _run_subagent(
        self,
        task_id: str,
        workflow_id: str,
        task: str,
        label: str,
        origin: dict[str, Any],
        agent_def: AgentDefinition | None = None,
    ) -> None:
        """서브에이전트를 실행합니다. AgentDefinition 기반 모델/도구 사용."""
        if agent_def is None:
            agent_def = BUILTIN_AGENTS["executor"]

        model: str = agent_def.model or self._model
        logger.info(
            "서브에이전트 [{}] 실행 시작: {} (에이전트: {}, 모델: {})",
            task_id,
            label,
            agent_def.name,
            model,
        )

        try:
            self._start_workflow(workflow_id)
            all_tools = create_default_tools(
                workspace=self._workspace,
                restrict_to_workspace=self._restrict_to_workspace,
                exec_config=self._exec_config,
                brave_api_key=self._brave_api_key,
                web_proxy=self._web_proxy,
            )

            # 도구 필터링: allowed_tools 또는 sandbox_mode 기반
            effective_tools: list[str] = agent_def.get_effective_tools()
            tools: ToolRegistry = ToolRegistry(hooks=self._hooks)
            for tool in all_tools:
                if not effective_tools or tool.name in effective_tools:
                    tools.register(tool)

            # 에이전트별 MCP 연결 (M4)
            from contextlib import AsyncExitStack

            mcp_stack: AsyncExitStack | None = None
            if agent_def.mcp_servers:
                from shacs_bot.agent.tools.mcp import connect_mcp_servers

                mcp_stack = AsyncExitStack()
                await mcp_stack.__aenter__()
                try:
                    await connect_mcp_servers(agent_def.mcp_servers, tools, mcp_stack)
                except Exception as e:
                    logger.warning(
                        "에이전트 [{}] MCP 연결 실패, MCP 없이 계속: {}", agent_def.name, e
                    )

            # workspace 에이전트 ApprovalGate (M3)
            approval_gate: ApprovalGate | None = None
            if agent_def.source == "workspace" and self._skill_approval != "off":
                effective_mode: str = self._skill_approval
                if effective_mode == "manual" and origin.get("channel") in ("cron", "system"):
                    effective_mode = "auto"

                from shacs_bot.agent.session.manager import SessionManager

                sessions = SessionManager(self._workspace)
                session_key: str | None = origin.get("session_key")
                session_history: list[dict[str, Any]] = []
                if session_key:
                    session = sessions.get_or_create(key=session_key)
                    session_history = session.get_history(max_messages=20)

                approval_gate = ApprovalGate(
                    mode=effective_mode,
                    provider=self._provider,
                    model=self._model,
                    session_history=session_history,
                    bus=self._bus,
                    origin=origin,
                    skill_name=agent_def.name,
                    workspace=self._workspace,
                    hooks=self._hooks,
                )

            system_prompt: str = self._build_subagent_prompt(agent_def)
            messages: list[dict[str, Any]] = [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": task},
            ]

            max_iterations: int = agent_def.max_iterations
            final_result: str | None = None

            try:
                for iteration in range(max_iterations):
                    response: LLMResponse = await self._provider.chat_with_retry(
                        messages=messages,
                        tools=tools.get_definitions(),
                        model=model,
                    )
                    if response.has_tool_calls:
                        tool_call_dicts: list[dict[str, Any]] = [
                            tool_call.to_openai_tool_call() for tool_call in response.tool_calls
                        ]
                        messages.append(
                            build_assistant_message(
                                content=response.content or "",
                                tool_calls=tool_call_dicts,
                                reasoning_content=response.reasoning_content,
                                thinking_blocks=response.thinking_blocks,
                            )
                        )

                        for tool_call in response.tool_calls:
                            # workspace 에이전트 ApprovalGate 검사
                            if approval_gate and tool_call.name not in ALWAYS_ALLOW:
                                decision = await approval_gate.check(
                                    tool_call.name,
                                    tool_call.arguments,
                                )
                                if decision.denied:
                                    logger.info(
                                        "서브에이전트 [{}] 도구 거부: {} ({})",
                                        task_id,
                                        tool_call.name,
                                        decision.reason,
                                    )
                                    messages.append(
                                        {
                                            "role": "tool",
                                            "tool_call_id": tool_call.id,
                                            "name": tool_call.name,
                                            "content": f"[DENIED] {decision.reason}",
                                        }
                                    )
                                    continue

                            args_str: str = json.dumps(tool_call.arguments, ensure_ascii=False)
                            logger.debug(
                                "서브에이전트 [{}] 실행: {} 인자: {}",
                                task_id,
                                tool_call.name,
                                args_str,
                            )
                            result: str = await tools.execute(tool_call.name, tool_call.arguments)
                            messages.append(
                                {
                                    "role": "tool",
                                    "tool_call_id": tool_call.id,
                                    "name": tool_call.name,
                                    "content": result,
                                }
                            )
                    else:
                        final_result = response.content
                        break
            finally:
                # MCP 연결 정리
                if mcp_stack:
                    await mcp_stack.aclose()

            if final_result is None:
                final_result = self._extract_partial_progress(messages, max_iterations)

            logger.info("서브에이전트 [{}]가 성공적으로 완료되었습니다", task_id)
            self._annotate_result(workflow_id, final_result)
            self._complete_workflow(workflow_id)
            await self._announce_result(
                workflow_id=workflow_id,
                task_id=task_id,
                label=label,
                task=task,
                result=final_result,
                origin=origin,
                status="ok",
            )
        except asyncio.CancelledError:
            self._fail_workflow(workflow_id, "cancelled")
            raise
        except Exception as e:
            error_msg: str = f"Error: {str(e)}"
            self._fail_workflow(workflow_id, str(e))
            logger.error("서브에이전트 [{}] 실패: {}", task_id, e)
            await self._announce_result(
                workflow_id=workflow_id,
                task_id=task_id,
                label=label,
                task=task,
                result=error_msg,
                origin=origin,
                status="error",
            )

    def _build_subagent_prompt(self, agent_def: AgentDefinition) -> str:
        """AgentDefinition 기반 시스템 프롬프트를 작성"""
        time_ctx: str = ContextBuilder.build_runtime_context(None, None)
        parts: list[str] = [
            agent_def.developer_instructions,
            f"\n## 환경\n{time_ctx}\n\n## Workspace\n{self._workspace}",
        ]

        skills_summary: str = SkillsLoader(self._workspace).build_skills_summary()
        if skills_summary:
            parts.append(
                f"## 스킬을 사용하려면 read_file로 SKILL.md를 읽으세요.\n\n{skills_summary}"
            )

        return "\n\n".join(parts)

    async def _announce_result(
        self,
        workflow_id: str,
        task_id: str,
        label: str,
        task: str,
        result: str,
        origin: dict[str, Any],
        status: str,
    ) -> None:
        """내부 네트워크를 통해 서브에이전트의 결과를 메인 에이전트에 알립니다."""
        status_text: str = "성공적으로 완료되었습니다." if status == "ok" else "failed"
        announce_content = f"""
            [Subagent '{label}' {status_text}]
    
            Task: {task}
            
            Result:
            {result}
            
            이 내용을 사용자에게 자연스럽게 요약하세요. 간단하게 1~2문장으로 작성하고, "subagent"나 작업 ID 같은 기술적인 세부 사항은 언급하지 마세요. 
        """

        await self._bus.publish_inbound(
            InboundMessage(
                channel="system",
                sender_id="subagent",
                chat_id=f"{origin['channel']}:{origin['chat_id']}",
                content=announce_content,
                metadata=origin.get("metadata", {}),
                session_key_override=origin.get("session_key"),
            )
        )
        self._mark_notified(workflow_id)
        logger.debug(
            "Subagent [{}]가 {}:{}에 결과를 알렸습니다.",
            task_id,
            origin["channel"],
            origin["chat_id"],
        )

    async def cancel_by_session(self, session_key: str) -> int:
        """주어진 세션에 속한 모든 서브에이전트를 취소하고, 취소된 개수를 반환합니다."""
        tasks: list[asyncio.Task[None]] = [
            self._running_tasks[tid].asyncio_task
            for tid in self._session_tasks.get(session_key, [])
            if (tid in self._running_tasks) and not self._running_tasks[tid].asyncio_task.done()
        ]
        for task in tasks:
            task.cancel()

        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)

        return len(tasks)

    def list_running_tasks(self, session_key: str | None = None) -> list[dict[str, str]]:
        now: datetime = datetime.now()
        task_ids = (
            self._session_tasks.get(session_key, set())
            if session_key
            else self._running_tasks.keys()
        )
        result: list[dict[str, str]] = []
        for tid in task_ids:
            st: SubagentTask | None = self._running_tasks.get(tid)
            if st and not st.asyncio_task.done():
                elapsed = now - st.started_at
                result.append(
                    {
                        "task_id": st.task_id,
                        "label": st.label,
                        "role": st.role,
                        "elapsed": str(elapsed).split(".")[0],
                    }
                )
        return result

    async def cancel_task(self, task_id: str) -> tuple[bool, str]:
        st: SubagentTask | None = self._running_tasks.get(task_id)
        if not st or st.asyncio_task.done():
            return False, ""
        label: str = st.label
        st.asyncio_task.cancel()
        try:
            await st.asyncio_task
        except (asyncio.CancelledError, Exception):
            pass
        return True, label

    async def execute_existing_workflow(self, workflow_id: str) -> bool:
        if self._workflow_runtime is None:
            return False
        if self._check_threads() is not None:
            return False
        record = self._workflow_runtime.store.get(workflow_id)
        if record is None:
            return False
        if record.source_kind != "subagent":
            return False
        _ = self._workflow_runtime.start(workflow_id)

        metadata = record.metadata
        label_obj = metadata.get("label")
        role_obj = metadata.get("role")
        skill_name_obj = metadata.get("skillName")
        skill_path_obj = metadata.get("skillPath")
        origin_channel_obj = metadata.get("originChannel")
        origin_chat_id_obj = metadata.get("originChatId")
        origin_metadata_obj = metadata.get("originMetadata")
        session_key_obj = metadata.get("sessionKey")

        label: str = label_obj if isinstance(label_obj, str) else record.goal[:30]
        role: str = role_obj if isinstance(role_obj, str) else "executor"
        origin_channel: str = origin_channel_obj if isinstance(origin_channel_obj, str) else "cli"
        origin_chat_id: str = (
            origin_chat_id_obj if isinstance(origin_chat_id_obj, str) else "direct"
        )
        session_key: str | None = session_key_obj if isinstance(session_key_obj, str) else None
        origin_metadata: dict[str, Any] | None = (
            origin_metadata_obj if isinstance(origin_metadata_obj, dict) else None
        )

        if role == "skill":
            if not isinstance(skill_name_obj, str) or not isinstance(skill_path_obj, str):
                self._fail_workflow(workflow_id, "missing skill replay metadata")
                return False
            _ = await self.spawn_skill(
                task=record.goal,
                label=label,
                skill_name=skill_name_obj,
                skill_path=skill_path_obj,
                origin_channel=origin_channel,
                origin_chat_id=origin_chat_id,
                session_key=session_key,
                origin_metadata=origin_metadata,
                workflow_id=workflow_id,
            )
            return True

        _ = await self.spawn(
            task=record.goal,
            label=label,
            role=role,
            origin_channel=origin_channel,
            origin_chat_id=origin_chat_id,
            session_key=session_key,
            origin_metadata=origin_metadata,
            workflow_id=workflow_id,
        )
        return True

    def _create_workflow(
        self,
        *,
        task_id: str,
        role: str,
        task: str,
        origin: dict[str, Any],
        label: str,
        extra_metadata: dict[str, object],
    ) -> str:
        if self._workflow_runtime is None:
            return ""
        record = self._workflow_runtime.store.create(
            source_kind="subagent",
            goal=task,
            notify_target={
                "channel": origin.get("channel", "cli"),
                "chat_id": origin.get("chat_id", "direct"),
                "session_key": origin.get("session_key") or "",
            },
            metadata={
                "subagentTaskId": task_id,
                "label": label,
                "role": role,
                "originChannel": origin.get("channel", "cli"),
                "originChatId": origin.get("chat_id", "direct"),
                "originMetadata": origin.get("metadata") or {},
                "sessionKey": origin.get("session_key") or "",
                **extra_metadata,
            },
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

    @staticmethod
    def _extract_partial_progress(messages: list[dict[str, Any]], max_iterations: int) -> str:
        tool_calls: list[str] = []
        last_assistant_text: str = ""

        for msg in messages:
            if msg.get("role") == "assistant":
                if msg.get("tool_calls"):
                    for tc in msg["tool_calls"]:
                        name: str = tc.get("function", {}).get("name", "unknown")
                        if name not in tool_calls:
                            tool_calls.append(name)
                if msg.get("content"):
                    last_assistant_text = msg["content"]

        parts: list[str] = [f"(최대 반복 {max_iterations}회 도달, 부분 진행 상황)"]
        if tool_calls:
            parts.append(f"사용한 도구: {', '.join(tool_calls)}")
        if last_assistant_text:
            trimmed: str = last_assistant_text[:500]
            if len(last_assistant_text) > 500:
                trimmed += "..."
            parts.append(f"마지막 진행:\n{trimmed}")

        return "\n".join(parts) if len(parts) > 1 else parts[0]

    async def shutdown(self) -> None:
        running: list[SubagentTask] = [
            st for st in self._running_tasks.values() if not st.asyncio_task.done()
        ]
        if not running:
            return
        logger.warning(
            "종료 시 {}개 서브에이전트가 아직 실행 중: {}",
            len(running),
            ", ".join(f"[{st.task_id}] {st.label}" for st in running),
        )
        for st in running:
            st.asyncio_task.cancel()
        await asyncio.gather(
            *(st.asyncio_task for st in running),
            return_exceptions=True,
        )

    def get_running_count(self) -> int:
        return len(self._running_tasks)
