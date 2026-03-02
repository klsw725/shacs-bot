"""영구 agent 메모리를 위한 메모리 시스템"""
import json
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.agent.session.manager import Session
from shacs_bot.providers.base import LLMProvider, LLMResponse
from shacs_bot.utils import ensure_dir


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
                            "description": "주요 사건/결정/주제를 요약한 하나의 문단(2~5문장). [YYYY-MM-DD HH:MM] 형식으로 시작하세요. grep 검색에 유용한 세부 정보를 포함하세요."
                        },
                        "memory_update": {
                            "type": "string",
                            "description": "마크다운 형식의 전체 업데이트된 장기 메모리입니다. 기존의 모든 사실에 새로운 사실을 추가해 포함하세요. 새로 추가할 내용이 없다면 변경 없이 그대로 반환하세요.",
                        }
                    },
                    "required": ["history_entry", "memory_update"]
                }
            }
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
            session: Session,
            provider: LLMProvider,
            model: str,
            *,
            archive_all: bool = False,
            memory_window: int = 50,
    ) -> bool:
        """
        LLM 툴 호출을 통해 오래된 메시지를 MEMORY.md와 HISTORY.md로 통합합니다.

        성공 시(아무 작업도 하지 않은 경우 포함) True를 반환하고, 실패 시 False를 반환합니다.
        """
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

            old_messages: list[dict[str, Any]] = session.messages[session.last_consolidated:-keep_count]

            if not old_messages:
                return True

            logger.info("메모리 통합: {}개 통합 대상, {}개 유지", len(old_messages), keep_count)

        lines: list[dict[str, Any]] = []
        for m in old_messages:
            if not m.get("content"):
                continue

            tools: str = f" [tools: {', '.join(m['tools_used'])}" if m.get("tools_used") else ""
            lines.append(f"[{m.get('timestamp', '?')[:16]}] {m['role'].upper()}{tools}: {m['content']}")

        current_memory: str = self.read_long_term()
        prompt: str = f"""이 대화를 처리하고, 통합 결과를 담아 save_memory 도구를 호출하세요.

            ## 현재 장기 기억(Long-term Memory)
            {current_memory or "(비어 있음)"}
            
            ## 처리할 대화
            {chr(10).join(lines)}"""
        try:
            response: LLMResponse = await provider.chat(
                messages=[
                    {
                        "role": "system",
                        "content":"You are a memory consolidation agent. Call the save_memory tool with your consolidation of the conversation.",
                    },
                    {
                        "role": "user",
                        "content": prompt
                    }
                ],
                tools=_SAVE_MEMORY_TOOL,
                model=model,
            )
            if not response.has_tool_calls:
                logger.warning("메모리 통합: LLM이 save_memory를 호출하지 않아 건너뜁니다.")
                return False

            args: dict[str, Any] = response.tool_calls[0].arguments
            # 몇몇 제공자들은 인자를 dict 대신 JSON 문자열로 제공합니다.
            if isinstance(args, str):
                args = json.loads(args)
            if not isinstance(args, dict):
                logger.warning("메모리 통합: 예상치 않은 인자 타입 {}", type(args).__name__)
                return False

            entry: Any = args.get("history_entry")
            if entry:
                if not isinstance(entry, str):
                    entry = json.dumps(entry, ensure_ascii=False)

                self.append_history(entry)

            update: Any = args.get("memory_update")
            if update:
                if not isinstance(update, str):
                    update = json.dumps(update, ensure_ascii=False)

                if update != current_memory:
                    self.write_long_term(update)

            session.last_consolidated = 0 if archive_all else (len(session.messages) - keep_count)
            logger.info(f"메모리 통합 완료: 총 {len(session.messages)}개의 메시지, 마지막 통합 위치(last_consolidated)={session.last_consolidated}")
            return True
        except Exception:
            logger.exception("메모리 통합 실패")
            return False
























