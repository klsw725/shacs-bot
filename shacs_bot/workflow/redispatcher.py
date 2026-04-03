from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

from loguru import logger

from shacs_bot.workflow.runtime import WorkflowRuntime

if TYPE_CHECKING:
    from shacs_bot.agent.tools.cron.service import CronService
    from shacs_bot.agent.subagent import SubagentManager


class WorkflowRedispatcher:
    def __init__(
        self,
        *,
        workflow_runtime: WorkflowRuntime,
        cron_service: "CronService",
        subagent_manager: "SubagentManager | None" = None,
        poll_interval_s: int = 5,
    ) -> None:
        self._workflow_runtime: WorkflowRuntime = workflow_runtime
        self._cron_service: CronService = cron_service
        self._subagent_manager: SubagentManager | None = subagent_manager
        self._poll_interval_s: int = poll_interval_s
        self._running: bool = False
        self._task: asyncio.Task[None] | None = None

    async def start(self) -> None:
        if self._running:
            return
        self._running = True
        self._task = asyncio.create_task(self._run_loop())

    def stop(self) -> None:
        self._running = False
        if self._task is not None:
            _ = self._task.cancel()
            self._task = None

    async def _run_loop(self) -> None:
        while self._running:
            try:
                await self._tick()
                await asyncio.sleep(self._poll_interval_s)
            except asyncio.CancelledError:
                break
            except Exception as exc:
                logger.error("WorkflowRedispatcher failed: {}", exc)

    async def _tick(self) -> None:
        queued = [
            record
            for record in self._workflow_runtime.store.list_incomplete()
            if record.state == "queued"
        ]
        queued.sort(key=lambda record: record.updated_at)

        for record in queued:
            if record.source_kind == "subagent":
                if self._subagent_manager is None:
                    logger.warning(
                        "WorkflowRedispatcher: subagent manager unavailable for {}",
                        record.workflow_id,
                    )
                    continue
                success = await self._subagent_manager.execute_existing_workflow(record.workflow_id)
                if not success:
                    logger.warning(
                        "WorkflowRedispatcher: subagent redispatch skipped for {}",
                        record.workflow_id,
                    )
                continue
            if record.source_kind != "cron":
                continue
            cron_job_id = record.metadata.get("cronJobId")
            if not isinstance(cron_job_id, str) or not cron_job_id:
                _ = self._workflow_runtime.fail(
                    record.workflow_id,
                    last_error="missing cronJobId for redispatch",
                )
                continue
            success = await self._cron_service.execute_existing_workflow(
                record.workflow_id, cron_job_id
            )
            if not success:
                logger.warning(
                    "WorkflowRedispatcher: cron redispatch skipped for {}",
                    record.workflow_id,
                )
