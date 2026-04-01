"""스킬 서브에이전트의 도구 호출 승인 게이트."""

import asyncio
import json
import re
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.agent.hooks import (
    APPROVAL_REQUESTED,
    APPROVAL_RESOLVED,
    HookContext,
    HookRegistry,
    NoOpHookRegistry,
)
from shacs_bot.bus.events import OutboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.providers.base import LLMProvider

# ── pending approvals 레지스트리 ────────────────────────────────
# request_id → asyncio.Future[bool]
# manual 모드에서 서브에이전트가 사용자 응답을 기다리는 데 사용
_pending_approvals: dict[str, asyncio.Future[bool]] = {}


def resolve_approval(request_id: str, approved: bool) -> bool:
    """pending approval을 해결한다. 존재하면 True, 없으면 False."""
    future = _pending_approvals.pop(request_id, None)
    if future and not future.done():
        future.set_result(approved)
        return True
    return False


def get_pending_approval_for_session(session_key: str) -> str | None:
    """주어진 세션에 대기 중인 approval request_id를 반환. 없으면 None."""
    for req_id, future in _pending_approvals.items():
        if not future.done() and req_id.startswith(session_key + ":"):
            return req_id
    return None


# Tier 1: 읽기 전용 도구 — LLM 호출 없이 즉시 승인
ALWAYS_ALLOW: frozenset[str] = frozenset(
    {
        "read_file",
        "list_dir",
        "web_search",
        "web_fetch",
        "search_history",
    }
)

# Tier 1: 명백히 위험한 exec 패턴 — LLM 호출 없이 즉시 차단
ALWAYS_DENY_PATTERNS: list[re.Pattern[str]] = [
    re.compile(r"rm\s+(-rf|-fr)\s+[/~]"),
    re.compile(r"curl\s.*\|\s*(sh|bash)"),
    re.compile(r"mkfs\."),
    re.compile(r"dd\s+.*of=/dev/"),
    re.compile(r">\s*/dev/sd"),
    re.compile(r"chmod\s+777\s+/"),
]

CLASSIFIER_PROMPT: str = """\
당신은 보안 분류기입니다. 서브에이전트의 도구 호출이 사용자의 의도 범위 내에서 안전한지 판단합니다.

판단 기준:
1. 사용자 의도 범위: 사용자가 명시적으로 요청한 작업과 관련 있는가?
2. 폭발 반경: 되돌릴 수 없는 파괴적 작업인가?
3. 신뢰 경계: 외부 서비스로 데이터를 전송하는가?
4. 권한 에스컬레이션: 보안 검사를 우회하거나 의도하지 않은 리소스에 접근하는가?

반드시 다음 JSON으로만 답변:
{{"approved": true/false, "reason": "판단 이유"}}\
"""


@dataclass(frozen=True)
class ApprovalDecision:
    denied: bool
    reason: str


