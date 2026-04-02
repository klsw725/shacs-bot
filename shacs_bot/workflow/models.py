from __future__ import annotations

from datetime import datetime
from typing import ClassVar, Literal

from pydantic import BaseModel, ConfigDict, Field
from pydantic.alias_generators import to_camel


WorkflowState = Literal[
    "queued",
    "running",
    "waiting_input",
    "retry_wait",
    "completed",
    "failed",
]

WorkflowSourceKind = Literal["heartbeat", "cron", "subagent", "manual"]


def _now_iso() -> str:
    return datetime.now().astimezone().isoformat()


class WorkflowBaseModel(BaseModel):
    model_config: ClassVar[ConfigDict] = ConfigDict(alias_generator=to_camel, populate_by_name=True)


class NotifyTarget(WorkflowBaseModel):
    channel: str = ""
    chat_id: str = ""
    session_key: str = ""


class WorkflowRecord(WorkflowBaseModel):
    workflow_id: str
    source_kind: WorkflowSourceKind
    state: WorkflowState = "queued"
    goal: str
    retries: int = 0
    next_run_at: str = ""
    notify_target: NotifyTarget = Field(default_factory=NotifyTarget)
    created_at: str = Field(default_factory=_now_iso)
    updated_at: str = Field(default_factory=_now_iso)
    last_error: str = ""
    metadata: dict[str, object] = Field(default_factory=dict)
