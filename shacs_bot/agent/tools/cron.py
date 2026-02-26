"""알림과 작업 스케줄링을 위한 Cron 도구"""
from typing import Any

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
                "description": "수행할 작업"
            },
            "message": {
                "type": "string",
                "description": "알림 메시지 (추가 시)"
            },
            "every_seconds": {
                "type": "integer",
                "description": "반복 작업의 간격(초 단위)"
            },
            "cron_expr": {
                "type": "string",
                "description": "'0 9 * * *'와 같은 크론 표현식(예약된 작업 시)"
            },
            "job_id": {
                "type": "string",
                "description": "작업 ID(제거 시)"
            }
        },
        "required": ["action"]
    }

    def __init__(self, cron_service: CronService):
        self._cron: CronService = cron_service
        self._channel: str = ""
        self._chat_id: str = ""

    def set_context(self, channel: str, chat_id: str) -> None:
        """전달을 위한 현재 세션 컨텍스트를 설정합니다."""
        self._channel: str = channel
        self._chat_id: str = chat_id

    async def execute(
            self,
            action: str,
            message: str = "",
            every_seconds: int | None = None,
            cron_expr: str | None = None,
            job_id: str | None = None,
            **kwargs: Any
    ) -> str:
        if action == "add":
            return self._add_job(message, every_seconds, cron_expr)
        elif action == "list":
            self._list_jobs()
        elif action == "remove":
            return self._remove_job(job_id)
        return f"알 수 없는 작업: {action}"

    def _add_job(self, message: str, every_seconds: int | None, cron_expr: str | None) -> str:
        if not message:
            return "에러: 추가를 위해 메시지가 필요합니다."
        if not self._channel or not self._chat_id:
            return "에러: 세션 컨텍스트가 설정되지 않았습니다 (channel/chat_id)."

        # 스케줄 추가
        if every_seconds:
            schedule: CronSchedule = CronSchedule(kind="every", every_ms=every_seconds * 1000)
        elif cron_expr:
            schedule = CronSchedule(kind="cron", expr=cron_expr)
        else:
            return "에러: every_seconds 또는 cron_expr 중 하나는 필요합니다."

        job: CronJob = self._cron.add_job(
            name=message[:30],
            schedule=schedule,
            message=message,
            deliver=True,
            channel=self._channel,
            to=self._chat_id,
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
