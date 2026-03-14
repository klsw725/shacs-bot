"""대화 기록을 위한 세션 관리"""

import json
import shutil
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.config.paths import get_legacy_sessions_dir
from shacs_bot.utils.helpers import ensure_dir, safe_filename


@dataclass
class Session:
    """
    대화 세션.

    메시지를 JSONL 형식으로 저장하여 읽기 쉽고 영구 보관이 가능하도록 합니다.

    중요: LLM 캐시 효율성을 위해 메시지는 append-only 방식으로 추가됩니다.
    통합(consolidation) 과정은 MEMORY.md/HISTORY.md에 요약을 작성하지만,
    messages 리스트나 get_history()의 출력은 수정하지 않습니다.
    """

    key: str
    messages: list[dict[str, Any]] = field(default_factory=list)
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)
    metadata: dict[str, Any] = field(default_factory=dict)
    last_consolidated: int = 0  # 파일로 이미 통합된 메시지 수

    def add_message(self, role: str, content: str, **kwargs: Any) -> None:
        """세션에 메시지 추가"""
        msg: dict[str, Any] = {
            "role": role,
            "content": content,
            "timestamp": datetime.now().isoformat(),
            **kwargs,
        }
        self.messages.append(msg)
        self.updated_at = datetime.now()

    def get_history(self, max_messages: int = 500) -> list[dict[str, Any]]:
        """LLM 입력용으로, 사용자 턴에 맞춰 정렬된 미통합 메시지를 반환합니다."""
        unconsolidated: list[dict[str, Any]] = self.messages[self.last_consolidated :]
        sliced: list[dict[str, Any]] = unconsolidated[-max_messages:]

        # 선행하는 user가 아닌 메시지를 제거하여 고아(orphaned) tool_result 블록이 생기지 않도록 합니다.
        for idx, m in enumerate(sliced):
            if m.get("role") == "user":
                sliced = sliced[idx:]
                break

        out: list[dict[str, Any]] = []

        for m in sliced:
            entry: dict[str, Any] = {
                "role": m["role"],
                "content": m.get("content", ""),
            }
            for k in ("tool_calls", "tool_call_id", "name"):
                if k in m:
                    entry[k] = m[k]

            out.append(entry)

        return out

    def clear(self) -> None:
        """모든 메시지를 삭제하고 세션을 초기 상태로 재설정합니다."""
        self.messages.clear()
        self.last_consolidated = 0
        self.updated_at = datetime.now()


class SessionManager:
    """
    대화 세션을 관리합니다.

    세션은 sessions 디렉터리에 JSONL 파일 형식으로 저장됩니다.
    """

    def __init__(self, workspace: Path):
        self._workspace: Path = workspace
        self._session_dir: Path = ensure_dir(self._workspace / "sessions")
        self._legacy_sessions_dir: Path = get_legacy_sessions_dir()
        self._cache: dict[str, Session] = {}

    def get_or_create(self, key: str) -> Session:
        """
        존재하는 세션을 가져오거나 새롭게 하나 생성합니다.

        Args:
            key: 세션 키 (보통 channel:chat_id).
        Returns:
            The session.
        """
        if key in self._cache:
            return self._cache[key]

        session: Session = self._load(key)
        if session is None:
            session = Session(key=key)

        self._cache[key] = session

        return session

    def _load(self, key: str) -> Session | None:
        """디스크로 부터 세션을 가져옵니다."""
        path: Path = self._get_session_path(key)
        if not path.exists():
            legacy_path: Path = self._get_legacy_session_path(key)
            if legacy_path.exists():
                try:
                    shutil.move(str(legacy_path), str(path))
                    logger.info(f"레거시 경로에서 세션 {key}을(를) 마이그레이션했습니다.")
                except Exception:
                    logger.exception("세션 마이그레이션 실패 {}", key)

        if not path.exists():
            return None

        try:
            messages: list[dict[str, Any]] = []
            metadata: dict[str, Any] = {}
            created_at: str | None = None
            last_consolidated: int = 0

            with open(file=path, encoding="utf-8") as f:
                for line in f:
                    line: str = line.strip()
                    if not line:
                        continue

                    data: dict[str, Any] = json.loads(line)
                    if data.get("_type") == "metadata":
                        metadata = data.get("metadata", {})
                        created_at = (
                            datetime.fromisoformat(data["created_at"])
                            if data.get("created_at")
                            else None
                        )
                        last_consolidated = data.get("last_consolidated", 0)
                    else:
                        messages.append(data)

            return Session(
                key=key,
                messages=messages,
                created_at=created_at or datetime.now(),
                metadata=metadata,
                last_consolidated=last_consolidated,
            )
        except Exception as e:
            logger.warning("세션 읽어오기 실패 {}: {}", key, e)
            return None

    def _get_session_path(self, key: str) -> Path:
        """세션의 파일 경로를 가져옵니다."""
        safe_key: str = safe_filename(key.replace(":", "_"))
        return self._session_dir / f"{safe_key}.jsonl"

    def _get_legacy_session_path(self, key: str) -> Path:
        """기존 전역 세션 경로 (~/.shacs-bot/sessions/)."""
        safe_key: str = safe_filename(key.replace(":", "_"))
        return self._legacy_sessions_dir / f"{safe_key}.jsonl"

    def save(self, session: Session) -> None:
        """디스크에 세션 저장"""
        path: Path = self._get_session_path(session.key)
        with open(path, "w", encoding="utf-8") as f:
            metadata_line: dict[str, Any] = {
                "_type": "metadata",
                "key": session.key,
                "created_at": session.created_at.isoformat(),
                "updated_at": session.updated_at.isoformat(),
                "metadata": session.metadata,
                "last_consolidated": session.last_consolidated,
            }
            f.write(json.dumps(metadata_line, ensure_ascii=False) + "\n")

            for msg in session.messages:
                f.write(json.dumps(msg, ensure_ascii=False) + "\n")

        self._cache[session.key] = session

    def invalidate(self, key: str) -> None:
        """인-메모레 캐시에서 세션 삭제"""
        self._cache.pop(key, None)

    def list_sessions(self) -> list[dict[str, Any]]:
        """
        모든 세션 리스트들

        Returns:
            세션 정보 객체의 리스트
        """
        sessions: list[dict[str, Any]] = []

        for path in self._session_dir.glob("*.jsonl"):
            try:
                # 메타데이터 line만 읽기
                with open(path, encoding="utf-8") as f:
                    first_line: str = f.readline().strip()
                    if first_line:
                        data: dict[str, Any] = json.loads(first_line)
                        if data.get("_type") == "metadata":
                            key: str = data.get("key") or path.stem.replace("_", ":", 1)
                            sessions.append(
                                {
                                    "key": key,
                                    "created_at": data.get("created_at"),
                                    "updated_at": data.get("updated_at"),
                                    "path": str(path),
                                }
                            )
            except Exception:
                continue

        return sorted(sessions, key=lambda x: x.get("updated_at", ""), reverse=True)
