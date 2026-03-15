"""알림과 작업 스케줄링을 위한 Cron 도구"""

from _contextvars import ContextVar, Token
from datetime import datetime
from typing import Any
from zoneinfo import ZoneInfo

from shacs_bot.agent.tools.base import Tool
from shacs_bot.agent.tools.cron.service import CronService
from shacs_bot.agent.tools.cron.types import CronSchedule, CronJob


class CronTool(Tool):
    """스케줄 알림과 반복 작업을 위한 도구입니다."""

    name = "cron"
    description = "알림과 반복 작업을 스케줄링합니다. Actions: add, list, remove."
    parameters = {
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["add", "list", "remove"],
                "description": "수행할 작업",
            },
            "message": {"type": "string", "description": "알림 메시지 (추가 시)"},
            "every_seconds": {"type": "integer", "description": "반복 작업의 간격(초 단위)"},
            "cron_expr": {
                "type": "string",
                "description": "'0 9 * * *'와 같은 크론 표현식(예약된 작업 시)",
            },
            "at": {
                "type": "string",
                "description": "일회성 알림의 실행 시각 (ISO 형식: YYYY-MM-DDTHH:MM:SS). 지정 시간에 한 번 실행 후 자동 삭제됩니다.",
            },
            "job_id": {"type": "string", "description": "작업 ID(제거 시)"},
        },
        "required": ["action"],
    }

    def __init__(self, cron_service: CronService):
        self._cron: CronService = cron_service
        self._channel: str = ""
        self._chat_id: str = ""
        self._in_cron_context: ContextVar[bool] = ContextVar("cron_in_context", default=False)

    def set_context(self, channel: str, chat_id: str) -> None:
        """전달을 위한 현재 세션 컨텍스트를 설정합니다."""
        self._channel: str = channel
        self._chat_id: str = chat_id

    def set_cron_context(self, active: bool) -> Token[bool]:
        """도구가 cron 작업 콜백 내부에서 실행 중인지 여부를 표시한다."""
        return self._in_cron_context.set(active)

    def reset_cron_context(self, token: Token[bool]) -> None:
        """이전 cron 컨텍스트 복구"""
        self._in_cron_context.reset(token)

    async def execute(
        self,
        action: str,
        message: str = "",
        every_seconds: int | None = None,
        cron_expr: str | None = None,
        tz: str | None = None,
        at: str | None = None,
        job_id: str | None = None,
        **kwargs: Any,
    ) -> str:
        if action == "add":
            if self._in_cron_context.get():
                return "에러: cron 작업 실행 중에는 새로운 작업을 예약할 수 없습니다."

            return self._add_job(
                message=message, every_seconds=every_seconds, cron_expr=cron_expr, tz=tz, at=at
            )
        elif action == "list":
            self._list_jobs()
        elif action == "remove":
            return self._remove_job(job_id)

        return f"알 수 없는 작업: {action}"

    def _add_job(
        self,
        message: str,
        every_seconds: int | None,
        cron_expr: str | None,
        tz: str | None = None,
        at: str | None = None,
    ) -> str:
        if not message:
            return "에러: 추가를 위해 메시지가 필요합니다."

        if not self._channel or not self._chat_id:
            return "에러: 세션 컨텍스트가 설정되지 않았습니다 (channel/chat_id)."

        if tz and not cron_expr:
            return "에러: tz는 cron_expr와 함께 사용할 때만 가능합니다."

        if tz:
            try:
                ZoneInfo(tz)
            except (KeyError, Exception):
                return f"에러: 알 수 없는 시간대 '{tz}'"

        # 스케줄 추가
        delete_after = False
        if every_seconds:
            schedule: CronSchedule = CronSchedule(kind="every", every_ms=every_seconds * 1000)
        elif cron_expr:
            schedule = CronSchedule(kind="cron", expr=cron_expr)
        elif at:
            try:
                dt: datetime = datetime.fromisoformat(at)
            except ValueError:
                return (
                    f"에러: 잘못된 ISO datetime 형식입니다 '{at}'. 예상 형식: YYYY-MM-DDTHH:MM:SS"
                )

            at_ms: int = int(dt.timestamp() * 1000)
            schedule: CronSchedule = CronSchedule(kind="at", at_ms=at_ms)

            delete_after = True
        else:
            return "에러: every_seconds, cron_expr, at 중 하나는 필요합니다."

        job: CronJob = self._cron.add_job(
            name=message[:30],
            schedule=schedule,
            message=message,
            deliver=True,
            channel=self._channel,
            to=self._chat_id,
            delete_after_run=delete_after,
        )

        return f"작업이 추가되었습니다: '{job.name}' (id: {job.id}"

    def _list_jobs(self) -> str:
        jobs: list[CronJob] = self._cron.list_jobs()
        if not jobs:
            return "예약된 작업이 없습니다."

        lines: list[str] = [f"- {job.name} (id: {job.id}, {job.schedule.kind})" for job in jobs]
        return "스케줄된 작업:\n" + "\n".join(lines)

    def _remove_job(self, job_id: str | None) -> str:
        if not job_id:
            return "에러: 제거를 위해 job_id가 필요합니다."

        if self._cron.remove_job(job_id):
            return f"작업이 제거되었습니다: {job_id}"

        return f"작업을 찾을 수 없습니다: {job_id}"
