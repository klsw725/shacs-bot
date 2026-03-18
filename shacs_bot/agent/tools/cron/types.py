"""Cron types."""

from dataclasses import dataclass, field
from typing import Any, Literal


@dataclass
class CronSchedule:
    """cron job의 스케줄 정의입니다."""

    kind: Literal["at", "every", "cron"]
    # "at"의 경우 ms 단위의 타임스탬프
    at_ms: int | None = None
    # "every"의 경우 ms 단위의 간격
    every_ms: int | None = None
    # "cron"의 경우 cron 표현식 (예: "0 9 * * *")
    expr: str | None = None
    # cron 표현식의 타임존
    tz: str | None = None


@dataclass
class CronPayload:
    """작업이 실행될 때 수행할 작업 정의입니다."""

    kind: Literal["system_event", "agent_turn"] = "agent_turn"
    message: str = ""
    # 채널로 응답 전달 여부
    deliver: bool = False
    channel: str | None = None  # 예: "whatsapp"
    to: str | None = None  # 예: 전화번호
    # 스레드 라우팅 등을 위한 채널 메타데이터
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class CronJobState:
    """작업의 런타임 상태입니다."""

    next_run_at_ms: int | None = None
    last_run_at_ms: int | None = None
    last_status: Literal["ok", "error", "skipped"] | None = None
    last_error: str | None = None


@dataclass
class CronJob:
    """스케줄된 작업입니다."""

    id: str
    name: str
    enabled: bool = True
    schedule: CronSchedule = field(default_factory=lambda: CronSchedule(kind="every"))
    payload: CronPayload = field(default_factory=CronPayload)
    state: CronJobState = field(default_factory=CronJobState)
    created_at_ms: int = 0
    updated_at_ms: int = 0
    delete_after_run: bool = False


@dataclass
class CronStore:
    """cron 작업의 영속적 저장소입니다."""

    version: int = 1
    jobs: list[CronJob] = field(default_factory=list)
