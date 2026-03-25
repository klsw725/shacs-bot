"""턴 단위 토큰/비용 누적 및 JSONL 영구 저장."""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import date, datetime
from pathlib import Path

from loguru import logger


@dataclass
class TurnUsage:
    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0
    cache_read_tokens: int = 0
    cache_creation_tokens: int = 0
    llm_calls: int = 0
    cost_usd: float = 0.0
    model: str = ""
    provider: str = ""

    def accumulate(self, usage: dict[str, int], model: str, provider: str) -> None:
        self.prompt_tokens += usage.get("prompt_tokens", 0)
        self.completion_tokens += usage.get("completion_tokens", 0)
        self.total_tokens += usage.get("total_tokens", 0)
        self.cache_read_tokens += usage.get("cache_read_input_tokens", 0)
        self.cache_creation_tokens += usage.get("cache_creation_input_tokens", 0)
        self.llm_calls += 1
        self.model = model
        self.provider = provider

        self.cost_usd += _compute_cost(
            model=model,
            prompt_tokens=usage.get("prompt_tokens", 0),
            completion_tokens=usage.get("completion_tokens", 0),
            cache_read_tokens=usage.get("cache_read_input_tokens", 0),
        )

    def format_footer(self, mode: str) -> str:
        if mode == "off" or self.total_tokens == 0:
            return ""

        tokens_str = _format_tokens(self.total_tokens)

        if mode == "tokens":
            return f"\U0001f4ca {tokens_str} tokens"

        # mode == "full"
        parts: list[str] = [f"{tokens_str} tokens"]
        if self.cost_usd > 0:
            parts.append(f"${self.cost_usd:.4f}")
        if self.llm_calls > 1:
            parts.append(f"{self.llm_calls} calls")

        model_short = self.model.rsplit("/", 1)[-1] if "/" in self.model else self.model
        parts.append(model_short)

        return f"\U0001f4ca {' \u00b7 '.join(parts)}"


class UsageTracker:
    def __init__(self, data_dir: Path) -> None:
        self._data_dir = data_dir
        self._data_dir.mkdir(parents=True, exist_ok=True)

    def record(self, session_key: str, turn: TurnUsage) -> None:
        if turn.total_tokens == 0:
            return

        today = date.today().isoformat()
        path = self._data_dir / f"{today}.jsonl"

        entry = {
            "ts": datetime.now().isoformat(timespec="seconds"),
            "session": session_key,
            "model": turn.model,
            "provider": turn.provider,
            "prompt": turn.prompt_tokens,
            "completion": turn.completion_tokens,
            "cache_read": turn.cache_read_tokens,
            "cost": round(turn.cost_usd, 6),
            "calls": turn.llm_calls,
        }

        try:
            with path.open("a", encoding="utf-8") as f:
                f.write(json.dumps(entry, ensure_ascii=False) + "\n")
        except OSError:
            logger.warning("사용량 기록 실패: {}", path)

    def get_session_summary(self, session_key: str) -> dict[str, int | float]:
        return self._aggregate(session_filter=session_key)

    def get_daily_summary(self, target_date: str | None = None) -> dict[str, int | float]:
        return self._aggregate(target_date=target_date)

    def _aggregate(
        self,
        target_date: str | None = None,
        session_filter: str | None = None,
    ) -> dict[str, int | float]:
        today = target_date or date.today().isoformat()
        path = self._data_dir / f"{today}.jsonl"

        result: dict[str, int | float] = {
            "prompt": 0,
            "completion": 0,
            "total": 0,
            "cost": 0.0,
            "calls": 0,
            "sessions": 0,
        }

        if not path.exists():
            return result

        seen_sessions: set[str] = set()

        try:
            with path.open("r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        entry = json.loads(line)
                    except json.JSONDecodeError:
                        continue

                    if session_filter and entry.get("session") != session_filter:
                        continue

                    result["prompt"] += entry.get("prompt", 0)
                    result["completion"] += entry.get("completion", 0)
                    result["total"] += entry.get("prompt", 0) + entry.get("completion", 0)
                    result["cost"] += entry.get("cost", 0.0)
                    result["calls"] += entry.get("calls", 0)
                    seen_sessions.add(entry.get("session", ""))
        except OSError:
            logger.warning("사용량 파일 읽기 실패: {}", path)

        result["sessions"] = len(seen_sessions)
        return result


_PRICE_PER_MILLION: dict[str, tuple[float, float]] = {
    "claude-sonnet-4-20250514": (3.0, 15.0),
    "claude-opus-4-20250514": (15.0, 75.0),
    "claude-3-5-haiku-20241022": (0.8, 4.0),
    "gpt-4o": (2.5, 10.0),
    "gpt-4o-mini": (0.15, 0.60),
    "gpt-4.1": (2.0, 8.0),
    "gpt-4.1-mini": (0.4, 1.6),
    "gpt-4.1-nano": (0.1, 0.4),
    "o3": (2.0, 8.0),
    "o3-mini": (1.1, 4.4),
    "o4-mini": (1.1, 4.4),
    "deepseek-chat": (0.27, 1.10),
    "deepseek-reasoner": (0.55, 2.19),
    "gemini-2.5-flash": (0.15, 0.60),
    "gemini-2.5-pro": (1.25, 10.0),
}


def _compute_cost(
    model: str,
    prompt_tokens: int,
    completion_tokens: int,
    cache_read_tokens: int = 0,
) -> float:
    model_key = model.split("/")[-1].lower()
    for key, (input_price, output_price) in _PRICE_PER_MILLION.items():
        if key in model_key or model_key in key:
            return (prompt_tokens * input_price + completion_tokens * output_price) / 1_000_000
    return 0.0


def _format_tokens(count: int) -> str:
    if count >= 1_000_000:
        return f"{count / 1_000_000:.1f}M"
    if count >= 1_000:
        return f"{count / 1_000:.1f}K"
    return str(count)
