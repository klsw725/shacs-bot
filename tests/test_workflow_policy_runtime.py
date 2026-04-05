from __future__ import annotations

from pathlib import Path

from shacs_bot.agent.loop import AgentLoop
from shacs_bot.workflow.models import NotifyTarget
from shacs_bot.workflow.runtime import WorkflowRuntime


def test_register_planned_workflow_persists_actor_metadata(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    plan = AgentLoop._classify_request("먼저 전체 데이터를 수집하고 그 다음에 정리해서 보내줘")

    record = runtime.register_planned_workflow(
        goal="actor metadata 저장 테스트",
        plan=plan.model_dump(),
        channel="slack:deploy-room",
        chat_id="room-1",
        session_key="session-actor",
        actor_metadata={"senderId": "user-1", "userId": "user-1", "isDm": False},
    )

    assert record.metadata.get("actor") == {
        "senderId": "user-1",
        "userId": "user-1",
        "isDm": False,
    }


def test_manual_recover_rejects_other_chat_owner(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    record = runtime.store.create(
        source_kind="manual",
        goal="recover auth test",
        notify_target=NotifyTarget(
            channel="telegram",
            chat_id="chat-a",
            session_key="telegram:chat-a",
        ),
        metadata={},
    )
    _ = runtime.store.upsert(record.model_copy(update={"state": "running"}))

    result = runtime.manual_recover(
        record.workflow_id,
        channel="telegram",
        chat_id="chat-b",
        sender_id="user-b",
    )

    assert result.status == "unauthorized"


def test_manual_recover_allows_cli_bypass(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    record = runtime.store.create(
        source_kind="manual",
        goal="recover cli test",
        notify_target=NotifyTarget(
            channel="telegram",
            chat_id="chat-a",
            session_key="telegram:chat-a",
        ),
        metadata={},
    )
    _ = runtime.store.upsert(record.model_copy(update={"state": "running"}))

    result = runtime.manual_recover(
        record.workflow_id,
        channel="cli",
        chat_id="direct",
        sender_id="cli",
    )

    assert result.status == "recovered"
