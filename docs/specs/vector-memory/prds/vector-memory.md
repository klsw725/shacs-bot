# PRD: Vector Memory (LanceDB)

> **Spec**: [`docs/specs/2026-03-16-vector-memory.md`](../spec.md)

---

## 문제

현재 `search_history` 도구 (`agent/tools/history.py`)는 HISTORY.md를 **키워드 grep**으로 검색한다:

```python
# history.py:40-41
query_lower = query.lower()
matched = [e for e in entries if query_lower in e.lower()]
```

**한계**:
- "저번에 말한 프로젝트" → `query_lower="저번에 말한 프로젝트"` → 해당 문자열이 정확히 포함된 항목만 반환
- 동의어, 유사 표현, 맥락 기반 검색 불가
- HISTORY.md가 커지면 전체 파일을 매번 읽고 순회 — O(n) 선형 검색

## 해결책

LanceDB + sentence-transformers를 **optional dependency**로 추가하여, 기존 grep 검색과 벡터 시맨틱 검색을 **하이브리드**로 병합한다. 기존 MEMORY.md / HISTORY.md 파일 시스템은 그대로 유지 — 벡터 인덱스는 보조 레이어.

## 사용자 영향

| Before | After |
|---|---|
| 키워드 일치만 검색 | 의미 기반 유사도 검색 추가 |
| "카페" 검색 시 "커피숍" 미매칭 | 시맨틱 유사도로 관련 항목 반환 |
| 검색 모드 없음 | `mode` 파라미터 — `grep` / `semantic` / `hybrid` |
| 설치 필수 의존성 증가 없음 | optional — `uv sync --extra vector-memory`로 선택 설치 |

## 기술적 범위

- **변경 파일**: 4개 수정 + 2개 신규
- **변경 유형**: optional 모듈 추가 + 기존 메서드/도구 확장
- **의존성**: `lancedb`, `sentence-transformers` (optional)
- **하위 호환성**: optional dep 미설치 시 기존 grep 동작 100% 유지

### 변경 1: 의존성 추가 (`pyproject.toml`)

```toml
[project.optional-dependencies]
vector-memory = [
    "lancedb>=0.4.0",
    "sentence-transformers>=2.2.0",
]
```

### 변경 2: Config 추가 (`config/schema.py`)

`ToolsConfig` 위 (line 325 부근)에 추가:

```python
class VectorMemoryConfig(Base):
    """벡터 메모리 설정."""
    enabled: bool = False
    embedding_model: str = "all-MiniLM-L6-v2"
    embedding_provider: str = "local"       # "local" | "openai"
    top_k: int = 5
    min_score: float = 0.5
```

`Config` 클래스에 필드 추가:

```python
vector_memory: VectorMemoryConfig = Field(default_factory=VectorMemoryConfig)
```

### 변경 3: VectorMemory 모듈 (`shacs_bot/memory/__init__.py`, `shacs_bot/memory/vector.py` 신규)

**`__init__.py`**: 빈 파일

**`vector.py`**:

