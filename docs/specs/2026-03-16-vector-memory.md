# SPEC: Vector Memory (LanceDB)

> **Prompt**: HKUDS/nanobot, OpenClaw 분석 후 shacs-bot에 추가할 기능 — Vector Memory

## PRDs

| PRD | 설명 |
|---|---|
| [`docs/prds/vector-memory.md`](../prds/vector-memory.md) | LanceDB + sentence-transformers + search_history 하이브리드 모드 |

## TL;DR

> **목적**: 현재 텍스트 기반 MEMORY.md + HISTORY.md grep 검색을 **벡터 기반 시맨틱 검색**으로 보강하여, "지난주에 얘기한 그 프로젝트" 같은 의미 기반 질의를 지원한다.
>
> **Deliverables**:
> - `memory/vector.py` — LanceDB 벡터 메모리 구현
> - `agent/memory.py` — 기존 MemoryStore에 벡터 검색 통합
> - `agent/tools/history.py` — `history_search` 도구 시맨틱 모드 추가
> - `config/schema.py` — `VectorMemoryConfig` 추가
>
> **Estimated Effort**: Medium (4-6시간)

## 현재 상태 분석

### 현재 메모리 아키텍처 (`agent/memory.py`)

```
MemoryStore (workspace/memory/)
  ├─ MEMORY.md — 장기 사실 (LLM이 요약/업데이트)
  └─ HISTORY.md — 시간순 이벤트 로그 (append-only)

MemoryConsolidator
  └─ 토큰 초과 시 오래된 메시지 → MEMORY.md + HISTORY.md로 압축
```

### 한계

1. **HISTORY.md 검색은 grep**: 키워드 매칭만 가능. "비슷한 주제" 검색 불가
2. **MEMORY.md는 전체 로드**: system prompt에 통째로 주입. 메모리가 커지면 토큰 낭비
3. **시맨틱 유사도 없음**: "저번에 말한 카페 추천" → grep으로는 "카페"만 매칭, 문맥 불일치 가능

## 설계

### 아키텍처: 하이브리드 메모리

기존 MEMORY.md/HISTORY.md를 **그대로 유지**하면서, 벡터 인덱스를 **보조 검색 레이어**로 추가한다.

```
MemoryStore
  ├─ MEMORY.md           (기존, 변경 없음)
  ├─ HISTORY.md          (기존, 변경 없음)
  └─ vector/             (신규)
      └─ lancedb/        LanceDB 테이블
          └─ memories    벡터 인덱스
```

### 임베딩 전략

| 방식 | 장점 | 단점 |
|------|------|------|
| OpenAI `text-embedding-3-small` | 높은 품질 | API 비용, 네트워크 필요 |
| **`sentence-transformers` 로컬** | 무료, 오프라인 | 초기 모델 다운로드 (~100MB) |
| LLM provider 재사용 | 설정 불필요 | 비용 높음, 오버킬 |

**선택: `sentence-transformers` 로컬** (기본값) + OpenAI embedding (선택적)

이유: 봇은 24/7 게이트웨이로 동작하므로 모델을 메모리에 캐시할 수 있고, 매 메모리 저장마다 API 비용이 발생하는 것은 비효율적.

### 의존성

```toml
# pyproject.toml [project.optional-dependencies]
vector-memory = [
    "lancedb>=0.4.0",
    "sentence-transformers>=2.2.0",
]
```

optional dependency. 미설치 시 기존 grep 검색으로 fallback.

### 변경 사항

#### 1. Config 추가 (`config/schema.py`)

```python
class VectorMemoryConfig(Base):
    """벡터 메모리 설정."""
    enabled: bool = False
    embedding_model: str = "all-MiniLM-L6-v2"  # sentence-transformers 모델명
    embedding_provider: str = "local"            # "local" or "openai"
    top_k: int = 5                               # 검색 결과 수
    min_score: float = 0.5                       # 최소 유사도 점수 (0.0 ~ 1.0)
```

#### 2. VectorMemory (`memory/vector.py`)

