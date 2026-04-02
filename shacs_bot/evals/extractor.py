from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path

from shacs_bot.agent.session.manager import SessionManager
from shacs_bot.evals.models import EvaluationCase
from shacs_bot.utils.helpers import ensure_dir, safe_filename


def get_auto_cases_dir(workspace: Path) -> Path:
    auto_dir: Path = ensure_dir(workspace / "evals" / "cases" / "auto")
    return auto_dir


def build_auto_cases_path(workspace: Path, name: str | None = None) -> Path:
    filename: str = safe_filename(name) if name else datetime.now().strftime("%Y-%m-%d-%H-%M-%S")
    return get_auto_cases_dir(workspace) / f"{filename}.json"


class SessionCaseExtractor:
    def __init__(self, workspace: Path, session_manager: SessionManager) -> None:
        self._workspace: Path = workspace
        self._sessions: SessionManager = session_manager

    def extract_cases(
        self,
        session_filter: str | None = None,
        session_limit: int = 10,
        case_limit: int = 20,
        include_eval_sessions: bool = False,
    ) -> list[EvaluationCase]:
        sessions: list[dict[str, object]] = self._sessions.list_sessions()
        cases: list[EvaluationCase] = []

        for item in sessions:
            session_key_value: object = item.get("key", "")
            if not isinstance(session_key_value, str) or not session_key_value:
                continue
            session_key: str = session_key_value
            if not include_eval_sessions and session_key.startswith("eval:"):
                continue
            if session_filter and session_filter not in session_key:
                continue

            session = self._sessions.get_or_create(session_key)
            cases.extend(self._extract_session_cases(session_key, session.messages))
            if len(cases) >= case_limit:
                return cases[:case_limit]

            session_limit -= 1
            if session_limit <= 0:
                break

        return cases[:case_limit]

    def write_cases_file(self, path: Path, cases: list[EvaluationCase]) -> Path:
        _ = ensure_dir(path.parent)
        payload = {
            "cases": [case.model_dump(mode="json", by_alias=True) for case in cases],
        }
        _ = path.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        return path

    def _extract_session_cases(
        self,
        session_key: str,
        messages: list[dict[str, object]],
    ) -> list[EvaluationCase]:
        channel: str = session_key.split(":", 1)[0] if ":" in session_key else session_key
        safe_key: str = safe_filename(session_key.replace(":", "_"))
        user_turn: int = 0
        extracted: list[EvaluationCase] = []

        for index, message in enumerate(messages):
            if message.get("role") != "user":
                continue

            content = message.get("content")
            if not isinstance(content, str):
                continue

            text: str = content.strip()
            if not text:
                continue

            user_turn += 1
            extracted.append(
                EvaluationCase(
                    case_id=f"{safe_key}-{user_turn:03d}",
                    input=text,
                    expected_mode="response",
                    tags=["auto", "session", channel],
                    notes=f"Extracted from session {session_key}",
                    source_session_key=session_key,
                    source_message_index=index,
                    source_timestamp=self._read_timestamp(message),
                    source_channel=channel,
                )
            )

        return extracted

    @staticmethod
    def _read_timestamp(message: dict[str, object]) -> str:
        timestamp = message.get("timestamp")
        return timestamp if isinstance(timestamp, str) else ""
