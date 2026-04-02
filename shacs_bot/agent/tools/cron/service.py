"""agent 작업 스케줄링을 위한 Cron 서비스입니다."""

import asyncio
import json
import time
import uuid
from datetime import datetime
from pathlib import Path
from typing import Callable, Coroutine, Any
from zoneinfo import ZoneInfo

from croniter import croniter
from loguru import logger

from shacs_bot.agent.tools.cron.types import (
    CronSchedule,
    CronJob,
    CronStore,
    CronPayload,
    CronJobState,
)
from shacs_bot.workflow.runtime import WorkflowRuntime


def _now_ms() -> int:
    return int(time.time() * 1000)


def _compute_next_run(schedule: CronSchedule, now_ms: int) -> int | None:
    """다음 실행 시간을 ms로 계산합니다."""
    if schedule.kind == "at":
        return schedule.at_ms if schedule.at_ms and (schedule.at_ms > now_ms) else None

    elif schedule.kind == "every":
        if not schedule.every_ms or schedule.every_ms <= 0:
            return None
        # 다음 실행 시간은 현재 시간에서 간격을 더한 값입니다.
        return now_ms + schedule.every_ms

    elif schedule.kind == "cron" and schedule.expr:
        try:
            base_time: float = now_ms / 1000
            tz: ZoneInfo = (
                ZoneInfo(schedule.tz) if schedule.tz else datetime.now().astimezone().tzinfo
            )
            base_dt: datetime = datetime.fromtimestamp(timestamp=base_time, tz=tz)
            cron: croniter = croniter(schedule.expr, base_dt)
            next_dt: datetime = cron.get_next(datetime)
            return int(next_dt.timestamp() * 1000)
        except Exception:
            return None

    return None


def _validate_schedule_for_add(schedule: CronSchedule) -> None:
    """실행 불가능한 작업이 생성되는 것을 방지하기 위해 스케줄 필드를 검증합니다."""
    if schedule.tz and schedule.kind != "cron":
        raise ValueError("tz는 cron 스케줄에서만 사용할 수 있습니다.")

    if schedule.kind == "cron" and schedule.tz:
        try:
            ZoneInfo(schedule.tz)
        except Exception:
            raise ValueError(f"알 수 없는 타임존 '{schedule.tz}'") from None