```python
"""LanceDB 기반 벡터 메모리 — optional dependency."""

from pathlib import Path
from typing import Any

try:
    import lancedb
    from sentence_transformers import SentenceTransformer
    _HAS_VECTOR = True
except ImportError:
    _HAS_VECTOR = False

from loguru import logger


class VectorMemory:
    """시맨틱 검색을 위한 벡터 메모리 레이어."""

    def __init__(self, workspace: Path, config):
        self._db_path = workspace / "memory" / "vector"
        self._config = config
        self._db = None
        self._table = None
        self._model = None

    async def initialize(self) -> bool:
        """벡터 DB와 임베딩 모델을 초기화한다."""
        if not _HAS_VECTOR:
            logger.debug("Vector memory dependencies not installed, skipping")
            return False

        self._db = lancedb.connect(str(self._db_path))
        self._model = SentenceTransformer(self._config.embedding_model)

        if "memories" in self._db.table_names():
            self._table = self._db.open_table("memories")
        else:
            self._table = self._db.create_table("memories", schema={
                "text": str,
                "vector": list,  # embedding dimension
                "timestamp": str,
                "source": str,   # "consolidation" | "history" | "fact"
                "session_key": str,
            })

        return True

    def add(self, text: str, timestamp: str, source: str = "consolidation", session_key: str = "") -> None:
        """텍스트를 임베딩하여 벡터 DB에 추가."""
        if not self._table or not self._model:
            return

        vector = self._model.encode(text).tolist()
        self._table.add([{
            "text": text,
            "vector": vector,
            "timestamp": timestamp,
            "source": source,
            "session_key": session_key,
        }])

    def search(self, query: str, top_k: int | None = None, min_score: float | None = None) -> list[dict[str, Any]]:
        """시맨틱 유사도로 메모리를 검색."""
        if not self._table or not self._model:
            return []

        k = top_k or self._config.top_k
        threshold = min_score or self._config.min_score
        query_vector = self._model.encode(query).tolist()

        results = (
            self._table
            .search(query_vector)
            .limit(k)
            .to_list()
        )

        return [
            {"text": r["text"], "timestamp": r["timestamp"], "score": 1 - r["_distance"], "source": r["source"]}
            for r in results
            if (1 - r["_distance"]) >= threshold
        ]
```

#### 3. MemoryStore 통합 (`agent/memory.py`)

```python
class MemoryStore:
    def __init__(self, workspace: Path, vector_config=None):
        # ... 기존 초기화 ...
        self._vector: VectorMemory | None = None
        if vector_config and vector_config.enabled:
            self._vector = VectorMemory(workspace, vector_config)

    async def init_vector(self) -> None:
        """벡터 메모리 비동기 초기화."""
        if self._vector:
            await self._vector.initialize()

    def append_history(self, entry: str) -> None:
        # 기존 파일 append
        with open(self._history_file, "a", encoding="utf-8") as f:
            f.write(entry.rstrip() + "\n\n")
        # 벡터에도 추가
        if self._vector:
            self._vector.add(text=entry, timestamp=self._extract_timestamp(entry), source="history")

    def semantic_search(self, query: str) -> list[dict]:
        """시맨틱 검색. 벡터 미설정 시 빈 리스트 반환."""
        if not self._vector:
            return []
        return self._vector.search(query)
```

#### 4. `history_search` 도구 확장 (`agent/tools/history.py`)

기존 grep + 신규 시맨틱 검색을 병합하여 반환:

```python
async def execute(self, query: str, mode: str = "hybrid") -> str:
    results = []

    if mode in ("grep", "hybrid"):
        grep_results = self._grep_history(query)
        results.extend(grep_results)

    if mode in ("semantic", "hybrid") and self._memory_store:
        semantic_results = self._memory_store.semantic_search(query)
        results.extend(semantic_results)

    # 중복 제거 후 점수 기반 정렬
    return self._format_results(self._deduplicate(results))
```

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `memory/__init__.py` | 신규 | 패키지 초기화 |
| `memory/vector.py` | 신규 | VectorMemory 구현 |
| `agent/memory.py` | 수정 | MemoryStore에 벡터 레이어 통합 |
| `agent/tools/history.py` | 수정 | `history_search`에 semantic/hybrid 모드 추가 |
| `config/schema.py` | 수정 | `VectorMemoryConfig` 추가, `Config`에 `vector_memory` 필드 |
| `pyproject.toml` | 수정 | `[project.optional-dependencies]`에 `vector-memory` 그룹 |

## 검증 기준

- [ ] `uv sync` (vector-memory 없이) 시 기존 동작 무변경 확인
- [ ] `uv sync --extra vector-memory` 후 벡터 메모리 활성화 확인
- [ ] 대화 5턴 후 "아까 말한 거" 같은 시맨틱 질의로 관련 히스토리 검색 성공
- [ ] grep 검색과 시맨틱 검색 결과 병합 시 중복 제거 확인
- [ ] 벡터 DB 파일이 `workspace/memory/vector/`에 정상 생성 확인
