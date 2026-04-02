from __future__ import annotations

from datetime import datetime
from pathlib import Path

from shacs_bot.workflow.models import WorkflowRecord, WorkflowState
from shacs_bot.workflow.store import WorkflowStore

TERMINAL_STATES: frozenset[str] = frozenset({"completed", "failed"})
RESUMABLE_STATES: frozenset[str] = frozenset({"running", "waiting_input", "retry_wait"})
ALLOWED_TRANSITIONS: dict[str, frozenset[str]] = {
    "queued": frozenset({"running"}),
    "running": frozenset({"waiting_input", "retry_wait", "completed", "failed"}),
    "waiting_input": frozenset({"queued", "completed", "failed"}),
    "retry_wait": frozenset({"queued", "completed", "failed"}),
    "completed": frozenset(),
    "failed": frozenset(),
}


class WorkflowRuntime:
    def __init__(self, workspace: Path | None = None, store: WorkflowStore | None = None) -> None:
        self._store: WorkflowStore = store or WorkflowStore(workspace)

    @property
    def store(self) -> WorkflowStore:
        return self._store

    def start(self, workflow_id: str) -> WorkflowRecord | None:
        return self._transition(workflow_id, "running", next_run_at="", last_error="")

    def wait_for_input(self, workflow_id: str) -> WorkflowRecord | None:
        return self._transition(workflow_id, "waiting_input", next_run_at="")

    def schedule_retry(
        self,
        workflow_id: str,
        *,
        next_run_at: str,
        last_error: str = "",
        increment_retries: bool = True,
    ) -> WorkflowRecord | None:
        return self._transition(
            workflow_id,
            "retry_wait",
            next_run_at=next_run_at,
            last_error=last_error,
            retries_delta=1 if increment_retries else 0,
        )

    def complete(self, workflow_id: str) -> WorkflowRecord | None:
        return self._transition(workflow_id, "completed", next_run_at="")

    def fail(self, workflow_id: str, *, last_error: str = "") -> WorkflowRecord | None:
        return self._transition(
            workflow_id,
            "failed",
            next_run_at="",
            last_error=last_error,
        )

    def resume(self, workflow_id: str) -> WorkflowRecord | None:
        record: WorkflowRecord | None = self._store.get(workflow_id)
        if record is None or record.state in TERMINAL_STATES:
            return None
        if record.state == "queued":
            return self._store.upsert_and_get(record.model_copy(update={"next_run_at": ""}))
        if record.state not in RESUMABLE_STATES:
            return None
        return self._transition(workflow_id, "queued", next_run_at="")

    def resume_incomplete(self) -> list[WorkflowRecord]:
        resumed: list[WorkflowRecord] = []
        for record in self._store.list_incomplete():
            updated: WorkflowRecord | None = self.resume(record.workflow_id)
            if updated is not None:
                resumed.append(updated)
        return resumed

    def recover_restart(self) -> list[WorkflowRecord]:
        recovered: list[WorkflowRecord] = []
        for record in self._store.list_incomplete():
            if record.state == "running":
                updated = self._store.upsert_and_get(
                    record.model_copy(
                        update={
                            "state": "queued",
                            "metadata": {
                                **record.metadata,
                                "recoveredAt": datetime.now().astimezone().isoformat(),
                            },
                        }
                    )
                )
                recovered.append(updated)
                continue
            if record.state == "retry_wait" and self._is_retry_due(record):
                updated = self.resume(record.workflow_id)
                if updated is not None:
                    updated = self._store.upsert_and_get(
                        updated.model_copy(
                            update={
                                "metadata": {
                                    **updated.metadata,
                                    "recoveredAt": datetime.now().astimezone().isoformat(),
                                },
                            }
                        )
                    )
                    recovered.append(updated)
        return recovered

    def annotate_result(self, workflow_id: str, result: str) -> WorkflowRecord | None:
        preview: str = result[:500]
        return self._update_metadata(workflow_id, resultPreview=preview)

    def mark_notified(
        self,
        workflow_id: str,
        *,
        channel: str,
        chat_id: str,
    ) -> WorkflowRecord | None:
        return self._update_metadata(
            workflow_id,
            notifyDelivered=True,
            notifyChannel=channel,
            notifyChatId=chat_id,
            notifiedAt=datetime.now().astimezone().isoformat(),
        )

    def mark_notify_delegated(self, workflow_id: str) -> WorkflowRecord | None:
        return self._update_metadata(
            workflow_id,
            notifyDelegated=True,
            notifiedAt=datetime.now().astimezone().isoformat(),
        )

    def _transition(
        self,
        workflow_id: str,
        next_state: WorkflowState,
        *,
        next_run_at: str | None = None,
        last_error: str | None = None,
        retries_delta: int = 0,
    ) -> WorkflowRecord | None:
        record: WorkflowRecord | None = self._store.get(workflow_id)
        if record is None:
            return None
        if not self._can_transition(record.state, next_state):
            raise ValueError(f"invalid workflow transition: {record.state} -> {next_state}")

        updates: dict[str, object] = {"state": next_state}
        if next_run_at is not None:
            updates["next_run_at"] = next_run_at
        if last_error is not None:
            updates["last_error"] = last_error
        if retries_delta:
            updates["retries"] = record.retries + retries_delta

        return self._store.upsert_and_get(record.model_copy(update=updates))

    def _can_transition(self, current_state: WorkflowState, next_state: WorkflowState) -> bool:
        return next_state in ALLOWED_TRANSITIONS.get(current_state, frozenset())

    def _is_retry_due(self, record: WorkflowRecord) -> bool:
        if record.state != "retry_wait" or not record.next_run_at:
            return False
        try:
            next_run_at = datetime.fromisoformat(record.next_run_at)
        except ValueError:
            return True
        return next_run_at <= datetime.now().astimezone()

    def _update_metadata(self, workflow_id: str, **entries: object) -> WorkflowRecord | None:
        record: WorkflowRecord | None = self._store.get(workflow_id)
        if record is None:
            return None
        metadata: dict[str, object] = {**record.metadata, **entries}
        return self._store.upsert_and_get(record.model_copy(update={"metadata": metadata}))