class ApprovalGate:
    """workspace 스킬의 도구 호출을 3단계 분류기(auto) 또는 사용자 승인(manual)으로 검사."""

    def __init__(
        self,
        mode: str,
        provider: LLMProvider,
        model: str,
        session_history: list[dict[str, Any]],
        bus: MessageBus,
        origin: dict[str, Any],
        skill_name: str,
        workspace: Path,
        hooks: HookRegistry | None = None,
    ):
        self._mode = mode
        self._provider = provider
        self._model = model
        self._session_history = session_history
        self._bus = bus
        self._origin = origin
        self._skill_name = skill_name
        self._workspace = workspace
        self._hooks: HookRegistry = hooks or NoOpHookRegistry()

    async def check(self, tool_name: str, arguments: dict[str, Any]) -> ApprovalDecision:
        if self._mode == "auto":
            return await self._check_auto(tool_name, arguments)
        if self._mode == "manual":
            return await self._check_manual(tool_name, arguments)
        return ApprovalDecision(denied=False, reason="")

    # ── auto: 3단계 분류기 ──────────────────────────────────────────

    async def _check_auto(self, tool_name: str, arguments: dict[str, Any]) -> ApprovalDecision:
        # Tier 1: 규칙 기반 즉시 판정
        tier1 = self._check_rules(tool_name, arguments)
        if tier1 is not None:
            await self._hooks.emit(
                HookContext(
                    event=APPROVAL_RESOLVED,
                    session_key=self._origin.get("session_key"),
                    channel=self._origin.get("channel"),
                    payload={
                        "tool": tool_name,
                        "tier": 1,
                        "denied": tier1.denied,
                        "reason": tier1.reason,
                    },
                )
            )
            return tier1

        # Tier 2: workspace 내 파일 쓰기
        if tool_name in ("write_file", "edit_file"):
            path = Path(arguments.get("path", "")).expanduser().resolve()
            if str(path).startswith(str(self._workspace)):
                decision = ApprovalDecision(denied=False, reason="workspace 내 파일")
                await self._hooks.emit(
                    HookContext(
                        event=APPROVAL_RESOLVED,
                        session_key=self._origin.get("session_key"),
                        channel=self._origin.get("channel"),
                        payload={
                            "tool": tool_name,
                            "tier": 2,
                            "denied": False,
                            "reason": decision.reason,
                        },
                    )
                )
                return decision

        # Tier 3: reasoning-blind LLM 분류기
        result = await self._check_llm(tool_name, arguments)
        await self._hooks.emit(
            HookContext(
                event=APPROVAL_RESOLVED,
                session_key=self._origin.get("session_key"),
                channel=self._origin.get("channel"),
                payload={
                    "tool": tool_name,
                    "tier": 3,
                    "denied": result.denied,
                    "reason": result.reason,
                },
            )
        )
        return result

    async def _check_llm(self, tool_name: str, arguments: dict[str, Any]) -> ApprovalDecision:
        """reasoning-blind LLM 분류기. 사용자 메시지 + 도구 호출만 전달."""
        filtered: list[dict[str, Any]] = []
        for msg in self._session_history:
            if msg.get("role") == "user":
                filtered.append(msg)
            elif msg.get("role") == "assistant" and msg.get("tool_calls"):
                filtered.append(
                    {"role": "assistant", "content": "", "tool_calls": msg["tool_calls"]}
                )

        args_str: str = json.dumps(arguments, ensure_ascii=False, indent=2)
        try:
            response = await asyncio.wait_for(
                self._provider.chat_with_retry(
                    messages=[
                        {"role": "system", "content": CLASSIFIER_PROMPT},
                        *filtered,
                        {
                            "role": "user",
                            "content": (
                                f"도구 호출 승인 요청:\n"
                                f"스킬: {self._skill_name}\n"
                                f"도구: {tool_name}\n"
                                f"인자: {args_str}"
                            ),
                        },
                    ],
                    model=self._model,
                ),
                timeout=30,
            )
            result = json.loads(response.content)
            approved = result.get("approved", False)
            reason = result.get("reason", "")
            logger.info(
                "ApprovalGate: {} → {} ({})",
                tool_name,
                "승인" if approved else "거부",
                reason,
            )
            return ApprovalDecision(denied=not approved, reason=reason)
        except Exception as e:
            logger.error("ApprovalGate: {} 판단 실패, 기본 거부: {}", tool_name, e)
            return ApprovalDecision(denied=True, reason=f"판단 실패: {e}")

    # ── manual: 사용자 직접 승인 ────────────────────────────────────

    async def _check_manual(self, tool_name: str, arguments: dict[str, Any]) -> ApprovalDecision:
        # Tier 1 규칙은 manual에서도 적용
        tier1 = self._check_rules(tool_name, arguments)
        if tier1 is not None:
            return tier1

        session_key: str = self._origin.get("session_key", "unknown")
        request_id: str = f"{session_key}:{uuid.uuid4().hex[:8]}"
        loop = asyncio.get_running_loop()
        future: asyncio.Future[bool] = loop.create_future()
        _pending_approvals[request_id] = future

        await self._hooks.emit(
            HookContext(
                event=APPROVAL_REQUESTED,
                session_key=session_key,
                channel=self._origin.get("channel"),
                payload={
                    "tool_name": tool_name,
                    "skill_name": self._skill_name,
                    "request_id": request_id,
                },
            )
        )

        args_str: str = json.dumps(arguments, ensure_ascii=False, indent=2)
        await self._bus.publish_outbound(
            OutboundMessage(
                channel=self._origin["channel"],
                chat_id=self._origin["chat_id"],
                content=(
                    f"\U0001f6e1 스킬 '{self._skill_name}'이 실행하려 합니다:\n"
                    f"도구: {tool_name}\n"
                    f"인자: {args_str}\n\n"
                    f"승인하려면 **y**, 거부하려면 **n**을 입력하세요. (60초 후 자동 거부)"
                ),
                metadata={"_approval_request": True, "_request_id": request_id},
            )
        )

        try:
            approved: bool = await asyncio.wait_for(future, timeout=60)
            reason: str = "사용자 승인" if approved else "사용자 거부"
            logger.info("ApprovalGate(manual): {} → {} ({})", tool_name, reason, request_id)
            await self._hooks.emit(
                HookContext(
                    event=APPROVAL_RESOLVED,
                    session_key=session_key,
                    channel=self._origin.get("channel"),
                    payload={"approved": approved, "reason": reason, "request_id": request_id},
                )
            )
            return ApprovalDecision(denied=not approved, reason=reason)
        except asyncio.TimeoutError:
            _pending_approvals.pop(request_id, None)
            logger.warning("ApprovalGate(manual): {} 타임아웃, 기본 거부", tool_name)
            await self._hooks.emit(
                HookContext(
                    event=APPROVAL_RESOLVED,
                    session_key=session_key,
                    channel=self._origin.get("channel"),
                    payload={"approved": False, "reason": "timeout", "request_id": request_id},
                )
            )
            return ApprovalDecision(denied=True, reason="승인 타임아웃 (60초)")

    # ── 공통: Tier 1 규칙 ───────────────────────────────────────────

    @staticmethod
    def _check_rules(tool_name: str, arguments: dict[str, Any]) -> ApprovalDecision | None:
        """Tier 1 규칙 기반 즉시 판정. 결정되면 ApprovalDecision, 아니면 None."""
        if tool_name in ALWAYS_ALLOW:
            return ApprovalDecision(denied=False, reason="")

        if tool_name == "exec":
            cmd: str = arguments.get("command", "")
            for pattern in ALWAYS_DENY_PATTERNS:
                if pattern.search(cmd):
                    logger.warning("ApprovalGate: DENY (패턴) {} — {}", tool_name, cmd[:100])
                    return ApprovalDecision(denied=True, reason=f"위험 패턴: {pattern.pattern}")

        return None
