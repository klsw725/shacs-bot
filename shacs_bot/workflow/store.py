from __future__ import annotations

import json
import uuid
from datetime import datetime
from pathlib import Path

from loguru import logger

from shacs_bot.config.paths import get_workspace_path
from shacs_bot.utils.helpers import ensure_dir, safe_filename
from shacs_bot.workflow.models import NotifyTarget, WorkflowRecord, WorkflowSourceKind

INCOMPLETE_STATES: frozenset[str] = frozenset({"queued", "running", "waiting_input", "retry_wait"})


def _now_iso() -> str:
    return datetime.now().astimezone().isoformat()


def build_workflow_id() -> str:
    return f"wf_{uuid.uuid4().hex[:8]}"


class WorkflowStore:
    def __init__(self, workspace: Path | None = None) -> None:
        root: Path = workspace or get_workspace_path()
        self._records_dir: Path = ensure_dir(root / "workflows" / "records")

    def create(
        self,
        *,
        source_kind: WorkflowSourceKind,
        goal: str,
        notify_target: NotifyTarget | dict[str, str] | None = None,
        metadata: dict[str, object] | None = None,
    ) -> WorkflowRecord:
        target: NotifyTarget
        if isinstance(notify_target, NotifyTarget):
            target = notify_target
        else:
            target = NotifyTarget.model_validate(notify_target or {})

        return WorkflowRecord(
            workflow_id=build_workflow_id(),
            source_kind=source_kind,
            goal=goal,
            notify_target=target,
            metadata=metadata or {},
        )

    def upsert(self, record: WorkflowRecord) -> Path:
        path: Path = self._path_for(record.workflow_id)
        updated: WorkflowRecord = record.model_copy(update={"updated_at": _now_iso()})
        self._write_json(path, updated)
        return path

    def upsert_and_get(self, record: WorkflowRecord) -> WorkflowRecord:
        updated: WorkflowRecord = record.model_copy(update={"updated_at": _now_iso()})
        self._write_json(self._path_for(updated.workflow_id), updated)
        return updated

    def get(self, workflow_id: str) -> WorkflowRecord | None:
        path: Path = self._path_for(workflow_id)
        if not path.exists():
            return None

        try:
            return WorkflowRecord.model_validate_json(path.read_text(encoding="utf-8"))
        except Exception as exc:
            logger.warning("WorkflowStore: failed to read workflow {}: {}", workflow_id, exc)
            return None

    def list_all(self) -> list[WorkflowRecord]:
        records: list[WorkflowRecord] = []
        for path in sorted(self._records_dir.glob("*.json")):
            try:
                records.append(WorkflowRecord.model_validate_json(path.read_text(encoding="utf-8")))
            except Exception as exc:
                logger.warning("WorkflowStore: skipping corrupt record {}: {}", path.name, exc)
        return records

    def list_incomplete(self) -> list[WorkflowRecord]:
        return [record for record in self.list_all() if record.state in INCOMPLETE_STATES]

    def _path_for(self, workflow_id: str) -> Path:
        return self._records_dir / f"{safe_filename(workflow_id)}.json"

    def _write_json(self, path: Path, record: WorkflowRecord) -> None:
        payload: dict[str, object] = record.model_dump(mode="json", by_alias=True)
        tmp_path: Path = path.with_name(f"{path.name}.tmp")
        with tmp_path.open("w", encoding="utf-8") as file:
            json.dump(payload, file, ensure_ascii=False, indent=2)
            _ = file.write("\n")
        _ = tmp_path.replace(path)
