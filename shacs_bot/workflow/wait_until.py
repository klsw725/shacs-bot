"""wait_until 스텝 description에서 대기 시각을 파싱합니다."""
from __future__ import annotations

import re
from datetime import datetime, timedelta


def parse_wait_until_time(description: str) -> datetime:
    """step description에서 대기 시각을 파싱하여 timezone-aware datetime을 반환합니다.

    지원 패턴 (우선순위 순):
    1. ISO 8601 datetime  예: ``2026-04-03T14:00``, ``2026-04-03 14:00:00``
    2. 상대 기간          예: ``30분``, ``2 hours``, ``3 days``, ``3일``
    3. 내일/tomorrow      예: ``내일 09:30``, ``tomorrow 14:00``
    4. 폴백               현재 시각 기준 5분 후
    """
    now = datetime.now().astimezone()

    # 1. ISO 8601 datetime
    iso_match = re.search(
        r"(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2})?(?:[+-]\d{2}:?\d{2}|Z)?)",
        description,
    )
    if iso_match:
        try:
            dt = datetime.fromisoformat(iso_match.group(1).replace(" ", "T"))
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=now.tzinfo)
            return dt
        except ValueError:
            pass

    # 2. 상대 기간
    rel_match = re.search(
        r"(\d+)\s*(분|minutes?|mins?|시간|hours?|hrs?|일|days?)",
        description,
        flags=re.IGNORECASE,
    )
    if rel_match:
        n = int(rel_match.group(1))
        unit = rel_match.group(2).lower()
        if unit in ("분", "min", "mins", "minute", "minutes"):
            return now + timedelta(minutes=n)
        if unit in ("시간", "hour", "hours", "hr", "hrs"):
            return now + timedelta(hours=n)
        if unit in ("일", "day", "days"):
            return now + timedelta(days=n)

    # 3. 내일/tomorrow HH:MM
    tomorrow_match = re.search(
        r"(?:tomorrow|내일)\s+(\d{1,2}):(\d{2})",
        description,
        flags=re.IGNORECASE,
    )
    if tomorrow_match:
        h, m = int(tomorrow_match.group(1)), int(tomorrow_match.group(2))
        return (now + timedelta(days=1)).replace(hour=h, minute=m, second=0, microsecond=0)

    # 4. 폴백: 5분 후
    return now + timedelta(minutes=5)
