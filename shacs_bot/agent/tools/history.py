"""HISTORY.md 검색 도구"""

from pathlib import Path
from typing import TYPE_CHECKING, Any


from shacs_bot.agent.tools.base import Tool

if TYPE_CHECKING:
    from shacs_bot.agent.memory import MemoryStore


class SearchHistoryTool(Tool):
    def __init__(self, workspace: Path, memory_store: "MemoryStore | None" = None):
        self._history_file: Path = workspace / "memory" / "HISTORY.md"
        self._memory_store: MemoryStore | None = memory_store

    @property
    def name(self) -> str:
        return "search_history"

    @property
    def description(self) -> str:
        return "과거 대화 히스토리를 검색합니다. 키워드 검색(grep), 의미 기반 검색(semantic), 또는 둘 다(hybrid) 모드를 지원합니다."

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "검색할 키워드 또는 자연어 질의",
                },
                "max_results": {
                    "type": "integer",
                    "description": "최대 반환 엔트리 수 (기본: 10)",
                },
                "mode": {
                    "type": "string",
                    "enum": ["grep", "semantic", "hybrid"],
                    "description": "검색 모드. grep=키워드, semantic=의미 기반, hybrid=둘 다 (기본: hybrid)",
                },
            },
            "required": ["query"],
        }

    async def execute(self, **kwargs: Any) -> str:
        query: str = str(kwargs.get("query", ""))
        max_results: int = int(kwargs.get("max_results", 10))
        mode: str = str(kwargs.get("mode", "hybrid"))
        if not self._history_file.exists():
            return "히스토리가 아직 없습니다."

        text: str = self._history_file.read_text(encoding="utf-8")
        if not text.strip() and mode != "semantic":
            return "히스토리가 비어 있습니다."

        grep_count: int = 0
        semantic_count: int = 0
        results: list[str] = []
        seen: set[str] = set()

        if mode in ("grep", "hybrid") and text.strip():
            entries: list[str] = [e.strip() for e in text.split("\n\n") if e.strip()]
            query_lower: str = query.lower()
            matched: list[str] = [e for e in entries if query_lower in e.lower()]
            recent: list[str] = matched[-max_results:]
            recent.reverse()
            grep_count = len(matched)
            for item in recent:
                if item in seen:
                    continue
                seen.add(item)
                results.append(item)

        if mode in ("semantic", "hybrid") and self._memory_store is not None:
            semantic_matches = self._memory_store.semantic_search(query, top_k=max_results)
            semantic_count = len(semantic_matches)
            for match in semantic_matches:
                text_value: str = str(match.get("text", "")).strip()
                if not text_value or text_value in seen:
                    continue
                seen.add(text_value)
                score: object = match.get("score", 0.0)
                score_text: str = (
                    f" [semantic score={score}]" if isinstance(score, int | float) else ""
                )
                results.append(f"{text_value}{score_text}")

        if not results:
            return f'"{query}"에 대한 검색 결과가 없습니다.'

        limited_results: list[str] = results[:max_results]
        summary: str = f"grep {grep_count}개"
        if mode in ("semantic", "hybrid"):
            summary += f", semantic {semantic_count}개"
        header: str = (
            f'[검색 결과: "{query}" — mode={mode}, {summary}, 표시 {len(limited_results)}개]\n'
        )
        return header + "\n\n".join(limited_results)
