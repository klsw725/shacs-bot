"""에이전트 루프: 핵심 처리 엔진."""

import asyncio
import json
import os
import re
import sys
import weakref
from contextlib import AsyncExitStack
from datetime import datetime, timedelta
from pathlib import Path
from typing import Callable, Awaitable, Any, Mapping, Protocol

from loguru import logger

from shacs_bot.agent.context import ContextBuilder, ContextVariant
from shacs_bot.agent.execution_health import ExecutionHealthMonitor
from shacs_bot.agent.hooks import (
    AFTER_LLM_CALL,
    BEFORE_CONTEXT_BUILD,
    BEFORE_LLM_CALL,
    SESSION_LOADED,
    HookContext,
    HookRegistry,
    NoOpHookRegistry,
)
from shacs_bot.agent.memory import MemoryStore, MemoryConsolidator
from shacs_bot.agent.planner import AssistantPlan, PlanStep
from shacs_bot.agent.session.manager import SessionManager, Session
from shacs_bot.agent.subagent import SubagentManager
from shacs_bot.agent.tools.cron.cron import CronTool
from shacs_bot.agent.tools.cron.service import CronService
from shacs_bot.agent.tools.mcp import connect_mcp_servers
from shacs_bot.agent.tools.message import MessageTool
from shacs_bot.agent.tools.registry import ToolRegistry, create_default_tools
from shacs_bot.agent.tools.spawn import SpawnTool, ListTasksTool, CancelTaskTool
from shacs_bot.bus.events import InboundMessage, OutboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.agent.usage import TurnUsage, UsageTracker
from shacs_bot.config.schema import ExecToolConfig, ChannelsConfig, MediaConfig, UsageConfig
from shacs_bot.providers.base import LLMProvider, LLMResponse, ToolCallRequest
from shacs_bot.providers.failover import FailoverManager
from shacs_bot.workflow.models import WorkflowRecord
from shacs_bot.workflow.runtime import ManualRecoverResult, WorkflowRuntime
from shacs_bot.workflow.store import INCOMPLETE_STATES
from shacs_bot.workflow.wait_until import parse_wait_until_time


class AgentLoopObserver(Protocol):
    def on_llm_response(self, response: LLMResponse) -> None: ...

    def on_tool_result(
        self, tool_name: str, arguments: Mapping[str, object], result: str
    ) -> None: ...

    def on_final(self, final_content: str | None, finish_reason: str) -> None: ...

    def on_planner_decision(self, kind: str, fallback_engaged: bool) -> None: ...


