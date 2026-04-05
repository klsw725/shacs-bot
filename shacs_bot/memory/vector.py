from pathlib import Path
from importlib import import_module
from typing import Any, cast

from loguru import logger

try:
    _lancedb_module = import_module("lancedb")
    _sentence_transformers_module = import_module("sentence_transformers")
    _has_vector: bool = True
except ImportError:
    _lancedb_module = None
    _sentence_transformers_module = None
    _has_vector = False


def is_available() -> bool:
    return _has_vector


class VectorMemory:
    def __init__(self, workspace: Path, embedding_model: str = "all-MiniLM-L6-v2"):
        self._db_path: Path = workspace / "memory" / "vector"
        self._embedding_model_name: str = embedding_model
        self._db: object | None = None
        self._table: object | None = None
        self._model: object | None = None
        self._dim: int = 0

    def initialize(self) -> bool:
        if not _has_vector or _lancedb_module is None or _sentence_transformers_module is None:
            return False

        lancedb_module = cast(Any, _lancedb_module)
        sentence_transformers_module = cast(Any, _sentence_transformers_module)
        self._db_path.mkdir(parents=True, exist_ok=True)
        db = lancedb_module.connect(str(self._db_path))
        model = sentence_transformers_module.SentenceTransformer(self._embedding_model_name)
        dim: int | None = model.get_sentence_embedding_dimension()
        self._db = db
        self._model = model
        self._dim = dim or 0
        self._table = db.create_table("memories", data=[], exist_ok=True)
        logger.info(
            "Vector memory initialized (model={}, dim={})",
            self._embedding_model_name,
            self._dim,
        )
        return True

    def add(self, text: str, timestamp: str = "", source: str = "history") -> None:
        if not self._table or not self._model:
            return

        model = cast(Any, self._model)
        table = cast(Any, self._table)
        vector = model.encode([text], convert_to_numpy=True)[0].tolist()
        table.add([{"text": text, "vector": vector, "timestamp": timestamp, "source": source}])

    def search(self, query: str, top_k: int = 5, min_score: float = 0.5) -> list[dict[str, Any]]:
        if not self._table or not self._model:
            return []

        model = cast(Any, self._model)
        table = cast(Any, self._table)
        query_vector = model.encode([query], convert_to_numpy=True)[0].tolist()
        results = cast(list[dict[str, object]], table.search(query_vector).limit(top_k).to_list())
        matches: list[dict[str, Any]] = []
        for result in results:
            distance: object = result.get("_distance", 1.0)
            score: float = (
                round(1 - float(distance), 3) if isinstance(distance, int | float) else 0.0
            )
            if score < min_score:
                continue
            matches.append(
                {
                    "text": str(result.get("text", "")),
                    "timestamp": str(result.get("timestamp", "")),
                    "score": score,
                    "source": str(result.get("source", "")),
                }
            )
        return matches
