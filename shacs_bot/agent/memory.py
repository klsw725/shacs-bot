"""영구 agent 메모리를 위한 메모리 시스템"""

import asyncio
import json
import weakref
from pathlib import Path
from typing import Any, Callable

from loguru import logger

from shacs_bot.agent.session.manager import Session, SessionManager
from shacs_bot.providers.base import LLMProvider, LLMResponse
from shacs_bot.utils import ensure_dir
from shacs_bot.utils.helpers import estimate_message_tokens, estimate_prompt_tokens_chain


class MemoryStore:
    """이중 계층 메모리: MEMORY.md(장기 사실) + HISTORY.md(grep 검색 가능한 로그)."""

    _SAVE_MEMORY_TOOL = [
        {
            "type": "function",
            "function": {
                "name": "save_memory",
                "description": "메모리 통합 결과를 영구 저장소에 저장합니다.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "history_entry": {
                            "type": "string",
                            "description": "주요 사건/결정/주제를 요약한 하나의 문단(2~5문장). [YYYY-MM-DD HH:MM] 형식으로 시작하세요. grep 검색에 유용한 세부 정보를 포함하세요.",
                        },
                        "memory_update": {
                            "type": "string",
                            "description": "마크다운 형식의 전체 업데이트된 장기 메모리입니다. 기존의 모든 사실에 새로운 사실을 추가해 포함하세요. 새로 추가할 내용이 없다면 변경 없이 그대로 반환하세요.",
                        },
                    },
                    "required": ["history_entry", "memory_update"],
                },
            },
        }
    ]

    def __init__(self, workspace: Path):
        self._memory_dir: Path = ensure_dir(workspace / "memory")
        self._memory_file: Path = self._memory_dir / "MEMORY.md"
        self._history_file: Path = self._memory_dir / "HISTORY.md"

    def read_long_term(self) -> str:
        if self._memory_file.exists():
            return self._memory_file.read_text(encoding="utf-8")

        return ""

    def write_long_term(self, content: str) -> None:
        self._memory_file.write_text(content, encoding="utf-8")

    def append_history(self, entry: str) -> None:
        with open(self._history_file, "a", encoding="utf-8") as f:
            f.write(entry.rstrip() + "\n\n")

    def get_memory_context(self) -> str:
        long_term: str = self.read_long_term()
        return f"## Long-term Memory\n{long_term}" if long_term else ""

    async def consolidate(
        self,
        messages: list[dict],
        provider: LLMProvider,
        model: str,
    ) -> bool:
        """LLM 툴 호출을 통해 오래된 메시지를 MEMORY.md와 HISTORY.md로 통합합니다."""
        if not messages:
            return True

        current_memory: str = self.read_long_term()
        prompt: str = f"""
            이 대화를 처리하고, 통합 결과를 담아 save_memory 도구를 호출하세요.
    
            ## 현재 장기 기억(Long-term Memory)
            {current_memory or "(비어 있음)"}
            
            ## 처리할 대화
            {self._format_messages(messages)}
        """

        # 성공 시(아무 작업도 하지 않은 경우 포함) True를 반환하고, 실패 시 False를 반환합니다.
        if archive_all:
            old_messages: list[dict[str, Any]] = session.messages
            keep_count: int = 0
            logger.info(f"메모리 통합(archive_all): {len(session.messages)}개의 메시지 ")
        else:
            keep_count: int = memory_window // 2
            if len(session.messages) <= keep_count:
                return True

            if (len(session.messages) - session.last_consolidated) <= 0:
                return True

            old_messages: list[dict[str, Any]] = session.messages[
                session.last_consolidated : -keep_count
            ]

            if not old_messages:
                return True

            logger.info("메모리 통합: {}개 통합 대상, {}개 유지", len(old_messages), keep_count)

        lines: list[str] = []

        for m in old_messages:
            if not m.get("content"):
                continue

            tools: str = f" [tools: {', '.join(m['tools_used'])}" if m.get("tools_used") else ""
            lines.append(
                f"[{m.get('timestamp', '?')[:16]}] {m['role'].upper()}{tools}: {m['content']}"
            )

        current_memory: str = self.read_long_term()
        prompt: str = f"""
            이 대화를 처리하고, 통합 결과를 담아 save_memory 도구를 호출하세요.
    
            ## 현재 장기 기억(Long-term Memory)
            {current_memory or "(비어 있음)"}
            
            ## 처리할 대화
            {chr(10).join(lines)}
        """

        try:
            response: LLMResponse = await provider.chat_with_retry(
                messages=[
                    {
                        "role": "system",
                        "content": "You are a memory consolidation agent. Call the save_memory tool with your consolidation of the conversation.",
                    },
                    {"role": "user", "content": prompt},
                ],
                tools=self._SAVE_MEMORY_TOOL,
                model=model,
                tool_choice="required",
            )
            if not response.has_tool_calls:
                logger.warning("메모리 통합: LLM이 save_memory를 호출하지 않아 건너뜁니다.")
                return False

            args: dict[str, Any] | str = self._normalize_save_memory_args(
                response.tool_calls[0].arguments
            )
            if args is None:
                logger.warning(
                    "메모리 통합: 예상지 않은 인자 타입. 비어있거나 list, dict 형태가 아닙니다e"
                )
                return False

            entry: Any = args.get("history_entry")
            if entry:
                self.append_history(self._ensure_text(entry))

            update: Any = args.get("memory_update")
            if update:
                update = self._ensure_text(update)
                if update != current_memory:
                    self.write_long_term(update)

            logger.info(f"메모리 통합 완료: 총 {len(messages)}개의 메시지")
            return True
        except Exception:
            logger.exception("메모리 통합 실패")
            return False

    def _format_messages(self, messages) -> str:
        lines: list[str] = []

        for message in messages:
            if not message.get("content"):
                continue

            tools: str = (
                f" [tools: {', '.join(message['tools_used'])}]" if message.get("tools_used") else ""
            )
            lines.append(
                f"[{message.get('timestamp', '?')[:16]}] {message['role'].upper()}{tools}: {message['content']}"
            )

        return "\n".join(lines)

    def _normalize_save_memory_args(self, arguments) -> dict[str, Any] | None:
        """provider의 tool-call 인자를 예상되는 dict 형태로 정규화한다."""
        if isinstance(arguments, str):
            arguments = json.loads(arguments)
        elif isinstance(arguments, list):
            return arguments[0] if arguments and isinstance(arguments[0], dict) else None

        return arguments if isinstance(arguments, dict) else None

    def _ensure_text(self, entry: Any) -> str:
        """파일 저장을 위해 tool-call 페이로드 값을 텍스트 형식으로 정규화한다."""
        return entry if isinstance(entry, str) else json.dumps(entry, ensure_ascii=False)


