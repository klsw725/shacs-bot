"""백그라운드 테스크 실행을 위한 Subagent 관리자"""

import asyncio
import json
import uuid
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.agent.approval import ALWAYS_ALLOW, ApprovalGate
from shacs_bot.agent.context import ContextBuilder
from shacs_bot.agent.skills import SkillsLoader
from shacs_bot.agent.tools.registry import ToolRegistry, create_default_tools
from shacs_bot.bus.events import InboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.config.schema import ExecToolConfig
from shacs_bot.providers.base import LLMProvider, LLMResponse
from shacs_bot.utils.helpers import build_assistant_message


@dataclass(frozen=True)
class SubagentRole:
    system_prompt: str
    allowed_tools: list[str]  # 비어있으면 전체 허용
    max_iterations: int = 15


RESEARCHER_PROMPT = """\
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

ANALYST_PROMPT = """\
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

EXECUTOR_PROMPT = """\
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


@dataclass
class SubagentTask:
    task_id: str
    label: str
    role: str
    original_task: str
    started_at: datetime
    asyncio_task: asyncio.Task[None]


SUBAGENT_ROLES: dict[str, SubagentRole] = {
    "researcher": SubagentRole(
        system_prompt=RESEARCHER_PROMPT,
        allowed_tools=[
            "read_file",
            "list_dir",
            "exec",
            "web_search",
            "web_fetch",
            "search_history",
        ],
        max_iterations=10,
    ),
    "analyst": SubagentRole(
        system_prompt=ANALYST_PROMPT,
        allowed_tools=[
            "read_file",
            "list_dir",
            "exec",
            "web_search",
            "web_fetch",
            "search_history",
        ],
        max_iterations=10,
    ),
    "executor": SubagentRole(
        system_prompt=EXECUTOR_PROMPT,
        allowed_tools=[],  # 전체 허용
        max_iterations=15,
    ),
}


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
    ):
        self._provider: LLMProvider = provider
        self._workspace: Path = workspace
        self._bus: MessageBus = bus
        self._model: str = model or provider.get_default_model()
        self._brave_api_key: str | None = brave_api_key
        self._web_proxy: str | None = web_proxy
        self._exec_config: ExecToolConfig = exec_config or ExecToolConfig()
        self._restrict_to_workspace: bool = restrict_to_workspace

        self._skill_approval: str = "auto"
        self._running_tasks: dict[str, SubagentTask] = {}
        self._session_tasks: dict[str, set[str]] = {}  # session_key -> {task_id, ...}

    @property
    def skill_approval(self) -> str:
        return self._skill_approval

    @skill_approval.setter
    def skill_approval(self, value: str) -> None:
        self._skill_approval = value

    async def spawn(
        self,
        task: str,
        label: str | None = None,
        role: str = "executor",
        origin_channel: str = "cli",
        origin_chat_id: str = "direct",
        session_key: str | None = None,
        origin_metadata: dict[str, Any] | None = None,
    ) -> str:
        """새로운 서브에이전트를 생성하여 주어진 작업을 실행합니다."""
        task_id: str = str(uuid.uuid4())[:8]
        display_label: str = label or task[:30] + ("..." if len(task) > 30 else "")
        origin: dict[str, Any] = {
            "channel": origin_channel,
            "chat_id": origin_chat_id,
            "metadata": origin_metadata or {},
            "session_key": session_key,
        }

        bg_task: asyncio.Task[None] = asyncio.create_task(
            self._run_subagent(task_id, task, display_label, origin, role=role)
        )
        self._running_tasks[task_id] = SubagentTask(
            task_id=task_id,
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

        logger.info(f"서브에이전트 [{task_id}] 생성됨: {display_label}")
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
    ) -> str:
        """스킬을 서브에이전트로 실행한다."""
        task_id: str = str(uuid.uuid4())[:8]
        origin: dict[str, Any] = {
            "channel": origin_channel,
            "chat_id": origin_chat_id,
            "metadata": origin_metadata or {},
            "session_key": session_key,
        }

        bg_task: asyncio.Task[None] = asyncio.create_task(
            self._run_skill(task_id, task, label, origin, skill_name, skill_path)
        )
        self._running_tasks[task_id] = SubagentTask(
            task_id=task_id,
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
        task: str,
        label: str,
        origin: dict[str, Any],
        skill_name: str,
        skill_path: str,
    ) -> None:
        """서브에이전트로 스킬을 실행한다. workspace 스킬은 승인 게이트 적용."""
        logger.info("스킬 서브에이전트 [{}] 실행 시작: {} ({})", task_id, label, skill_name)

        try:
            # 1. 스킬 내용 로드
            skill_content: str = Path(skill_path).expanduser().read_text(encoding="utf-8")

            # 2. 출처 확인 → 승인 게이트 필요 여부
            source: str | None = SkillsLoader(self._workspace).get_skill_source(skill_name)
            needs_approval: bool = (source != "builtin") and (self._skill_approval != "off")

            # 3. 도구 생성 — spawn 도구 제외 (재귀 방지)
            all_tools = create_default_tools(
                workspace=self._workspace,
                restrict_to_workspace=self._restrict_to_workspace,
                exec_config=self._exec_config,
                brave_api_key=self._brave_api_key,
                web_proxy=self._web_proxy,
            )
            tools: ToolRegistry = ToolRegistry()
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
                    mode=self._skill_approval,
                    provider=self._provider,
                    model=self._model,
                    session_history=session_history,
                    bus=self._bus,
                    origin=origin,
                    skill_name=skill_name,
                    workspace=self._workspace,
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
                                tool_call.name, tool_call.arguments,
                            )
                            if decision.denied:
                                logger.info(
                                    "스킬 서브에이전트 [{}] 도구 거부: {} ({})",
                                    task_id, tool_call.name, decision.reason,
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
                            task_id, tool_call.name, args_str,
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
            await self._announce_result(
                task_id=task_id, label=label, task=task,
                result=final_result, origin=origin, status="ok",
            )
        except Exception as e:
            logger.error("스킬 서브에이전트 [{}] 실패: {}", task_id, e)
            await self._announce_result(
                task_id=task_id, label=label, task=task,
                result=f"Error: {e}", origin=origin, status="error",
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
        task: str,
        label: str,
        origin: dict[str, str],
        role: str = "executor",
    ) -> None:
        """서브에이전트를 실행합니다. 그리고 완료되면 결과를 보고합니다."""
        role_config: SubagentRole = SUBAGENT_ROLES.get(role, SUBAGENT_ROLES["executor"])
        logger.info(f"서브에이전트 [{task_id}] 실행 시작: {label} (역할: {role})")

        try:
            all_tools = create_default_tools(
                workspace=self._workspace,
                restrict_to_workspace=self._restrict_to_workspace,
                exec_config=self._exec_config,
                brave_api_key=self._brave_api_key,
                web_proxy=self._web_proxy,
            )

            tools: ToolRegistry = ToolRegistry()
            for tool in all_tools:
                if not role_config.allowed_tools or tool.name in role_config.allowed_tools:
                    tools.register(tool)

            system_prompt: str = self._build_subagent_prompt(role_config)
            messages: list[dict[str, Any]] = [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": task},
            ]

            max_iterations: int = role_config.max_iterations
            final_result: str | None = None

            for iteration in range(max_iterations):
                response: LLMResponse = await self._provider.chat_with_retry(
                    messages=messages,
                    tools=tools.get_definitions(),
                    model=self._model,
                )
                if response.has_tool_calls:
                    # 도구 호출 어시스턴트 메시지 추가
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

                    # Execute tools
                    for tool_call in response.tool_calls:
                        args_str: str = json.dumps(tool_call.arguments, ensure_ascii=False)
                        logger.debug(
                            "서브에이전트 [{}] 실행: {} 인자: {}", task_id, tool_call.name, args_str
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

            logger.info("서브에이전트 [{}]가 성공적으로 완료되었습니다", task_id)
            await self._announce_result(
                task_id=task_id,
                label=label,
                task=task,
                result=final_result,
                origin=origin,
                status="ok",
            )
        except Exception as e:
            error_msg: str = f"Error: {str(e)}"
            logger.error("서브에이전트 [{}] 실패: {}", task_id, e)
            await self._announce_result(
                task_id=task_id,
                label=label,
                task=task,
                result=error_msg,
                origin=origin,
                status="error",
            )

    def _build_subagent_prompt(self, role_config: SubagentRole) -> str:
        """역할 기반 시스템 프롬프트를 작성"""
        time_ctx: str = ContextBuilder.build_runtime_context(None, None)
        parts: list[str] = [
            role_config.system_prompt,
            f"\n## 환경\n{time_ctx}\n\n## Workspace\n{self._workspace}",
        ]

        skills_summary: str = SkillsLoader(self._workspace).build_skills_summary()
        if skills_summary:
            parts.append(
                f"## 스킬을 사용하려면 read_file로 SKILL.md를 읽으세요.\n\n{skills_summary}"
            )

        return "\n\n".join(parts)

    async def _announce_result(
        self, task_id: str, label: str, task: str, result: str, origin: dict[str, Any], status: str
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

        # 메인 에이전트를 트리거하기 위해 시스템 메시지로 주입
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