class AgentLoop:
    """
    에이전트 루프는 핵심 처리 엔진입니다.

    역할:
    1. 버스로부터 메시지를 수신합니다.
    2. 히스토리, 메모리, 스킬을 포함한 컨텍스트를 구성합니다.
    3. LLM을 호출합니다.
    4. 도구 호출을 실행합니다.
    5. 응답을 다시 전송합니다.
    """

    _TOOL_RESULT_MAX_CHARS = 500

    def __init__(
        self,
        bus: MessageBus,
        provider: LLMProvider,
        workspace: Path,
        model: str | None = None,
        max_iterations: int = 40,
        memory_window: int = 100,
        context_window_tokens: int = 65_536,
        brave_api_key: str | None = None,
        web_proxy: str | None = None,
        exec_config: ExecToolConfig | None = None,
        cron_service: CronService | None = None,
        restrict_to_workspace: bool = False,
        session_manager: SessionManager | None = None,
        mcp_servers: dict | None = None,
        channels_config: ChannelsConfig | None = None,
        failover_manager: FailoverManager | None = None,
        provider_name: str | None = None,
        usage_config: UsageConfig | None = None,
        media_config: MediaConfig | None = None,
        media_api_key: str | None = None,
        media_base_url: str | None = None,
        skill_approval: str = "auto",
        hooks: HookRegistry | None = None,
        workflow_runtime: WorkflowRuntime | None = None,
    ):
        self._bus: MessageBus = bus
        self._hooks: HookRegistry = hooks or NoOpHookRegistry()
        self._channels_config: ChannelsConfig = channels_config
        self._workspace: Path = workspace
        self._workflow_runtime: WorkflowRuntime = workflow_runtime or WorkflowRuntime(
            self._workspace
        )

        self._provider: LLMProvider = provider
        self._model: str = model or self._provider.get_default_model()

        self._max_iterations: int = max_iterations
        self._memory_window: int = memory_window
        self._context_window_tokens: int = context_window_tokens

        self._brave_api_key: str | None = brave_api_key
        self._web_proxy: str | None = web_proxy
        self._exec_config: ExecToolConfig = exec_config or ExecToolConfig()
        self._cron_service: CronService | None = cron_service
        self._restrict_to_workspace: bool = restrict_to_workspace
        self._failover: FailoverManager | None = failover_manager
        self._provider_name: str | None = provider_name
        self._usage_config: UsageConfig | None = usage_config
        self._media_config: MediaConfig | None = media_config
        self._media_api_key: str | None = media_api_key
        self._media_base_url: str | None = media_base_url
        self._auto_eval_task: asyncio.Task[None] | None = None
        self._usage_tracker: UsageTracker | None = None
        if usage_config and usage_config.enabled:
            from shacs_bot.config.paths import get_usage_dir

            self._usage_tracker = UsageTracker(get_usage_dir())

        self._context = ContextBuilder(self._workspace)
        self._sessions: SessionManager = session_manager or SessionManager(self._workspace)
        self._tools: ToolRegistry = ToolRegistry(hooks=self._hooks)
        self._subagent = SubagentManager(
            provider=self._provider,
            workspace=self._workspace,
            bus=self._bus,
            model=self._model,
            brave_api_key=self._brave_api_key,
            web_proxy=self._web_proxy,
            exec_config=self._exec_config,
            restrict_to_workspace=self._restrict_to_workspace,
            hooks=self._hooks,
            workflow_runtime=self._workflow_runtime,
        )
        self._subagent.skill_approval = skill_approval

        self._running = False
        self._mcp_servers: dict = mcp_servers or {}
        self._mcp_stack: AsyncExitStack | None = None
        self._mcp_connected = False
        self._mcp_connecting = False

        self._active_tasks: dict[str, list[asyncio.Task]] = {}  # session_key -> tasks
        self._processing_lock: asyncio.Lock = asyncio.Lock()
        self._memory_consolidator: MemoryConsolidator = MemoryConsolidator(
            workspace=self._workspace,
            provider=self._provider,
            model=self._model,
            sessions=self._sessions,
            context_window_tokens=self._context_window_tokens,
            build_messages=self._context.build_messages,
            get_tool_definitions=self._tools.get_definitions,
        )
        self._register_default_tools()

    @property
    def tools(self) -> ToolRegistry:
        return self._tools

    @property
    def model(self) -> str:
        return self._model

    @property
    def workflow_runtime(self) -> WorkflowRuntime:
        return self._workflow_runtime

    @property
    def subagent_manager(self) -> SubagentManager:
        return self._subagent

    @property
    def channels_config(self) -> ChannelsConfig:
        return self._channels_config

    def _register_default_tools(self) -> None:
        """기본 도구 세트를 등록합니다."""
        for tool in create_default_tools(
            workspace=self._workspace,
            restrict_to_workspace=self._restrict_to_workspace,
            exec_config=self._exec_config,
            brave_api_key=self._brave_api_key,
            web_proxy=self._web_proxy,
        ):
            self._tools.register(tool)

        self._tools.register(MessageTool(send_callback=self._bus.publish_outbound))
        self._tools.register(SpawnTool(manager=self._subagent))
        self._tools.register(ListTasksTool(manager=self._subagent))
        self._tools.register(CancelTaskTool(manager=self._subagent))

        if self._cron_service:
            self._tools.register(CronTool(self._cron_service))

        if self._media_config and self._media_config.enabled:
            from shacs_bot.agent.tools.media import MediaGenerateTool

            self._tools.register(
                MediaGenerateTool(
                    config=self._media_config,
                    api_key=self._media_api_key or "",
                    base_url=self._media_base_url or "",
                )
            )

    async def run(self) -> None:
        """stop 명령에 계속 반응할 수 있도록, 메시지를 작업(task)으로 디스패치하면서 에이전트 루프를 실행합니다."""
        self._running = True

        await self._connect_mcp()
        logger.info("에이전트 루프 시작")

        while self._running:
            try:
                msg: InboundMessage = await asyncio.wait_for(
                    fut=self._bus.consume_inbound(), timeout=1.0
                )
            except asyncio.TimeoutError:
                continue

            cmd: str = msg.content.strip().lower()
            if cmd == "/stop":
                await self._handle_stop(msg)
            elif cmd == "/restart":
                await self._handle_restart(msg)
            else:
                task: asyncio.Task = asyncio.create_task(self._dispatch(msg))
                self._active_tasks.setdefault(msg.session_key, []).append(task)
                task.add_done_callback(
                    lambda t, k=msg.session_key: (
                        self._active_tasks.get(k, []) and self._active_tasks[k].remove(t)
                        if t in self._active_tasks.get(k, [])
                        else None
                    )
                )

    async def _connect_mcp(self) -> None:
        """설정된 MCP 서버에 연결합니다 (한 번만, 지연 방식으로)."""
        if self._mcp_connected or self._mcp_connecting or not self._mcp_servers:
            return

        self._mcp_connecting = True

        try:
            self._mcp_stack = AsyncExitStack()
            await self._mcp_stack.__aenter__()
            await connect_mcp_servers(self._mcp_servers, self._tools, self._mcp_stack)

            self._mcp_connected = True
        except Exception as e:
            logger.error("MCP 서버에 연결하지 못했습니다 (다음 메시지에서 다시 시도합니다): {}", e)

            if self._mcp_stack:
                try:
                    await self._mcp_stack.aclose()
                except (RuntimeError, BaseExceptionGroup):
                    pass

                self._mcp_stack = None
        finally:
            self._mcp_connecting = False

    async def _handle_stop(self, msg: InboundMessage) -> None:
        """해당 세션의 모든 활성 작업과 서브에이전트를 취소합니다."""
        tasks: list[asyncio.Task] = self._active_tasks.pop(msg.session_key, [])
        cancelled: int = sum(1 for t in tasks if not t.done() and t.cancel())
        for task in tasks:
            try:
                await task
            except (asyncio.CancelledError, Exception):
                pass

        sub_cancelled: int = await self._subagent.cancel_by_session(msg.session_key)
        total: int = cancelled + sub_cancelled
        content: str = (
            f"⏹ {total}개의 작업을 중지했습니다." if total else "중지할 활성 작업이 없습니다."
        )
        await self._bus.publish_outbound(
            OutboundMessage(channel=msg.channel, chat_id=msg.chat_id, content=content)
        )

    async def _handle_restart(self, msg):
        """os.execv를 통해 프로세스 재시작합니다."""
        await self._bus.publish_outbound(
            OutboundMessage(channel=msg.channel, chat_id=msg.chat_id, content="재시작 중...")
        )

        async def _do_restart():
            await asyncio.sleep(1)
            os.execv(sys.executable, [sys.executable] + sys.argv)

        asyncio.create_task(_do_restart())

    async def _dispatch(self, msg: InboundMessage) -> None:
        """전역 락(global lock) 하에서 메시지를 처리합니다."""
        async with self._processing_lock:
            try:
                response: OutboundMessage | None = await self._process_message(msg)
                if response is not None:
                    await self._bus.publish_outbound(response)
                elif msg.channel == "cli":
                    await self._bus.publish_outbound(
                        OutboundMessage(
                            channel=msg.channel,
                            chat_id=msg.chat_id,
                            content="",
                            metadata=msg.metadata or {},
                        )
                    )
            except asyncio.CancelledError:
                logger.info("세션 {}에 대한 작업이 취소되었습니다.", msg.session_key)
                raise
            except Exception:
                logger.exception(
                    "세션 {}의 메시지를 처리하는 중 오류가 발생했습니다.", msg.session_key
                )
                await self._bus.publish_outbound(
                    OutboundMessage(
                        channel=msg.channel,
                        chat_id=msg.chat_id,
                        content="죄송합니다, 오류가 발생했습니다.",
                    )
                )

    def _set_tool_context(
        self,
        channel: str,
        chat_id: str,
        message_id: str | None = None,
        metadata: dict[str, Any] | None = None,
        session_key: str | None = None,
    ) -> None:
        """라우팅 정보가 필요한 모든 도구의 컨텍스트를 업데이트합니다."""
        for name in ("message", "spawn", "list_tasks", "cron"):
            if tool := self._tools.get(name):
                if hasattr(tool, "set_context"):
                    tool.set_context(
                        channel, chat_id, metadata=metadata or {}, session_key=session_key
                    )

    async def _process_message(
        self,
        msg: InboundMessage,
        session_key: str = None,
        on_progress: Callable[[str], Awaitable[None]] | None = None,
        observer: AgentLoopObserver | None = None,
        variant: ContextVariant | None = None,
    ) -> OutboundMessage | None:
        """단일 인바운드 메시지를 처리하고 응답을 반환합니다."""
        # 시스템 메시지: chat_id에서 파싱 ("channel:chat_id")
        if msg.channel == "system":
            channel, chat_id = (
                msg.chat_id.split(":", 1) if ":" in msg.chat_id else ("cli", msg.chat_id)
            )

            logger.info("{}로부터의 시스템 메시지를 처리 중입니다.", msg.sender_id)
            self._set_tool_context(
                channel=channel,
                chat_id=chat_id,
                message_id=msg.metadata.get("message_id"),
                metadata=msg.metadata,
                session_key=msg.session_key_override,
            )

            key: str = msg.session_key_override or f"{channel}:{chat_id}"
            session: Session = self._sessions.get_or_create(key=key)
            await self._hooks.emit(
                HookContext(
                    event=SESSION_LOADED,
                    session_key=key,
                    channel=channel,
                )
            )
            history: list[dict[str, Any]] = session.get_history(max_messages=0)
            await self._hooks.emit(
                HookContext(
                    event=BEFORE_CONTEXT_BUILD,
                    session_key=key,
                    channel=channel,
                )
            )
            messages: list[dict[str, Any]] = self._context.build_messages(
                history=history,
                current_messages=msg.content,
                channel=channel,
                chat_id=chat_id,
                variant=variant,
            )
            final_content, _, all_msg, _ = await self._run_agent_loop(
                messages, _session_key=key, _channel=channel, observer=observer
            )
            self._save_turn(session=session, messages=all_msg, skip=(1 + len(history)))

            self._sessions.save(session)

            return OutboundMessage(
                channel=channel,
                chat_id=chat_id,
                content=final_content or "백그라운드 태스크 완료.",
                metadata=msg.metadata or {},
            )

        preview: str = msg.content[:80] + "..." if len(msg.content) > 80 else msg.content
        logger.info("{}:{} 로부터 온 메시지를 처리 중: {}", msg.channel, msg.sender_id, preview)

        key: str = session_key or msg.session_key
        session: Session = self._sessions.get_or_create(key=key)
        effective_variant: ContextVariant | None = variant or self._runtime_policy_variant(key)
        await self._hooks.emit(
            HookContext(
                event=SESSION_LOADED,
                session_key=key,
                channel=msg.channel,
            )
        )

        # Pending approval 응답 감지
        user_text: str = msg.content.strip().lower()
        if user_text in ("y", "n", "yes", "no"):
            from shacs_bot.agent.approval import get_pending_approval_for_session, resolve_approval

            req_id: str | None = get_pending_approval_for_session(key)
            if req_id:
                approved: bool = user_text in ("y", "yes")
                resolve_approval(req_id, approved)
                label: str = "승인" if approved else "거부"
                return OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content=f"\U0001f6e1 {label}되었습니다.",
                )

        # Slash 명령어
        cmd: str = msg.content.strip().lower()
        if cmd == "/new":
            try:
                if not await self._memory_consolidator.archive_unconsolidated(session=session):
                    return OutboundMessage(
                        channel=msg.channel,
                        chat_id=msg.chat_id,
                        content="메모리 아카이브에 실패했습니다. 세션이 초기화되지 않았습니다. 다시 시도해 주세요.",
                    )
            except Exception:
                logger.exception("{} /new 아카이브 실패", session.key)
                return OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content="메모리 아카이브에 실패했습니다. 세션이 초기화되지 않았습니다. 다시 시도해 주세요.",
                )

            session.clear()
            self._sessions.save(session)
            self._sessions.invalidate(session.key)

            return OutboundMessage(
                channel=msg.channel, chat_id=msg.chat_id, content="새로운 세션이 시작되었습니다."
            )
        elif cmd == "/usage":
            if not self._usage_tracker:
                return OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content="사용량 추적이 비활성화되어 있습니다.",
                )

            session_summary = self._usage_tracker.get_session_summary(key)
            daily_summary = self._usage_tracker.get_daily_summary()

            cost_session = (
                f"${session_summary['cost']:.4f}" if session_summary["cost"] else "해당 없음"
            )
            cost_daily = f"${daily_summary['cost']:.4f}" if daily_summary["cost"] else "해당 없음"

            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=(
                    f"\U0001f4ca 사용량 요약\n\n"
                    f"**현재 세션** ({key})\n"
                    f"\u2022 토큰: {session_summary['prompt']:,} prompt + {session_summary['completion']:,} completion\n"
                    f"\u2022 비용: {cost_session}\n"
                    f"\u2022 LLM 호출: {session_summary['calls']}회\n\n"
                    f"**오늘 전체**\n"
                    f"\u2022 토큰: {daily_summary['total']:,}\n"
                    f"\u2022 비용: {cost_daily}\n"
                    f"\u2022 세션 수: {daily_summary['sessions']}"
                ),
            )
        elif cmd == "/status":
            msg_count = len(session.messages) - session.last_consolidated
            total_count = len(session.messages)

            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=(
                    f"\U0001f916 상태\n\n"
                    f"\u2022 모델: {self._model}\n"
                    f"\u2022 프로바이더: {self._provider_name or 'auto'}\n"
                    f"\u2022 세션: {key}\n"
                    f"\u2022 메시지: {msg_count}개 (미통합) / {total_count}개 (전체)\n"
                    f"\u2022 메모리 윈도우: {self._memory_window}"
                ),
            )
        elif cmd == "/workflows" or cmd == "/workflows all":
            return self._handle_workflows_command(
                msg=msg, session_key=key, include_completed=cmd.endswith(" all")
            )
        elif cmd.startswith("/workflow recover"):
            return self._handle_workflow_recover(msg)
        elif cmd.startswith("/workflow "):
            return self._handle_workflow_show(msg=msg, session_key=key)
        elif cmd.startswith("/skill trust"):
            return self._handle_skill_trust(msg)
        elif cmd == "/help":
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=(
                    "\U0001f988 shacs-bot 명령어:\n"
                    "/new \u2014 새 대화를 시작합니다\n"
                    "/stop \u2014 현재 작업을 중지합니다\n"
                    "/restart \u2014 봇을 재시작합니다\n"
                    "/usage \u2014 토큰 사용량과 비용을 확인합니다\n"
                    "/status \u2014 현재 모델, 세션 상태를 확인합니다\n"
                    "/workflows [all] \u2014 현재 채널에서 보이는 workflow 목록을 표시합니다\n"
                    "/workflow <id> \u2014 특정 workflow 상세를 표시합니다\n"
                    "/workflow recover <id> \u2014 workflow를 queued 상태로 복원합니다\n"
                    "/skill trust \u2014 스킬 승인 모드를 확인하거나 변경합니다\n"
                    "/help \u2014 사용 가능한 명령어를 표시합니다"
                ),
            )

        # request_approval 승인 게이트: y/yes/승인 또는 n/no/거절만 소비, 그 외는 알림 반환
        _APPROVAL_YES: frozenset[str] = frozenset({"y", "yes", "승인"})
        _APPROVAL_NO: frozenset[str] = frozenset({"n", "no", "거절"})
        _waiting_approval_id: str | None = session.metadata.get("waiting_workflow_approval_id")
        if isinstance(_waiting_approval_id, str) and _waiting_approval_id:
            _ap_wf = self._workflow_runtime.store.get(_waiting_approval_id)
            if _ap_wf is not None and _ap_wf.state == "waiting_input":
                _reply: str = msg.content.strip().lower()
                if _reply in _APPROVAL_YES:
                    _approved_rec = self._workflow_runtime.approve_workflow(_waiting_approval_id)
                    if _approved_rec is not None:
                        session.metadata.pop("waiting_workflow_approval_id", None)
                        self._sessions.save(session)
                        return OutboundMessage(
                            channel=msg.channel,
                            chat_id=msg.chat_id,
                            content=f"✅ 승인되었습니다. 워크플로우를 계속 진행합니다. (`{_waiting_approval_id}`)",
                            metadata=msg.metadata or {},
                        )
                elif _reply in _APPROVAL_NO:
                    _ = self._workflow_runtime.fail(
                        _waiting_approval_id, last_error="사용자 거절"
                    )
                    session.metadata.pop("waiting_workflow_approval_id", None)
                    self._sessions.save(session)
                    return OutboundMessage(
                        channel=msg.channel,
                        chat_id=msg.chat_id,
                        content=f"❌ 거절되었습니다. 워크플로우가 종료됩니다. (`{_waiting_approval_id}`)",
                        metadata=msg.metadata or {},
                    )
                else:
                    # 승인/거절 외 텍스트는 워크플로우를 건드리지 않고 알림만 반환
                    return OutboundMessage(
                        channel=msg.channel,
                        chat_id=msg.chat_id,
                        content=(
                            f"⏳ 워크플로우 `{_waiting_approval_id}`가 승인을 기다리고 있습니다.\n"
                            f"승인하려면 **y** 또는 **승인**, 거절하려면 **n** 또는 **거절**을 입력하세요."
                        ),
                        metadata=msg.metadata or {},
                    )
            else:
                # 워크플로우가 더 이상 waiting_input 상태가 아니면 세션 메타 정리
                session.metadata.pop("waiting_workflow_approval_id", None)
                self._sessions.save(session)

        # waiting_input 상태 워크플로우가 있으면 이 메시지를 답변으로 소비합니다.
        _waiting_wf_id: str | None = session.metadata.get("waiting_workflow_id")
        if isinstance(_waiting_wf_id, str) and _waiting_wf_id:
            _wf = self._workflow_runtime.store.get(_waiting_wf_id)
            if _wf is not None and _wf.state == "waiting_input":
                _resumed = self._workflow_runtime.resume_with_user_answer(
                    _waiting_wf_id, answer=msg.content
                )
                if _resumed is not None:
                    session.metadata.pop("waiting_workflow_id", None)
                    self._sessions.save(session)
                    return OutboundMessage(
                        channel=msg.channel,
                        chat_id=msg.chat_id,
                        content=f"✅ 답변을 받았습니다. 워크플로우를 계속 진행합니다. (`{_waiting_wf_id}`)",
                        metadata=msg.metadata or {},
                    )
            else:
                # 워크플로우가 더 이상 waiting_input 상태가 아니면 세션 메타 정리
                session.metadata.pop("waiting_workflow_id", None)
                self._sessions.save(session)

        consolidated: bool = await self._memory_consolidator.maybe_consolidate_by_tokens(
            session=session
        )
        if consolidated and self._channels_config and self._channels_config.send_memory_hints:
            await self._bus.publish_outbound(
                OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content="\U0001f4be 기억을 정리했어요",
                    metadata={"_progress": True, "_memory_hint": True},
                )
            )

        self._set_tool_context(
            channel=msg.channel,
            chat_id=msg.chat_id,
            message_id=msg.metadata.get("message_id"),
            metadata=msg.metadata,
            session_key=key,
        )

        if message_tool := self._tools.get("message"):
            if isinstance(message_tool, MessageTool):
                message_tool.start_turn()

        history: list[dict[str, Any]] = session.get_history(max_messages=self._memory_window)
        await self._hooks.emit(
            HookContext(
                event=BEFORE_CONTEXT_BUILD,
                session_key=key,
                channel=msg.channel,
            )
        )
        initial_messages: list[dict[str, Any]] = self._context.build_messages(
            history=history,
            current_messages=msg.content,
            media=msg.media if msg.media else None,
            channel=msg.channel,
            chat_id=msg.chat_id,
            variant=effective_variant,
        )

        async def _bus_progress(
            content: str, *, tool_hint: bool = False, skill_hint: bool = False
        ) -> None:
            meta: dict[str, Any] = dict(msg.metadata or {})
            meta["_progress"] = True
            meta["_tool_hint"] = tool_hint
            meta["_skill_hint"] = skill_hint
            await self._bus.publish_outbound(
                OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content=content,
                    metadata=meta,
                )
            )

        if not msg.media:
            _plan = await self._classify_request_with_llm_fallback(msg.content, observer=observer)
            session.metadata["last_planning_result"] = _plan.model_dump()
            if _plan.kind == "direct_answer":
                session.metadata.pop("current_plan", None)
            if _plan.kind == "clarification":
                session.metadata["current_plan"] = _plan.model_dump()
                self._sessions.save(session)
                return OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content=_plan.clarification_question
                    or "요청을 좀 더 구체적으로 알려주시겠어요?",
                    metadata=msg.metadata or {},
                )
            if _plan.kind == "planned_workflow":
                session.metadata["current_plan"] = _plan.model_dump()
                _wf_record = self._workflow_runtime.register_planned_workflow(
                    goal=_plan.summary or msg.content[:200],
                    plan=_plan.model_dump(),
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    session_key=key,
                )
                self._sessions.save(session)
                return OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content=self._format_plan(_plan)
                    + f"\n\n🆔 워크플로우 ID: `{_wf_record.workflow_id}`",
                    metadata=msg.metadata or {},
                )
        final_content, _, all_msg, turn_usage = await self._run_agent_loop(
            init_messages=initial_messages,
            on_progress=on_progress or _bus_progress,
            _session_key=key,
            _channel=msg.channel,
            observer=observer,
        )
        if final_content is None:
            final_content = "처리는 완료했지만 제공할 응답이 없습니다."

        if self._usage_tracker:
            self._usage_tracker.record(session_key=key, turn=turn_usage)

        if self._usage_config and self._usage_config.footer != "off":
            footer = turn_usage.format_footer(mode=self._usage_config.footer)
            if footer:
                final_content = f"{final_content}\n\n{footer}"

        self._save_turn(session=session, messages=all_msg, skip=(1 + len(history)))
        self._sessions.save(session)
        self._maybe_schedule_auto_eval(session_key=key)

        if msg.media:
            self._cleanup_inbound_media(msg.media)

        consolidated: bool = await self._memory_consolidator.maybe_consolidate_by_tokens(
            session=session
        )
        if consolidated and self._channels_config and self._channels_config.send_memory_hints:
            await self._bus.publish_outbound(
                OutboundMessage(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    content="\U0001f4be 기억을 정리했어요",
                    metadata={"_progress": True, "_memory_hint": True},
                )
            )

        mt: Any = self._tools.get("message")
        if mt and isinstance(mt, MessageTool) and mt.sent_in_turn:
            return None

        preview: str = final_content[:120] + "..." if len(final_content) > 120 else final_content
        logger.info("{}:{}에 대한 응답: {}", msg.channel, msg.sender_id, preview)
        return OutboundMessage(
            channel=msg.channel,
            chat_id=msg.chat_id,
            content=final_content,
            metadata=msg.metadata or {},
        )

    def _runtime_policy_variant(self, session_key: str) -> ContextVariant | None:
        if session_key.startswith("eval:") or session_key.startswith("scheduled:"):
            return None

        try:
            from shacs_bot.evals.state import read_auto_eval_state

            state = read_auto_eval_state(self._workspace)
            if not state:
                return None
            if state.recommended_runtime_variant == "strict-completion":
                return ContextVariant(completion_policy="strict")
        except Exception as exc:
            logger.warning("Runtime eval policy 로드 실패: {}", exc)

        return None

    async def _run_agent_loop(
        self,
        init_messages: list[dict[str, Any]],
        on_progress: Callable[..., Awaitable[None]] | None = None,
        _session_key: str | None = None,
        _channel: str | None = None,
        observer: AgentLoopObserver | None = None,
    ) -> tuple[str | None, list[str], list[dict[str, Any]], TurnUsage]:
        from shacs_bot.observability.tracing import span as otel_span

        messages: list[dict[str, Any]] = init_messages
        final_content: str | None = None
        tools_used: list[str] = []
        health: ExecutionHealthMonitor = ExecutionHealthMonitor()
        turn_usage: TurnUsage = TurnUsage(model=self._model, provider=self._provider_name or "")
        final_finish_reason: str = ""

        for _ in range(self._max_iterations):
            await self._hooks.emit(
                HookContext(
                    event=BEFORE_LLM_CALL,
                    session_key=_session_key,
                    channel=_channel,
                    payload={"model": self._model, "messages_count": len(messages)},
                )
            )
            with otel_span("llm_call", {"model": self._model}) as llm_span:
                response: LLMResponse = await self._provider.chat_with_retry(
                    messages=messages,
                    tools=self._tools.get_definitions(),
                    model=self._model,
                    failover_manager=self._failover,
                    provider_name=self._provider_name,
                )
                if llm_span and response.finish_reason != "error":
                    llm_span.set_attribute("tokens.prompt", response.usage.get("prompt_tokens", 0))
                    llm_span.set_attribute(
                        "tokens.completion", response.usage.get("completion_tokens", 0)
                    )
                    llm_span.set_attribute("finish_reason", response.finish_reason)
                    llm_span.set_attribute(
                        "cache.read_tokens", response.usage.get("cache_read_input_tokens", 0)
                    )
                turn_usage.accumulate(response.usage, self._model, self._provider_name or "")
            final_finish_reason = response.finish_reason
            if observer:
                try:
                    observer.on_llm_response(response)
                except Exception as e:
                    logger.warning("AgentLoop observer on_llm_response 실패: {}", e)
            await self._hooks.emit(
                HookContext(
                    event=AFTER_LLM_CALL,
                    session_key=_session_key,
                    channel=_channel,
                    payload={
                        "model": self._model,
                        "finish_reason": response.finish_reason,
                        "has_tool_calls": response.has_tool_calls,
                        "usage": {
                            "prompt_tokens": response.usage.get("prompt_tokens", 0),
                            "completion_tokens": response.usage.get("completion_tokens", 0),
                        },
                    },
                )
            )
            if response.has_tool_calls:
                if on_progress:
                    thought = self._strip_think(response.content)
                    if thought:
                        await on_progress(thought)

                    await on_progress(self._tool_hint(response.tool_calls), tool_hint=True)

                    skill_msg: str | None = self._detect_skill_hint(response.tool_calls)
                    if skill_msg:
                        await on_progress(skill_msg, skill_hint=True)

                    spawn_msg: str | None = self._detect_spawn_hint(response.tool_calls)
                    if spawn_msg:
                        await on_progress(spawn_msg, skill_hint=True)

                tool_call_dicts: list[dict[str, Any]] = [
                    tool_call.to_openai_tool_call() for tool_call in response.tool_calls
                ]
                messages: list[dict[str, Any]] = self._context.add_assistant_message(
                    messages=messages,
                    content=response.content,
                    tool_calls=tool_call_dicts,
                    reasoning_content=response.reasoning_content,
                    thinking_blocks=response.thinking_blocks,
                )

                for tool_call in response.tool_calls:
                    tools_used.append(tool_call.name)
                    args_str: str = json.dumps(tool_call.arguments, ensure_ascii=False)
                    logger.info("Tool call: {}({})", tool_call.name, args_str[:200])
                    result: str = await self._tools.execute(
                        tool_call.name,
                        tool_call.arguments,
                        session_key=_session_key,
                        channel=_channel,
                    )
                    if observer:
                        try:
                            observer.on_tool_result(tool_call.name, tool_call.arguments, result)
                        except Exception as e:
                            logger.warning("AgentLoop observer on_tool_result 실패: {}", e)
                    health.check(tool_call.name, tool_call.arguments, result)
                    messages: list[dict[str, Any]] = self._context.add_tool_result(
                        messages=messages,
                        tool_call_id=tool_call.id,
                        tool_name=tool_call.name,
                        result=result,
                    )
            else:
                clean: str | None = self._strip_think(response.content)
                # 에러 응답은 세션 히스토리에 저장하지 않는다.
                # 이렇게 하면 컨텍스트가 오염되어 영구적인 400 에러 루프(#1303)가 발생할 수 있다.
                if response.finish_reason == "error":
                    logger.error("LLM이 오류를 반환했습니다: {}", (clean or "")[:200])
                    final_content = (
                        clean or "죄송합니다. AI 모델을 호출하는 중 오류가 발생했습니다."
                    )
                    break

                messages: list[dict[str, Any]] = self._context.add_assistant_message(
                    messages=messages,
                    content=clean,
                    reasoning_content=response.reasoning_content,
                    thinking_blocks=response.thinking_blocks,
                )

                final_content = clean
                break
        else:
            if final_content is None:
                logger.warning("최대 반복횟수 ({}) 도달", self._max_iterations)
                final_content = f"""
                   도구 호출 최대 반복 횟수({self._max_iterations})에 도달했지만 작업을 완료하지 못했습니다. 작업을 더 작은 단계로 나누어 다시 시도해 보세요. 
                """
                final_finish_reason = "max_iterations"

        if observer:
            try:
                observer.on_final(final_content, final_finish_reason)
            except Exception as e:
                logger.warning("AgentLoop observer on_final 실패: {}", e)

        return final_content, tools_used, messages, turn_usage

    def _save_turn(self, session: Session, messages: list[dict[str, Any]], skip: int) -> None:
        """새로운 대화 턴의 메시지를 세션에 저장하고, 크기가 큰 도구 실행 결과는 잘라서 저장합니다."""
        for message in messages[skip:]:
            entry: dict = dict(message)
            role: str = entry.get("role")
            content: Any = entry.get("content")
            if role == "assistant" and not content and not entry.get("tool_calls"):
                continue  # 내용이 없는 assistant 메시지는 저장하지 않고 건너뛴다 — 세션 컨텍스트를 망가뜨릴 수 있기 때문이다.
            elif (
                role == "tool"
                and isinstance(content, str)
                and (len(content) > self._TOOL_RESULT_MAX_CHARS)
            ):
                entry["content"] = content[: self._TOOL_RESULT_MAX_CHARS] + "\n... (중략)"
            elif role == "user":
                if isinstance(content, str) and content.startswith(
                    ContextBuilder.RUNTIME_CONTEXT_TAG
                ):
                    # 런타임 컨텍스트 접두사는 제거하고, 사용자 텍스트만 유지한다.
                    parts: list[str] = content.split("\n\n", 1)
                    if len(parts) > 1 and parts[1].strip():
                        entry["content"] = parts[1]
                    else:
                        continue
                elif isinstance(content, list):
                    filtered: list[dict[str, Any]] = []

                    for c in content:
                        if (
                            c.get("type") == "text"
                            and isinstance(c.get("text"), str)
                            and c["text"].startswith(ContextBuilder.RUNTIME_CONTEXT_TAG)
                        ):
                            continue  # 멀티모달 메시지에서 런타임 컨텍스트를 제거
                        elif c.get("type") == "image_url" and c.get("image_url", {}).get(
                            "url", ""
                        ).startswith("data:image/"):
                            source: str = c.get("_meta", {}).get("source_path", "")
                            label: str = f"[image: {source}]" if source else "[image]"
                            filtered.append({"type": "text", "text": label})
                        else:
                            filtered.append(c)

                    if not filtered:
                        continue
                    entry["content"] = filtered

            entry.setdefault("timestamp", datetime.now().isoformat())
            session.messages.append(entry)

        session.updated_at = datetime.now()

    def _maybe_schedule_auto_eval(self, session_key: str) -> None:
        if session_key.startswith("eval:") or session_key.startswith("scheduled:"):
            return
        if self._auto_eval_task and not self._auto_eval_task.done():
            return

        try:
            from shacs_bot.evals.autoloop import AutoEvalService

            service = AutoEvalService(self._workspace, self, self._sessions)
            if not service.prepare_trigger(session_key):
                return

            async def _run_auto_eval() -> None:
                try:
                    _ = await service.run_auto_eval(
                        session_filter="",
                        include_eval_sessions=False,
                        triggered=True,
                        trigger_session_key=session_key,
                    )
                except Exception as exc:
                    _ = service.mark_trigger_failure(
                        session_key, str(exc) or exc.__class__.__name__
                    )
                    logger.warning("Self-eval auto-run 실패: {}", exc)

            self._auto_eval_task = asyncio.create_task(_run_auto_eval())
        except Exception as exc:
            logger.warning("Self-eval trigger 준비 실패: {}", exc)

    @staticmethod
    def _cleanup_inbound_media(media: list[str]) -> None:
        """턴 처리 완료 후 수신 미디어 파일을 삭제합니다."""
        for media_path in media:
            try:
                p = Path(media_path)
                if p.exists():
                    p.unlink()
                    logger.debug("Inbound media cleaned up: {}", media_path)
            except Exception as e:
                logger.warning("Failed to clean up inbound media {}: {}", media_path, e)

    async def process_direct(
        self,
        content: str,
        session_key: str = "cli:direct",
        channel: str = "cli",
        chat_id: str = "direct",
        on_progress: Callable[[str], Awaitable[None]] | None = None,
        observer: AgentLoopObserver | None = None,
        variant: ContextVariant | None = None,
    ) -> str:
        """메시지를 직접 처리합니다(CLI 또는 cron 용도)."""
        await self._connect_mcp()

        msg: InboundMessage = InboundMessage(
            channel=channel, sender_id="user", chat_id=chat_id, content=content
        )
        response: OutboundMessage = await self._process_message(
            msg=msg,
            session_key=session_key,
            on_progress=on_progress,
            observer=observer,
            variant=variant,
        )
        return response.content if response else ""

    def _strip_think(self, content: str | None) -> str | None:
        """몇몇 모델의 경우 content 안에 있는 <think>...</think> 블록 제거"""
        if not content:
            return None

        return re.sub(r"<think>[\s\S]*?</think>", "", content).strip() or None

    def _tool_hint(self, tool_calls: list[ToolCallRequest]):
        """툴 호출을 간단한 힌트 형태로 포맷합니다. 예: web_search("query")."""

        def _fmt(tc: ToolCallRequest):
            args = tc.arguments or {}
            val: dict = next(iter(args.values()), None) if isinstance(args, dict) else None
            if not isinstance(val, str):
                return tc.name

            return f"{tc.name}('{val[:40]}...')" if len(val) > 40 else f"{tc.name}('{val}')"

        return ", ".join(_fmt(tc) for tc in tool_calls)

    @staticmethod
    def _detect_skill_hint(tool_calls: list[ToolCallRequest]) -> str | None:
        """도구 호출에서 스킬 사용을 감지하여 친화적 힌트를 반환합니다."""
        for tc in tool_calls:
            if tc.name == "read_file":
                path: str = (tc.arguments or {}).get("path", "")
                if "/skills/" in path and path.endswith("/SKILL.md"):
                    skill_name: str = path.split("/skills/")[-1].split("/")[0]
                    return f"\U0001f527 {skill_name} 스킬 사용 중"
        return None

    def _handle_skill_trust(self, msg: InboundMessage) -> OutboundMessage:
        """'/skill trust [auto|manual|off]' 슬래시 명령어를 처리합니다."""
        from shacs_bot.config.loader import load_config, save_config

        parts: list[str] = msg.content.strip().split()
        valid_modes = ("auto", "manual", "off")

        if len(parts) == 3 and parts[2].lower() in valid_modes:
            mode: str = parts[2].lower()
            config = load_config()
            config.tools.skill_approval = mode
            save_config(config)
            self._subagent.skill_approval = mode
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=f"\U0001f6e1 스킬 승인 모드: **{mode}**",
            )

        current: str = self._subagent.skill_approval
        return OutboundMessage(
            channel=msg.channel,
            chat_id=msg.chat_id,
            content=(
                f"\U0001f6e1 스킬 승인 모드: **{current}**\n\n"
                f"사용법: `/skill trust auto|manual|off`\n"
                f"• auto — 3단계 분류기가 자동 판단\n"
                f"• manual — 사용자에게 직접 승인 요청\n"
                f"• off — 승인 없이 실행"
            ),
        )

    def _handle_workflows_command(
        self,
        *,
        msg: InboundMessage,
        session_key: str,
        include_completed: bool,
    ) -> OutboundMessage:
        records = self._workflow_runtime.store.list_all()
        visible = [
            record
            for record in records
            if self._workflow_visible_to_session(record, session_key=session_key, msg=msg)
        ]
        if not include_completed:
            visible = [record for record in visible if record.state in INCOMPLETE_STATES]

        visible.sort(key=lambda record: record.updated_at, reverse=True)
        visible = visible[:10]

        if not visible:
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content="표시할 workflow가 없습니다.",
            )

        lines = ["📋 workflow 목록", ""]
        for record in visible:
            goal = record.goal if len(record.goal) <= 60 else f"{record.goal[:57]}..."
            lines.append(
                f"• `{record.workflow_id}` [{record.state}] {record.source_kind} · retries={record.retries}\n"
                f"  {goal}"
            )
        if not include_completed:
            lines.append("\n완료된 항목까지 보려면 `/workflows all`을 사용하세요.")
        lines.append("상세 조회: `/workflow <id>`")
        return OutboundMessage(channel=msg.channel, chat_id=msg.chat_id, content="\n".join(lines))

    def _handle_workflow_show(self, msg: InboundMessage, session_key: str) -> OutboundMessage:
        parts = msg.content.strip().split(maxsplit=1)
        if len(parts) < 2 or not parts[1].strip():
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content="사용법: `/workflow <id>`",
            )

        workflow_id = parts[1].strip()
        record = self._workflow_runtime.store.get(workflow_id)
        if record is None or not self._workflow_visible_to_session(
            record, session_key=session_key, msg=msg
        ):
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=f"workflow `{workflow_id}`를 찾을 수 없습니다.",
            )

        details = [
            f"🧭 workflow `{record.workflow_id}`",
            f"• source: {record.source_kind}",
            f"• state: {record.state}",
            f"• retries: {record.retries}",
            f"• updated: {record.updated_at}",
            f"• next run: {record.next_run_at or '-'}",
            f"• goal: {record.goal}",
        ]
        if record.last_error:
            details.append(f"• last error: {record.last_error}")
        if record.metadata:
            metadata_preview = ", ".join(
                f"{key}={value}" for key, value in sorted(record.metadata.items())
            )
            details.append(f"• metadata: {metadata_preview}")

        return OutboundMessage(channel=msg.channel, chat_id=msg.chat_id, content="\n".join(details))

    def _handle_workflow_recover(self, msg: InboundMessage) -> OutboundMessage:
        parts = msg.content.strip().split(maxsplit=2)
        if len(parts) < 3 or not parts[2].strip():
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content="사용법: `/workflow recover <id>`",
            )

        workflow_id = parts[2].strip()
        if workflow_id == "all":
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content="`all` recover는 지원하지 않습니다. `/workflow recover <id>`만 사용할 수 있습니다.",
            )

        result: ManualRecoverResult = self._workflow_runtime.manual_recover(
            workflow_id,
            channel=msg.channel,
            chat_id=msg.chat_id,
            sender_id=msg.sender_id,
        )
        if result.status == "missing":
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=f"workflow `{workflow_id}`를 찾을 수 없습니다.",
            )

        record = result.record
        if record is None:
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=f"workflow `{workflow_id}`를 찾을 수 없습니다.",
            )

        if result.status == "already_queued":
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=f"ℹ️ workflow `{workflow_id}`는 이미 대기열에 있습니다.",
            )
        if result.status == "cooldown":
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=(
                    f"⏳ workflow `{workflow_id}`는 방금 recover 요청되었습니다. "
                    f"현재 상태: {record.state}"
                ),
            )
        if result.status == "terminal":
            return OutboundMessage(
                channel=msg.channel,
                chat_id=msg.chat_id,
                content=(
                    f"✗ workflow `{workflow_id}`는 recover할 수 없습니다. 현재 상태: {record.state}"
                ),
            )

        previous_state = result.previous_state or "unknown"
        return OutboundMessage(
            channel=msg.channel,
            chat_id=msg.chat_id,
            content=(
                f"✓ workflow `{workflow_id}`를 재개 대기열에 넣었습니다.\n"
                f"• 이전 상태: {previous_state}\n"
                f"• 현재 상태: {record.state}\n"
                "• 실제 실행은 처리 가능한 큐 시스템이 담당합니다."
            ),
        )

    def _workflow_visible_to_session(
        self,
        record: WorkflowRecord,
        *,
        session_key: str,
        msg: InboundMessage,
    ) -> bool:
        if record.notify_target.session_key and record.notify_target.session_key == session_key:
            return True
        if record.notify_target.channel and record.notify_target.chat_id:
            return (
                record.notify_target.channel == msg.channel
                and record.notify_target.chat_id == msg.chat_id
            )
        return False

    @staticmethod
    def _detect_spawn_hint(tool_calls: list[ToolCallRequest]) -> str | None:
        for tc in tool_calls:
            if tc.name == "spawn":
                args: dict = tc.arguments or {}
                label: str = args.get("label") or args.get("task", "")[:30]
                return f"\U0001f680 백그라운드 작업 시작: {label}"
        return None

    @staticmethod
    def _classify_request(user_text: str) -> AssistantPlan:
        """메시지를 분석하여 처리 경로를 결정합니다 (규칙 기반, M2).

        Returns:
            AssistantPlan: kind이 direct_answer / clarification / planned_workflow 중 하나.
        """
        text = user_text.strip()

        if not text or text.startswith("/"):
            return AssistantPlan(kind="direct_answer")

        _VAGUE_RE = re.compile(
            (
                r"^(그거|그것|이거|이것|저거|저것)\s*"
                r"(해(줘|주세요|주십시오)|처리|진행|실행)\s*[.!]?$"
                r"|^(do|fix|handle)\s+(it|that|this)\s*[.!]?$"
            ),
            re.IGNORECASE,
        )
        if _VAGUE_RE.match(text):
            return AssistantPlan(
                kind="clarification",
                clarification_question="요청이 무엇인지 좀 더 구체적으로 알려주시겠어요?",
                summary="요청이 너무 모호합니다.",
            )

        lower = text.lower()

        if len(text) < 15:
            return AssistantPlan(kind="direct_answer")

        # --- 특수 step kind 감지 (우선순위: wait_until > request_approval > ask_user) ---

        _WAIT_UNTIL_RE = re.compile(
            r"\d+\s*(?:분|시간|일)\s*(?:후|뒤|있다가|지나서|지나고|지나면|지난\s*후)"
            r"|(?:tomorrow|내일)\s+\d{1,2}:\d{2}"
            r"|\bwait\s+(?:for\s+)?\d+\s*(?:min(?:utes?)?|hours?|days?)\b"
            r"|\bin\s+\d+\s*(?:min(?:utes?)?|hours?)\b"
            r"|\bafter\s+\d+\s*(?:min(?:utes?)?|hours?|days?)\b"
            r"|\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}",
            re.IGNORECASE,
        )
        if _WAIT_UNTIL_RE.search(text):
            wait_dt = parse_wait_until_time(text)
            return AssistantPlan(
                kind="planned_workflow",
                steps=[
                    PlanStep(
                        kind="wait_until",
                        description=text[:120],
                        step_meta={"iso_time": wait_dt.isoformat()},
                    ),
                    PlanStep(kind="research", description="대기 후 요청 수행", depends_on=[0]),
                    PlanStep(kind="send_result", description="결과 전달", depends_on=[1]),
                ],
                summary="대기 후 요청을 처리합니다.",
            )

        _APPROVAL_DETECT_RE = re.compile(
            r"확인\s*(?:후에?|받고|하고)"
            r"|승인\s*(?:후에?|받고)"
            r"|\bconfirm\s+before\b"
            r"|\bget\s+(?:my\s+)?approval\b"
            r"|진행해도\s*될까",
            re.IGNORECASE,
        )
        if _APPROVAL_DETECT_RE.search(text):
            return AssistantPlan(
                kind="planned_workflow",
                steps=[
                    PlanStep(kind="research", description="작업 내용 분석"),
                    PlanStep(
                        kind="request_approval",
                        description="작업 승인 요청",
                        step_meta={"prompt": f"다음 작업을 계속 진행할까요?\n\n{text[:120]}"},
                        depends_on=[0],
                    ),
                    PlanStep(kind="send_result", description="승인 후 결과 전달", depends_on=[1]),
                ],
                summary="승인 후 작업을 진행합니다.",
            )

        _ASK_USER_DETECT_RE = re.compile(
            r"물어보고|묻고\s+(?:나서|이후|그에?\s*맞게|조사|처리|진행|실행|알려|확인)|입력\s*받고"
            r"|\bask\s+me\b"
            r"|\bget\s+(?:my\s+)?input\b",
            re.IGNORECASE,
        )
        if _ASK_USER_DETECT_RE.search(text):
            return AssistantPlan(
                kind="planned_workflow",
                steps=[
                    PlanStep(
                        kind="ask_user",
                        description="사용자 입력 요청",
                        step_meta={"prompt": "계속 진행하기 위한 정보를 입력해 주세요."},
                    ),
                    PlanStep(kind="research", description="입력 기반 작업 수행", depends_on=[0]),
                    PlanStep(kind="send_result", description="결과 전달", depends_on=[1]),
                ],
                summary="사용자 입력 후 작업을 처리합니다.",
            )

        _SEQ_EN = re.compile(
            (
                r"\bfirst\b.{1,80}\bthen\b"
                r"|\bstep\s+\d\b"
                r"|\bafter\s+(?:that|which|this)\b"
                r"|\bfollowed\s+by\b"
                r"|\bonce\s+.{1,40}\bthen\b"
            ),
            re.IGNORECASE,
        )
        _SEQ_KO = re.compile(
            (
                r"먼저.{1,60}(?:그\s*다음|이후|그리고\s*나서)"
                r"|그\s*다음\s*에?\s"
                r"|이\s*후\s*에?\s"
                r"|단계\s*별"
                r"|첫\s*번째.{1,40}두\s*번째"
            ),
        )
        _SCHEDULE_RE = re.compile(
            (
                r"\bevery\s+(?:day|week|month|hour|minute|Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday)\b"
                r"|\b(?:schedule|remind)\b.{0,30}\b(?:me|every|daily|weekly|monthly)\b"
                r"|\bweekly\b|\bmonthly\b"
                r"|매일|매주|매월|매달|매시간|주기적으로|정기적으로"
                r"|매주\s*(?:월요일|화요일|수요일|목요일|금요일|토요일|일요일)"
            ),
            re.IGNORECASE,
        )
        _NUMBERED_RE = re.compile(r"(?:^|\n)\s*\d+[.)]\s+\S", re.MULTILINE)

        is_compound = bool(
            _SEQ_EN.search(lower)
            or _SEQ_KO.search(text)
            or _SCHEDULE_RE.search(text)
            or len(_NUMBERED_RE.findall(text)) >= 2
        )

        if is_compound:
            steps = [
                PlanStep(kind="research", description="요청 내용 분석 및 필요한 정보 수집"),
                PlanStep(kind="summarize", description="수집한 내용 정리", depends_on=[0]),
                PlanStep(kind="send_result", description="최종 결과 전달", depends_on=[1]),
            ]
            return AssistantPlan(
                kind="planned_workflow",
                steps=steps,
                summary="복합 요청으로 단계별 처리를 계획합니다.",
            )

        return AssistantPlan(kind="direct_answer")

    # ------------------------------------------------------------------
    # LLM 플래너 폴백
    # ------------------------------------------------------------------

    _LLM_PLANNER_SYSTEM: str = (
        "You are a request router. Analyse the user message and classify it.\n\n"
        "Return ONLY a JSON object — no markdown, no explanation:\n"
        '{"kind": "direct_answer" | "planned_workflow", "summary": "<one-line>", '
        '"steps": [{"kind": "<step_kind>", "description": "<what to do>", '
        '"depends_on": [<0-based>], "step_meta": {}}]}\n\n'
        "Step kinds: research, summarize, ask_user, request_approval, wait_until, send_result\n\n"
        "step_meta rules — include when the step kind requires it:\n"
        '- wait_until: set "iso_time" (ISO 8601 absolute datetime) OR "duration_minutes" (integer).\n'
        '  Prefer "iso_time" when a specific time is mentioned; prefer "duration_minutes" for relative delays.\n'
        '  Example: {"iso_time": "2026-04-03T15:00:00+09:00"} or {"duration_minutes": 30}\n'
        '- ask_user: set "prompt" to the exact question string shown to the user.\n'
        '  Example: {"prompt": "어떤 형식으로 결과를 받으시겠습니까?"}\n'
        '- request_approval: set "prompt" to the approval confirmation message.\n'
        '  Example: {"prompt": "다음 작업을 진행할까요?\\n\\n<task summary>"}\n'
        "- Other step kinds: omit step_meta or leave it as {}.\n\n"
        "Routing rules:\n"
        '- "direct_answer": simple questions, greetings, single-topic queries, factual lookups\n'
        '- "planned_workflow": requires multiple distinct phases, complex multi-part tasks with '
        "sequential dependencies\n\n"
        "For planned_workflow, always end steps with send_result.\n"
        "Return ONLY the JSON object."
    )

    @staticmethod
    def _normalize_llm_plan_metadata(plan: AssistantPlan) -> AssistantPlan:
        """LLM 폴백 플랜의 step_meta를 보충합니다.

        LLM이 step_meta를 생략했을 때 description에서 도출 가능한 메타데이터를 채웁니다.
        - ``wait_until``: ``iso_time``/``duration_minutes`` 모두 없으면 description 파싱
        - ``ask_user`` / ``request_approval``: ``prompt`` 없으면 description 사용

        기존 step_meta가 이미 올바른 키를 가지면 변경하지 않습니다.
        """
        if plan.kind != "planned_workflow":
            return plan

        updated = False
        normalized: list[PlanStep] = []
        for step in plan.steps:
            if step.kind == "wait_until" and "iso_time" not in step.step_meta and "duration_minutes" not in step.step_meta:
                dt = parse_wait_until_time(step.description)
                normalized.append(step.model_copy(update={"step_meta": {**step.step_meta, "iso_time": dt.isoformat()}}))
                updated = True
            elif step.kind in ("ask_user", "request_approval") and "prompt" not in step.step_meta:
                normalized.append(step.model_copy(update={"step_meta": {**step.step_meta, "prompt": step.description}}))
                updated = True
            else:
                normalized.append(step)

        if not updated:
            return plan
        return plan.model_copy(update={"steps": normalized})

    @staticmethod
    def _is_nontrivial_for_llm_fallback(text: str) -> bool:
        """LLM 폴백을 호출할 가치가 있는 비자명 요청인지 판단합니다.

        규칙 기반이 이미 짧은 텍스트(<15자)를 필터링하므로, 여기서는
        보다 넉넉한 임계값(30자)을 기준으로 한다.
        """
        return len(text.strip()) >= 30

    async def _llm_classify_fallback(self, user_text: str) -> AssistantPlan | None:
        """LLM에 구조화된 AssistantPlan JSON을 요청합니다.

        파싱 실패, 네트워크 오류, 빈 응답 등 모든 예외 상황에서 None을 반환하며
        호출자가 안전하게 원래 ``direct_answer``로 폴백할 수 있도록 한다.
        """
        messages: list[dict[str, Any]] = [
            {"role": "system", "content": self._LLM_PLANNER_SYSTEM},
            {"role": "user", "content": user_text},
        ]
        try:
            response: LLMResponse = await self._provider.chat_with_retry(
                messages=messages,
                tools=None,
                model=self._model,
                max_tokens=512,
                temperature=0.0,
                failover_manager=self._failover,
                provider_name=self._provider_name,
            )
        except Exception as exc:
            logger.warning("LLM 플래너 폴백 호출 실패: {}", exc)
            return None

        if response.finish_reason == "error" or not response.content:
            return None

        raw: str = response.content.strip()
        md_match = re.search(r"```(?:json)?\s*([\s\S]+?)```", raw)
        if md_match:
            raw = md_match.group(1).strip()

        try:
            data: dict = json.loads(raw)
            plan: AssistantPlan = AssistantPlan.model_validate(data)
        except Exception as exc:
            logger.warning(
                "LLM 플래너 폴백 JSON 파싱 실패: {} — raw={!r}", exc, raw[:200]
            )
            return None

        if plan.kind == "planned_workflow" and not plan.steps:
            return None

        return self._normalize_llm_plan_metadata(plan)

    async def _classify_request_with_llm_fallback(
        self,
        user_text: str,
        *,
        observer: AgentLoopObserver | None = None,
    ) -> AssistantPlan:
        """규칙 기반 플래너로 분류하고, 결과가 ``direct_answer`` + 비자명 요청이면 LLM 폴백을 시도합니다.

        빠른 경로(규칙 기반)에서 이미 ``planned_workflow`` 또는 ``clarification``이 결정되면
        LLM을 호출하지 않는다. LLM 폴백이 ``direct_answer``를 반환하면 원래 결과를 유지한다.

        observer가 있으면 최종 결정 후 ``on_planner_decision``을 호출한다.
        """
        plan: AssistantPlan = self._classify_request(user_text)
        if plan.kind != "direct_answer":
            self._emit_planner_decision(observer, plan.kind, fallback_engaged=False)
            return plan
        if not self._is_nontrivial_for_llm_fallback(user_text):
            self._emit_planner_decision(observer, plan.kind, fallback_engaged=False)
            return plan

        fallback: AssistantPlan | None = await self._llm_classify_fallback(user_text)
        if fallback is not None and fallback.kind != "direct_answer":
            logger.debug("LLM 플래너 폴백 적용: kind={}", fallback.kind)
            self._emit_planner_decision(observer, fallback.kind, fallback_engaged=True)
            return fallback
        self._emit_planner_decision(observer, plan.kind, fallback_engaged=True)
        return plan

    @staticmethod
    def _emit_planner_decision(
        observer: AgentLoopObserver | None, kind: str, *, fallback_engaged: bool
    ) -> None:
        if observer is None:
            return
        try:
            observer.on_planner_decision(kind, fallback_engaged)
        except Exception as e:
            logger.warning("AgentLoop observer on_planner_decision 실패: {}", e)

    @staticmethod
    def _format_plan(plan: AssistantPlan) -> str:
        """AssistantPlan을 사용자 표시용 텍스트로 변환합니다."""
        lines = ["📋 **처리 계획**"]
        if plan.summary:
            lines.append(plan.summary)
        lines.append("")
        for i, step in enumerate(plan.steps):
            dep_str = f" (선행: {step.depends_on})" if step.depends_on else ""
            notify_str = " 🔔" if step.notify else ""
            lines.append(f"{i + 1}. [{step.kind}] {step.description}{dep_str}{notify_str}")
        return "\n".join(lines)

    async def execute_existing_workflow(self, workflow_id: str) -> bool:
        """queued 상태의 manual 워크플로우를 비동기 태스크로 실행합니다.

        WorkflowRedispatcher가 호출하는 진입점.  성공 시 True, 실패(record 없음,
        source_kind 불일치 등) 시 False를 반환합니다.
        """
        record = self._workflow_runtime.store.get(workflow_id)
        if record is None or record.source_kind != "manual":
            return False

        started = self._workflow_runtime.start(workflow_id)
        if started is None:
            return False

        channel: str = record.notify_target.channel or "cli"
        chat_id: str = record.notify_target.chat_id or "direct"
        session_key: str | None = record.notify_target.session_key or None

        parsed_plan: AssistantPlan | None = self._parse_workflow_plan(record)

        if parsed_plan is not None and parsed_plan.kind == "planned_workflow" and parsed_plan.steps:
            _ = asyncio.create_task(
                self._run_planned_workflow_steps(
                    workflow_id=workflow_id,
                    plan=parsed_plan,
                    channel=channel,
                    chat_id=chat_id,
                    session_key=session_key,
                )
            )
            return True

        async def _run() -> None:
            try:
                final_content: str = await self._run_workflow_prompt(
                    prompt=record.goal,
                    channel=channel,
                    chat_id=chat_id,
                    session_key=session_key,
                )
                result_text: str = final_content or "워크플로우가 완료되었습니다."
                _ = self._workflow_runtime.annotate_result(workflow_id, result_text)
                _ = self._workflow_runtime.clear_step_cursor(workflow_id)
                _ = self._workflow_runtime.complete(workflow_id)

                await self._publish_workflow_outbound(
                    workflow_id=workflow_id,
                    channel=channel,
                    chat_id=chat_id,
                    content=f"✅ 워크플로우 완료 (`{workflow_id}`)\n\n{result_text}",
                )
            except Exception as exc:
                logger.error("manual 워크플로우 {} 실행 실패: {}", workflow_id, exc)
                _ = self._workflow_runtime.fail(workflow_id, last_error=str(exc))
                await self._publish_workflow_outbound(
                    workflow_id=workflow_id,
                    channel=channel,
                    chat_id=chat_id,
                    content=f"❌ 워크플로우 실패 (`{workflow_id}`): {exc}",
                )

        _ = asyncio.create_task(_run())
        return True

    def _parse_workflow_plan(self, record: WorkflowRecord) -> AssistantPlan | None:
        raw_plan: object = record.metadata.get("plan")
        if not isinstance(raw_plan, dict):
            return None
        try:
            return AssistantPlan.model_validate(raw_plan)
        except Exception as exc:
            logger.warning("workflow {} plan 파싱 실패: {}", record.workflow_id, exc)
            return None

    async def _run_workflow_prompt(
        self,
        *,
        prompt: str,
        channel: str,
        chat_id: str,
        session_key: str | None,
    ) -> str:
        key: str = session_key or f"{channel}:{chat_id}"
        self._set_tool_context(channel=channel, chat_id=chat_id, session_key=key)
        session: Session = self._sessions.get_or_create(key=key)
        history: list[dict[str, Any]] = session.get_history(max_messages=self._memory_window)
        messages: list[dict[str, Any]] = self._context.build_messages(
            history=history,
            current_messages=prompt,
            channel=channel,
            chat_id=chat_id,
        )
        final_content, _, all_msg, _ = await self._run_agent_loop(
            messages,
            _session_key=key,
            _channel=channel,
        )
        self._save_turn(session=session, messages=all_msg, skip=(1 + len(history)))
        self._sessions.save(session)
        return final_content or ""

    async def _publish_workflow_outbound(
        self,
        *,
        workflow_id: str,
        channel: str,
        chat_id: str,
        content: str,
    ) -> None:
        await self._bus.publish_outbound(
            OutboundMessage(
                channel=channel,
                chat_id=chat_id,
                content=content,
            )
        )
        _ = self._workflow_runtime.mark_notified(
            workflow_id,
            channel=channel,
            chat_id=chat_id,
        )

    async def _run_planned_workflow_steps(
        self,
        *,
        workflow_id: str,
        plan: AssistantPlan,
        channel: str,
        chat_id: str,
        session_key: str | None,
    ) -> None:
        record: WorkflowRecord | None = self._workflow_runtime.store.get(workflow_id)
        if record is None:
            return

        current_step_index = record.metadata.get("currentStepIndex", 0)
        if not isinstance(current_step_index, int) or current_step_index < 0:
            current_step_index = 0

        last_result = record.metadata.get("lastStepResultSummary", "")
        current_result: str = last_result if isinstance(last_result, str) else ""

        try:
            while current_step_index < len(plan.steps):
                step = plan.steps[current_step_index]
                _ = self._workflow_runtime.update_step_cursor(
                    workflow_id,
                    step_index=current_step_index,
                    step_kind=step.kind,
                )

                outcome, current_result = await self._execute_plan_step(
                    workflow_id=workflow_id,
                    record_goal=record.goal,
                    step=step,
                    previous_result=current_result,
                    channel=channel,
                    chat_id=chat_id,
                    session_key=session_key,
                )

                if outcome != "continue":
                    return

                _ = self._workflow_runtime.annotate_step_result(workflow_id, current_result)
                _ = self._workflow_runtime.annotate_result(workflow_id, current_result)
                next_idx = current_step_index + 1
                next_kind = plan.steps[next_idx].kind if next_idx < len(plan.steps) else ""
                _ = self._workflow_runtime.update_step_cursor(
                    workflow_id,
                    step_index=next_idx,
                    step_kind=next_kind,
                )
                current_step_index = next_idx

            _ = self._workflow_runtime.clear_step_cursor(workflow_id)
            _ = self._workflow_runtime.complete(workflow_id)
            await self._publish_workflow_outbound(
                workflow_id=workflow_id,
                channel=channel,
                chat_id=chat_id,
                content=f"✅ 워크플로우 완료 (`{workflow_id}`)\n\n{current_result or '워크플로우가 완료되었습니다.'}",
            )
        except Exception as exc:
            logger.error("planned workflow {} step 실행 실패: {}", workflow_id, exc)
            _ = self._workflow_runtime.fail(workflow_id, last_error=str(exc))
            await self._publish_workflow_outbound(
                workflow_id=workflow_id,
                channel=channel,
                chat_id=chat_id,
                content=f"❌ 워크플로우 실패 (`{workflow_id}`): {exc}",
            )

    async def _execute_plan_step(
        self,
        *,
        workflow_id: str,
        record_goal: str,
        step: PlanStep,
        previous_result: str,
        channel: str,
        chat_id: str,
        session_key: str | None,
    ) -> tuple[str, str]:
        if step.kind == "research":
            prompt = (
                f"다음 workflow 목표를 수행하기 위한 조사 단계를 실행하세요.\n\n"
                f"목표: {record_goal}\n"
                f"현재 step: {step.description}\n\n"
                "아직 최종 전달은 하지 말고, 핵심 조사 결과만 간결하게 정리하세요."
            )
            result = await self._run_workflow_prompt(
                prompt=prompt,
                channel=channel,
                chat_id=chat_id,
                session_key=session_key,
            )
            return "continue", result or previous_result

        if step.kind == "summarize":
            source_text = previous_result or record_goal
            prompt = (
                "다음 조사 결과를 사용자가 바로 이해할 수 있게 간결하게 요약하세요.\n\n"
                f"조사 결과:\n{source_text}\n\n"
                f"요약 지시: {step.description}"
            )
            result = await self._run_workflow_prompt(
                prompt=prompt,
                channel=channel,
                chat_id=chat_id,
                session_key=session_key,
            )
            return "continue", result or source_text

        if step.kind == "send_result":
            result_text = previous_result or "전달할 결과가 없습니다."
            _ = self._workflow_runtime.clear_step_cursor(workflow_id)
            _ = self._workflow_runtime.annotate_result(workflow_id, result_text)
            _ = self._workflow_runtime.complete(workflow_id)
            await self._publish_workflow_outbound(
                workflow_id=workflow_id,
                channel=channel,
                chat_id=chat_id,
                content=f"✅ 워크플로우 완료 (`{workflow_id}`)\n\n{result_text}",
            )
            return "completed", result_text

        if step.kind == "ask_user":
            meta_prompt = step.step_meta.get("prompt")
            prompt_text = meta_prompt if isinstance(meta_prompt, str) and meta_prompt else step.description
            message = (
                f"워크플로우 `{workflow_id}`는 `ask_user` 단계에서 사용자 입력을 기다립니다.\n"
                f"- {prompt_text}"
            )
            _ = self._workflow_runtime.wait_for_input(workflow_id)
            _ = self._workflow_runtime.annotate_step_result(workflow_id, message)
            _wf_sess_key: str = session_key or f"{channel}:{chat_id}"
            _wf_sess = self._sessions.get_or_create(key=_wf_sess_key)
            _wf_sess.metadata["waiting_workflow_id"] = workflow_id
            self._sessions.save(_wf_sess)
            await self._publish_workflow_outbound(
                workflow_id=workflow_id,
                channel=channel,
                chat_id=chat_id,
                content=message,
            )
            return "waiting", previous_result

        if step.kind == "request_approval":
            meta_prompt = step.step_meta.get("prompt")
            prompt_text = meta_prompt if isinstance(meta_prompt, str) and meta_prompt else step.description
            message = (
                f"워크플로우 `{workflow_id}`는 `request_approval` 단계에서 승인을 기다립니다.\n"
                f"- {prompt_text}\n\n"
                f"승인하려면 **y** 또는 **승인**, 거절하려면 **n** 또는 **거절**을 입력하세요."
            )
            _ = self._workflow_runtime.wait_for_input(workflow_id)
            _ = self._workflow_runtime.annotate_step_result(workflow_id, message)
            _ap_sess_key: str = session_key or f"{channel}:{chat_id}"
            _ap_sess = self._sessions.get_or_create(key=_ap_sess_key)
            _ap_sess.metadata["waiting_workflow_approval_id"] = workflow_id
            self._sessions.save(_ap_sess)
            await self._publish_workflow_outbound(
                workflow_id=workflow_id,
                channel=channel,
                chat_id=chat_id,
                content=message,
            )
            return "waiting", previous_result

        if step.kind == "wait_until":
            iso_time = step.step_meta.get("iso_time")
            duration_minutes = step.step_meta.get("duration_minutes")
            if isinstance(iso_time, str) and iso_time:
                next_run_dt = datetime.fromisoformat(iso_time)
                if next_run_dt.tzinfo is None:
                    next_run_dt = next_run_dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
            elif isinstance(duration_minutes, (int, float)) and duration_minutes > 0:
                next_run_dt = datetime.now().astimezone() + timedelta(minutes=duration_minutes)
            else:
                next_run_dt = parse_wait_until_time(step.description)
            next_run_at = next_run_dt.isoformat()
            message = (
                f"워크플로우 `{workflow_id}`의 `wait_until` 단계: {next_run_dt.strftime('%Y-%m-%d %H:%M %Z')} 에 재시도합니다.\n"
                f"- step: {step.description}"
            )
            _ = self._workflow_runtime.schedule_retry(
                workflow_id,
                next_run_at=next_run_at,
                last_error=message,
                increment_retries=False,
            )
            _ = self._workflow_runtime.annotate_step_result(workflow_id, message)
            await self._publish_workflow_outbound(
                workflow_id=workflow_id,
                channel=channel,
                chat_id=chat_id,
                content=message,
            )
            return "waiting", previous_result

        message = f"지원하지 않는 step kind: {step.kind}"
        _ = self._workflow_runtime.fail(workflow_id, last_error=message)
        await self._publish_workflow_outbound(
            workflow_id=workflow_id,
            channel=channel,
            chat_id=chat_id,
            content=f"❌ 워크플로우 실패 (`{workflow_id}`): {message}",
        )
        return "failed", previous_result

    async def close_mcp(self) -> None:
        """ "MCP 연결 종료"""
        if self._mcp_stack:
            try:
                await self._mcp_stack.aclose()
            except (RuntimeError, BaseExceptionGroup):
                pass  # MCP SDK cancel scope cleanup 로그는 시끄럽지만 무해함.

            self._mcp_stack = None

    def stop(self) -> None:
        """에이전트 루프 멈춤"""
        self._running = False
        logger.info("에이전트 루프 멈추는 중")