class CronService:
    """스케줄링 된 작업의 실행과 관리를 위한 서비스입니다."""

    def __init__(
        self,
        store_path: Path,
        on_job: Callable[[CronJob], Coroutine[Any, Any, str | None]] | None = None,
        workflow_runtime: WorkflowRuntime | None = None,
    ):
        self._store_path: Path = store_path
        self._on_job: Callable[[CronJob], Coroutine[Any, Any, str | None]] | None = on_job
        self._store: CronStore | None = None
        self._last_mtime: float = 0.0
        self._timer_task: asyncio.Task | None = None
        self._running: bool = False
        self._workflow_runtime: WorkflowRuntime | None = workflow_runtime

    def set_job(self, on_job: Callable[[CronJob], Coroutine[Any, Any, str | None]]) -> None:
        self._on_job = on_job

    async def start(self) -> None:
        """cron 서비스를 시작합니다."""
        self._running: bool = True

        self._load_store()
        self._recompute_next_runs()
        self._save_store()
        self._arm_timer()

        logger.info(
            f"Cron 서비스가 시작되었습니다. {len(self._store.jobs if self._store else [])}개의 작업이 로드되었습니다."
        )

    def _load_store(self) -> CronStore:
        """디스크에서 작업을 로드합니다. 파일이 외부에서 수정된 경우 자동으로 다시 불러옵니다."""
        if self._store and self._store_path.exists():
            mtime: float = self._store_path.stat().st_mtime
            if mtime != self._last_mtime:
                logger.info("Cron: jobs.json 파일이 외부에서 수정되어 다시 불러옵니다.")
                self._store = None

        if self._store:
            return self._store

        if self._store_path.exists():
            try:
                data: dict[str, list[dict[str, Any]]] = json.loads(
                    self._store_path.read_text(encoding="utf-8")
                )
                jobs: list[CronJob] = []

                for job in data.get("jobs", []):
                    jobs.append(
                        CronJob(
                            id=job["id"],
                            name=job["name"],
                            enabled=job.get("enabled", True),
                            schedule=CronSchedule(
                                kind=job["schedule"]["kind"],
                                at_ms=job["schedule"].get("atMs"),
                                every_ms=job["schedule"].get("everyMs"),
                                expr=job["schedule"].get("expr"),
                                tz=job["schedule"].get("tz"),
                            ),
                            payload=CronPayload(
                                kind=job["payload"].get("kind", "agent_turn"),
                                message=job["payload"].get("message", ""),
                                deliver=job["payload"].get("deliver", False),
                                channel=job["payload"].get("channel"),
                                to=job["payload"].get("to"),
                                metadata=job["payload"].get("metadata", {}),
                            ),
                            state=CronJobState(
                                next_run_at_ms=job.get("state", {}).get("nextRunAtMs"),
                                last_run_at_ms=job.get("state", {}).get("lastRunAtMs"),
                                last_status=job.get("state", {}).get("lastStatus"),
                                last_error=job.get("state", {}).get("lastError"),
                            ),
                            created_at_ms=job.get("createdAtMs", 0),
                            updated_at_ms=job.get("updatedAtMs", 0),
                            delete_after_run=job.get("deleteAfterRun", False),
                        )
                    )

                self._store = CronStore(jobs=jobs)
            except Exception as e:
                logger.warning(f"cron store를 로드하는 중 오류가 발생했습니다: {e}")
                self._store = CronStore()
        else:
            self._store = CronStore()

        return self._store

    def _recompute_next_runs(self) -> None:
        """모든 작업에 대해 다음 실행 시간을 재계산합니다."""
        if not self._store:
            return

        now: int = _now_ms()
        for job in self._store.jobs:
            if job.enabled:
                job.state.next_run_at_ms = _compute_next_run(job.schedule, now)

    def _save_store(self) -> None:
        """현재 작업을 저장소에 저장합니다."""
        if not self._store:
            return

        self._store_path.parent.mkdir(parents=True, exist_ok=True)

        data = {
            "version": self._store.version,
            "jobs": [
                {
                    "id": j.id,
                    "name": j.name,
                    "enabled": j.enabled,
                    "schedule": {
                        "kind": j.schedule.kind,
                        "atMs": j.schedule.at_ms,
                        "everyMs": j.schedule.every_ms,
                        "expr": j.schedule.expr,
                        "tz": j.schedule.tz,
                    },
                    "payload": {
                        "kind": j.payload.kind,
                        "message": j.payload.message,
                        "deliver": j.payload.deliver,
                        "channel": j.payload.channel,
                        "to": j.payload.to,
                        "metadata": j.payload.metadata,
                    },
                    "state": {
                        "nextRunAtMs": j.state.next_run_at_ms,
                        "lastRunAtMs": j.state.last_run_at_ms,
                        "lastStatus": j.state.last_status,
                        "lastError": j.state.last_error,
                    },
                    "createdAtMs": j.created_at_ms,
                    "updatedAtMs": j.updated_at_ms,
                    "deleteAfterRun": j.delete_after_run,
                }
                for j in self._store.jobs
            ],
        }

        self._store_path.write_text(
            json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8"
        )
        self._last_mtime = self._store_path.stat().st_mtime

    def _arm_timer(self) -> None:
        """다음 타이머 실행을 예약합니다."""
        if self._timer_task:
            self._timer_task.cancel()

        next_wake: int = self._get_next_wake_ms()
        if not next_wake or not self._running:
            return

        delay_ms = max(0, next_wake - _now_ms())
        delay_s = delay_ms / 1000

        async def tick():
            await asyncio.sleep(delay_s)
            if self._running:
                await self._on_timer()

        self._timer_task = asyncio.create_task(tick())

    def _get_next_wake_ms(self) -> int | None:
        """전체 작업 중 가장 빠른 다음 실행 시간을 조회합니다."""
        if not self._store:
            return None

        times: list[int] = [
            job.state.next_run_at_ms
            for job in self._store.jobs
            if job.enabled and job.state.next_run_at_ms
        ]
        return min(times) if times else None

    def stop(self) -> None:
        """Cron 서비스를 중지합니다."""
        self._running = False
        if self._timer_task:
            self._timer_task.cancel()
            self._timer_task = None

    async def _execute_job(self, job: CronJob) -> None:
        """하나의 작업을 실행합니다."""
        start_ms: int = _now_ms()
        workflow_id: str = self._create_workflow(job)
        self._start_workflow(workflow_id)
        logger.info(f"Cron: 작업 실행 시작 '{job.name}' ({job.id})")

        try:
            response: str | None = None
            if self._on_job:
                response = await self._on_job(job)
            if response:
                self._annotate_result(workflow_id, response)

            job.state.last_status = "ok"
            job.state.last_error = None
            self._complete_workflow(workflow_id)
            logger.info(f"Cron: 작업 '{job.name}' 성공")
        except Exception as e:
            job.state.last_status = "error"
            job.state.last_error = str(e)
            self._annotate_result(workflow_id, str(e))
            self._fail_workflow(workflow_id, str(e))
            logger.error(f"Cron: 작업 '{job.name}' 실패: {e}")

        job.state.last_run_at_ms = start_ms
        job.updated_at_ms = _now_ms()

        # 일회성 작업을 처리합니다.
        if job.schedule.kind == "at":
            if job.delete_after_run:
                self._store.jobs = [j for j in self._store.jobs if j.id != job.id]
            else:
                job.enabled = False
                job.state.next_run_at_ms = None
        else:
            # 다음 작업을 계산합니다
            job.state.next_run_at_ms = _compute_next_run(job.schedule, _now_ms())

    def _create_workflow(self, job: CronJob) -> str:
        if self._workflow_runtime is None:
            return ""
        record = self._workflow_runtime.store.create(
            source_kind="cron",
            goal=job.payload.message,
            notify_target={
                "channel": job.payload.channel or "cli",
                "chat_id": job.payload.to or "direct",
                "session_key": f"cron:{job.id}",
            },
            metadata={"cronJobId": job.id, "jobName": job.name},
        )
        self._workflow_runtime.store.upsert(record)
        job.payload.metadata["workflowId"] = record.workflow_id
        return record.workflow_id

    def _start_workflow(self, workflow_id: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.start(workflow_id)

    def _complete_workflow(self, workflow_id: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.complete(workflow_id)

    def _annotate_result(self, workflow_id: str, result: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.annotate_result(workflow_id, result)

    def _fail_workflow(self, workflow_id: str, error: str) -> None:
        if self._workflow_runtime is None or not workflow_id:
            return
        _ = self._workflow_runtime.fail(workflow_id, last_error=error)

    async def _on_timer(self) -> None:
        """타이머 틱을 처리합니다. - 실행 시간이 된 작업을 실행합니다."""
        self._load_store()
        if not self._store:
            return

        now: int = _now_ms()
        due_jobs: list[CronJob] = [
            job
            for job in self._store.jobs
            if job.enabled and job.state.next_run_at_ms and (now >= job.state.next_run_at_ms)
        ]

        for job in due_jobs:
            await self._execute_job(job)

        self._save_store()
        self._arm_timer()

    # Cron 작업 관리 메서드 (추가, 제거, 조회 등)을 여기에 추가할 수 있습니다.

    def list_jobs(self, include_disabled: bool = False) -> list[CronJob]:
        """등록된 작업 목록을 반환합니다."""
        store: CronStore = self._load_store()
        jobs: list[CronJob] = (
            store.jobs if include_disabled else [job for job in store.jobs if job.enabled]
        )
        return sorted(jobs, key=lambda job: job.state.next_run_at_ms or float("inf"))

    def add_job(
        self,
        name: str,
        schedule: CronSchedule,
        message: str,
        deliver: bool = False,
        channel: str | None = None,
        to: str | None = None,
        delete_after_run: bool = False,
        metadata: dict[str, Any] | None = None,
    ) -> CronJob:
        """새로운 작업을 추가합니다."""
        store: CronStore = self._load_store()
        _validate_schedule_for_add(schedule=schedule)
        now: int = _now_ms()

        job: CronJob = CronJob(
            id=str(uuid.uuid4())[:8],
            name=name,
            enabled=True,
            schedule=schedule,
            payload=CronPayload(
                kind="agent_turn",
                message=message,
                deliver=deliver,
                channel=channel,
                to=to,
                metadata=metadata or {},
            ),
            state=CronJobState(next_run_at_ms=_compute_next_run(schedule, now)),
            created_at_ms=now,
            updated_at_ms=now,
            delete_after_run=delete_after_run,
        )
        store.jobs.append(job)

        self._save_store()
        self._arm_timer()

        logger.info(f"Cron: 작업이 추가되었습니다 - '{job.name}' ({job.id})")
        return job

    def remove_job(self, job_id: str) -> bool:
        """작업 ID로 작업을 제거합니다."""
        store: CronStore = self._load_store()
        before_len: int = len(store.jobs)

        store.jobs = [job for job in store.jobs if job.id != job_id]

        removed: bool = len(store.jobs) < before_len

        if removed:
            self._save_store()
            self._arm_timer()
            logger.info(f"Cron: 작업이 제거되었습니다 - ID: {job_id}")

        return removed

    def enable_job(self, job_id: str, enabled: bool = True) -> CronJob | None:
        """작업 ID로 작업을 활성화 또는 비활성화합니다."""
        store: CronStore = self._load_store()
        for job in store.jobs:
            if job.id == job_id:
                job.enabled = enabled
                job.updated_at_ms = _now_ms()

                if enabled:
                    job.state.next_run_at_ms = _compute_next_run(job.schedule, _now_ms())
                else:
                    job.state.next_run_at_ms = None

                self._save_store()
                self._arm_timer()

                return job

        return None

    async def run_job(self, job_id: str, force: bool = False) -> bool:
        """일반적인 작업을 시작합니다."""
        store: CronStore = self._load_store()
        for job in store.jobs:
            if job.id == job_id:
                if not force and not job.enabled:
                    return False

                await self._execute_job(job)

                self._save_store()
                self._arm_timer()

                return True

        return False

    def status(self) -> dict[str, Any]:
        """서비스 상태를 반환합니다."""
        store: CronStore = self._load_store()
        return {
            "enabled": self._running,
            "jobs": len(store.jobs) if store else 0,
            "next_wake_at_ms": self._get_next_wake_ms(),
        }