class MemoryConsolidator:
    """통합(consolidation) 정책, 락(lock) 관리, 그리고 세션 오프셋 업데이트를 담당한다."""

    _MAX_CONSOLIDATION_ROUNDS: int = 5

    def __init__(
        self,
        workspace: Path,
        provider: LLMProvider,
        model: str,
        sessions: SessionManager,
        context_window_tokens: int,
        build_messages: Callable[..., list[dict[str, Any]]],
        get_tool_definitions: Callable[[], list[dict[str, Any]]],
    ):
        self._store: MemoryStore = MemoryStore(workspace)

        self._provider: LLMProvider = provider
        self._model: str = model
        self._sessions: SessionManager = sessions
        self._context_window_tokens: int = context_window_tokens
        self._build_messages: Callable[..., list[dict[str, Any]]] = build_messages
        self._get_tool_definitions: Callable[[], list[dict[str, Any]]] = get_tool_definitions

        self._locks: weakref.WeakValueDictionary[str, asyncio.Lock] = weakref.WeakValueDictionary()

    def get_lock(self, session_key: str) -> asyncio.Lock:
        """하나의 세션에 대해 공유되는 consolidation lock을 반환한다."""
        return self._locks.setdefault(session_key, asyncio.Lock())

    async def consolidate_messages(self, messages: list[dict[str, Any]]) -> bool:
        """선택된 메시지 청크를 영구 메모리에 아카이브한다."""
        return await self._store.consolidate(messages, self._provider, self._model)

    def pick_consolidation_boundary(
        self,
        session: Session,
        tokens_to_remove: int,
    ) -> tuple[int, int] | None:
        """충분한 오래된 프롬프트 토큰을 제거할 수 있는 사용자 턴(user-turn) 경계를 선택한다."""
        start: int = session.last_consolidated
        if start >= len(session.messages) or tokens_to_remove <= 0:
            return None

        removed_tokens: int = 0
        last_boundary: tuple[int, int] | None = None

        for idx in range(start, len(session.messages)):
            message: dict[str, Any] = session.messages[idx]
            if idx > start and message.get("role") == "user":
                last_boundary = (idx, removed_tokens)
                if removed_tokens >= tokens_to_remove:
                    return last_boundary

            removed_tokens += estimate_message_tokens(message)

        return last_boundary

    def estimate_session_prompt_tokens(self, session: Session) -> tuple[int, str]:
        """일반 세션 히스토리 뷰에서 현재 프롬프트 크기를 추정한다."""
        history: list[dict[str, Any]] = session.get_history(max_messages=0)
        channel, chat_id = session.key.split(":", 1) if ":" in session.key else (None, None)
        probe_messages: list[dict[str, Any]] = self._build_messages(
            history=history,
            current_messages="[token-probe]",
            channel=channel,
            chat_id=chat_id,
        )
        return estimate_prompt_tokens_chain(
            self._provider,
            self._model,
            probe_messages,
            self._get_tool_definitions(),
        )

    async def archive_unconsolidated(self, session: Session) -> bool:
        """/new 스타일 세션 롤오버를 위해 아직 통합되지 않은 전체 tail(마지막 구간)을 아카이브한다."""
        lock: asyncio.Lock = self.get_lock(session.key)
        async with lock:
            snapshot: list[dict[str, Any]] = session.messages[session.last_consolidated :]
            if not snapshot:
                return True

            return await self.consolidate_messages(snapshot)

    async def maybe_consolidate_by_tokens(self, session: Session) -> bool:
        """루프: 프롬프트가 컨텍스트 윈도우의 절반 이내에 들어올 때까지 오래된 메시지를 아카이브한다.

        통합이 실제로 수행되고 성공하면 True를 반환한다.
        """
        if not session.messages or self._context_window_tokens <= 0:
            return False

        lock: asyncio.Lock = self.get_lock(session.key)
        async with lock:
            estimated, source = self.estimate_session_prompt_tokens(session)
            if estimated <= 0:
                return False
            if estimated < self._context_window_tokens:
                logger.debug(
                    "토큰 통합 대기 {}: {}/{} (경로: {})",
                    session.key,
                    estimated,
                    self._context_window_tokens,
                    source,
                )
                return False

            consolidated: bool = False
            target: int = self._context_window_tokens // 2
            for round_num in range(self._MAX_CONSOLIDATION_ROUNDS):
                if estimated <= target:
                    return consolidated

                boundary: tuple[int, int] = self.pick_consolidation_boundary(
                    session, max(1, estimated - target)
                )
                if boundary is None:
                    logger.debug(
                        "Token consolidation: no safe boundary for {} (round {})",
                        session.key,
                        round_num,
                    )
                    return consolidated

                end_idx: int = boundary[0]
                chunk: list[dict[str, Any]] = session.messages[session.last_consolidated : end_idx]
                if not chunk:
                    return consolidated

                logger.info(
                    "토큰 통합 라운드 {} - {}: {}/{} (경로: {}), 청크={}개 메시지",
                    round_num,
                    session.key,
                    estimated,
                    self._context_window_tokens,
                    source,
                    len(chunk),
                )

                if not await self.consolidate_messages(chunk):
                    return consolidated

                consolidated = True
                session.last_consolidated = end_idx
                self._sessions.save(session)

                estimated, source = self.estimate_session_prompt_tokens(session)
                if estimated <= 0:
                    return consolidated

            return consolidated

        return False
