from __future__ import annotations

from pathlib import Path
from typing import cast

import pytest

from shacs_bot.agent.memory import MemoryStore
from shacs_bot.agent.tools.base import Tool
from shacs_bot.agent.tools.history import SearchHistoryTool
from shacs_bot.agent.tools.registry import create_default_tools
from shacs_bot.config.schema import Config
from shacs_bot.config.schema import VectorMemoryConfig


class _FakeVectorMemory:
    def __init__(self, workspace: Path, embedding_model: str = "all-MiniLM-L6-v2") -> None:
        self._db_path: Path = workspace / "memory" / "vector"
        self._embedding_model: str = embedding_model
        self._rows: list[dict[str, str | float]] = []

    def initialize(self) -> bool:
        self._db_path.mkdir(parents=True, exist_ok=True)
        return True

    def add(self, text: str, timestamp: str = "", source: str = "history") -> None:
        self._rows.append(
            {
                "text": text,
                "timestamp": timestamp,
                "source": source,
            }
        )

    def search(
        self, query: str, top_k: int = 5, min_score: float = 0.5
    ) -> list[dict[str, str | float]]:
        del min_score

        results: list[dict[str, str | float]] = []
        for row in self._rows:
            text = cast(str, row["text"])
            if query in text:
                results.append(
                    {
                        "text": text,
                        "timestamp": cast(str, row["timestamp"]),
                        "score": 0.93,
                        "source": cast(str, row["source"]),
                    }
                )

        if query == "카페 추천":
            for row in self._rows:
                text = cast(str, row["text"])
                if "커피숍 추천" in text:
                    results.append(
                        {
                            "text": text,
                            "timestamp": cast(str, row["timestamp"]),
                            "score": 0.91,
                            "source": cast(str, row["source"]),
                        }
                    )
            results.append(
                {
                    "text": "[2026-04-05 10:05] 디저트 맛집도 함께 정리했다.",
                    "timestamp": "2026-04-05 10:05",
                    "score": 0.82,
                    "source": "history",
                }
            )

        return results[:top_k]


def test_vector_memory_config_accepts_and_dumps_camel_case_keys() -> None:
    config = Config.model_validate(
        {
            "tools": {
                "vectorMemory": {
                    "enabled": True,
                    "embeddingModel": "demo-model",
                    "embeddingProvider": "local",
                    "topK": 7,
                    "minScore": 0.75,
                }
            }
        }
    )

    assert config.tools.vector_memory.enabled is True
    assert config.tools.vector_memory.embedding_model == "demo-model"
    assert config.tools.vector_memory.embedding_provider == "local"
    assert config.tools.vector_memory.top_k == 7
    assert config.tools.vector_memory.min_score == 0.75

    payload = config.tools.model_dump(by_alias=True)
    assert payload["vectorMemory"]["embeddingModel"] == "demo-model"
    assert payload["vectorMemory"]["topK"] == 7
    assert payload["vectorMemory"]["minScore"] == 0.75


@pytest.mark.asyncio
async def test_search_history_hybrid_falls_back_to_grep_when_vector_dependency_is_unavailable(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from shacs_bot.memory import vector as vector_module

    monkeypatch.setattr(vector_module, "is_available", lambda: False)

    store = MemoryStore(tmp_path, vector_config=VectorMemoryConfig(enabled=True))
    store.append_history("[2026-04-05 10:00] 커피숍 추천을 정리했다.")
    tool = SearchHistoryTool(workspace=tmp_path, memory_store=store)

    result = await tool.execute(query="커피숍", mode="hybrid")

    assert "커피숍 추천을 정리했다." in result
    assert "semantic 0개" in result
    assert store.semantic_search("카페 추천") == []
    assert not (tmp_path / "memory" / "vector").exists()


@pytest.mark.asyncio
async def test_search_history_semantic_mode_uses_vector_store_and_creates_vector_directory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from shacs_bot.memory import vector as vector_module

    monkeypatch.setattr(vector_module, "is_available", lambda: True)
    monkeypatch.setattr(vector_module, "VectorMemory", _FakeVectorMemory)

    store = MemoryStore(tmp_path, vector_config=VectorMemoryConfig(enabled=True))
    store.append_history("[2026-04-05 10:00] 커피숍 추천을 정리했다.")
    tool = SearchHistoryTool(workspace=tmp_path, memory_store=store)

    result = await tool.execute(query="카페 추천", mode="semantic")

    assert "커피숍 추천을 정리했다." in result
    assert "semantic 2개" in result
    assert (tmp_path / "memory" / "vector").is_dir()


@pytest.mark.asyncio
async def test_search_history_hybrid_deduplicates_grep_and_semantic_results(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from shacs_bot.memory import vector as vector_module

    monkeypatch.setattr(vector_module, "is_available", lambda: True)
    monkeypatch.setattr(vector_module, "VectorMemory", _FakeVectorMemory)

    store = MemoryStore(tmp_path, vector_config=VectorMemoryConfig(enabled=True))
    store.append_history("[2026-04-05 10:00] 카페 추천 목록을 만들었다.")
    store.append_history("[2026-04-05 10:02] 커피숍 추천을 정리했다.")
    tool = SearchHistoryTool(workspace=tmp_path, memory_store=store)

    result = await tool.execute(query="카페 추천", mode="hybrid")

    assert result.count("[2026-04-05 10:00] 카페 추천 목록을 만들었다.") == 1
    assert "디저트 맛집도 함께 정리했다." in result


def test_create_default_tools_wires_search_history_with_vector_memory_config(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from shacs_bot.memory import vector as vector_module

    monkeypatch.setattr(vector_module, "is_available", lambda: True)
    monkeypatch.setattr(vector_module, "VectorMemory", _FakeVectorMemory)

    tools = create_default_tools(
        workspace=tmp_path,
        vector_memory_config=VectorMemoryConfig(enabled=True),
    )

    search_tool = next(tool for tool in tools if tool.name == "search_history")

    assert isinstance(search_tool, SearchHistoryTool)
    assert (tmp_path / "memory" / "vector").is_dir()
    assert isinstance(search_tool, Tool)