```python
"""LanceDB 기반 벡터 메모리 — optional dependency."""

from pathlib import Path
from typing import Any

try:
    import lancedb
    import pyarrow as pa
    from sentence_transformers import SentenceTransformer
    _HAS_VECTOR = True
except ImportError:
    _HAS_VECTOR = False

from loguru import logger


def is_available() -> bool:
    return _HAS_VECTOR


class VectorMemory:
    """시맨틱 검색을 위한 벡터 메모리 레이어."""

    def __init__(self, workspace: Path, embedding_model: str = "all-MiniLM-L6-v2"):
        self._db_path: Path = workspace / "memory" / "vector"
        self._embedding_model_name: str = embedding_model
        self._db = None
        self._table = None
        self._model = None
        self._dim: int = 0

    def initialize(self) -> bool:
        """벡터 DB와 임베딩 모델을 초기화한다. 동기 호출."""
        if not _HAS_VECTOR:
            return False

        self._db_path.mkdir(parents=True, exist_ok=True)
        self._db = lancedb.connect(str(self._db_path))
        self._model = SentenceTransformer(self._embedding_model_name)
        self._dim = self._model.get_sentence_embedding_dimension()

        schema = pa.schema([
            pa.field("text", pa.utf8()),
            pa.field("vector", pa.list_(pa.float32(), self._dim)),
            pa.field("timestamp", pa.utf8()),
            pa.field("source", pa.utf8()),
        ])

        if "memories" in self._db.table_names():
            self._table = self._db.open_table("memories")
        else:
            self._table = self._db.create_table("memories", schema=schema)

        logger.info("Vector memory 초기화 완료 (model={}, dim={})", self._embedding_model_name, self._dim)
        return True

    def add(self, text: str, timestamp: str = "", source: str = "history") -> None:
        """텍스트를 임베딩하여 벡터 DB에 추가."""
        if not self._table or not self._model:
            return

        vector: list[float] = self._model.encode(text).tolist()
        self._table.add([{
            "text": text,
            "vector": vector,
            "timestamp": timestamp,
            "source": source,
        }])

    def search(self, query: str, top_k: int = 5, min_score: float = 0.5) -> list[dict[str, Any]]:
        """시맨틱 유사도로 메모리를 검색."""
        if not self._table or not self._model:
            return []

        query_vector: list[float] = self._model.encode(query).tolist()
        results = self._table.search(query_vector).limit(top_k).to_list()

        return [
            {
                "text": r["text"],
                "timestamp": r.get("timestamp", ""),
                "score": round(1 - r["_distance"], 3),
                "source": r.get("source", ""),
            }
            for r in results
            if (1 - r["_distance"]) >= min_score
        ]
```

### 변경 4: MemoryStore 통합 (`agent/memory.py`)

**memory.py** (line 44, `MemoryStore.__init__`):

```python
from shacs_bot.memory.vector import VectorMemory, is_available as vector_available

class MemoryStore:
    def __init__(self, workspace: Path, vector_config=None):
        self._memory_dir = ensure_dir(workspace / "memory")
        self._memory_file = self._memory_dir / "MEMORY.md"
        self._history_file = self._memory_dir / "HISTORY.md"

        self._vector: VectorMemory | None = None
        if vector_config and vector_config.enabled and vector_available():
            self._vector = VectorMemory(workspace, vector_config.embedding_model)
            self._vector.initialize()
```

**memory.py** (line 58, `append_history`):

```python
def append_history(self, entry: str) -> None:
    with open(self._history_file, "a", encoding="utf-8") as f:
        f.write(entry.rstrip() + "\n\n")
    # 벡터 인덱스에도 추가
    if self._vector:
        ts = entry[:17] if len(entry) > 17 and entry[0] == "[" else ""
        self._vector.add(text=entry, timestamp=ts, source="history")

def semantic_search(self, query: str, top_k: int = 5, min_score: float = 0.5) -> list[dict]:
    """시맨틱 검색. 벡터 미설정 시 빈 리스트."""
    if not self._vector:
        return []
    return self._vector.search(query, top_k=top_k, min_score=min_score)
```

### 변경 5: `search_history` 도구 확장 (`agent/tools/history.py`)

**history.py** — 전체 교체:

```python
"""HISTORY.md 검색 도구 — grep + 시맨틱 하이브리드"""

from pathlib import Path
from typing import Any

from shacs_bot.agent.tools.base import Tool


class SearchHistoryTool(Tool):
    name = "search_history"
    description = "과거 대화 히스토리를 검색합니다. 키워드 검색(grep), 의미 기반 검색(semantic), 또는 둘 다(hybrid) 모드를 지원합니다."
    parameters = {
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
                "description": "검색 모드. grep=키워드 매칭, semantic=의미 유사도, hybrid=둘 다 (기본: hybrid)",
            },
        },
        "required": ["query"],
    }

    def __init__(self, workspace: Path, memory_store=None):
        self._history_file: Path = workspace / "memory" / "HISTORY.md"
        self._memory_store = memory_store

    async def execute(self, query: str, max_results: int = 10, mode: str = "hybrid", **kwargs: Any) -> str:
        if not self._history_file.exists():
            return "히스토리가 아직 없습니다."

        results: list[str] = []

        # grep 검색
        if mode in ("grep", "hybrid"):
            text = self._history_file.read_text(encoding="utf-8")
            if text.strip():
                entries = [e.strip() for e in text.split("\n\n") if e.strip()]
                query_lower = query.lower()
                matched = [e for e in entries if query_lower in e.lower()]
                recent = matched[-max_results:]
                recent.reverse()
                results.extend(recent)

        # 시맨틱 검색
        if mode in ("semantic", "hybrid") and self._memory_store:
            semantic = self._memory_store.semantic_search(query, top_k=max_results)
            for item in semantic:
                text = item["text"]
                if text not in results:  # 중복 제거
                    results.append(f"[유사도: {item['score']}] {text}")

        if not results:
            return f'"{query}"에 대한 검색 결과가 없습니다.'

        header = f'[검색 결과: "{query}" — {len(results)}개, 모드: {mode}]\n'
        return header + "\n\n".join(results[:max_results])
```

