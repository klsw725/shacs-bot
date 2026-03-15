"""HISTORY.md 검색 도구"""

from pathlib import Path
from typing import Any


from shacs_bot.agent.tools.base import Tool


class SearchHistoryTool(Tool):
    name = "search_history"
    description = "과거 대화 히스토리를 키워드로 검색합니다. 사용자가 이전 대화 내용을 회상하거나 과거에 논의한 주제를 찾을 때 사용하세요."
    parameters = {
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "검색할 키워드 또는 문구",
            },
            "max_results": {
                "type": "integer",
                "description": "최대 반환 엔트리 수 (기본: 10)",
            },
        },
        "required": ["query"],
    }

    def __init__(self, workspace: Path):
        self._history_file: Path = workspace / "memory" / "HISTORY.md"

    async def execute(self, query: str, max_results: int = 10, **kwargs: Any) -> str:
        if not self._history_file.exists():
            return "히스토리가 아직 없습니다."

        text: str = self._history_file.read_text(encoding="utf-8")
        if not text.strip():
            return "히스토리가 비어 있습니다."

        entries: list[str] = [e.strip() for e in text.split("\n\n") if e.strip()]
        query_lower: str = query.lower()
        matched: list[str] = [e for e in entries if query_lower in e.lower()]

        if not matched:
            return f'"{query}"에 대한 검색 결과가 없습니다.'

        recent: list[str] = matched[-max_results:]
        recent.reverse()

        header: str = f'[검색 결과: "{query}" — {len(matched)}개 매칭, 최신 {len(recent)}개 표시]\n'
        return header + "\n\n".join(recent)
