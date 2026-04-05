from __future__ import annotations

from pathlib import Path

from shacs_bot.agent.loop import AgentLoop
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