### 변경 6: 도구 등록 (`agent/loop.py`)

**loop.py** (line 153, `SearchHistoryTool` 등록 부분):

```python
# Before
self._tools.register(SearchHistoryTool(workspace=self._workspace))

# After
self._tools.register(SearchHistoryTool(
    workspace=self._workspace,
    memory_store=self._memory_consolidator._store if hasattr(self._memory_consolidator, '_store') else None,
))
```

**loop.py** (line 107, `MemoryConsolidator` 생성 부근):

`MemoryStore` 생성 시 `vector_config` 전달을 위해 `MemoryConsolidator` 초기화 흐름에서 config를 받아야 한다. `AgentLoop.__init__`에 `vector_memory_config` 파라미터 추가하거나, config 객체를 통째로 전달.

## 성공 기준

1. `uv sync` (vector-memory 없이) — 기존 grep 검색 동작 100% 유지, `ImportError` 없음
2. `uv sync --extra vector-memory` + `vector_memory.enabled=true` — 벡터 초기화 로그 출력
3. "아까 말한 카페" 검색 시 "커피숍 추천" 항목이 시맨틱 결과로 반환
4. `mode=grep` — 기존과 동일한 키워드 매칭만 수행
5. `mode=hybrid` — grep + semantic 결과 병합, 중복 제거
6. 벡터 DB 파일이 `workspace/memory/vector/`에 생성

---

## 마일스톤

- [x] **M1: VectorMemory 모듈 구현**
  `memory/vector.py` — LanceDB 연결, sentence-transformers 임베딩, add/search API. `pyproject.toml` optional dep.

- [x] **M2: MemoryStore 통합**
  `agent/memory.py` — `MemoryStore.__init__`에 벡터 초기화, `append_history`에 벡터 추가, `semantic_search` 메서드.

- [x] **M3: search_history 도구 확장**
  `agent/tools/history.py` — `mode` 파라미터 추가, grep/semantic/hybrid 분기, 중복 제거. `loop.py`에서 `memory_store` 전달.

- [x] **M4: Config + 검증**
  `VectorMemoryConfig` 스키마 추가. 멀티턴 대화 후 시맨틱 검색 동작 확인. optional dep 미설치 시 fallback 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| sentence-transformers 초기 로드 시간 (~5초) | 확실 | 낮음 | 게이트웨이 시작 시 1회만 로드. 이후 메모리 캐시 |
| 모델 다운로드 필요 (~100MB) | 확실 | 낮음 | 첫 실행 시 자동 다운로드. 문서에 안내 |
| LanceDB 파일 손상 | 낮음 | 중간 | HISTORY.md는 그대로 유지 — 벡터 인덱스만 재생성하면 복구 |
| 기존 HISTORY.md 항목은 벡터에 없음 | 확실 | 낮음 | 마이그레이션 스크립트 또는 첫 초기화 시 기존 HISTORY.md 일괄 인덱싱 고려 |
| `memory_store` 전달 구조가 복잡해짐 | 중간 | 낮음 | `MemoryConsolidator._store`를 통해 접근. 추후 DI 정리 가능 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-04-05 | M4 검증 테스트 추가 및 fallback/semantic/hybrid smoke 확인 |
