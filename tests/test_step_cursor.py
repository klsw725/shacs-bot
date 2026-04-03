"""step cursor 전진 테스트: 완료된 step이 재디스패치 후 재실행되지 않음."""
from __future__ import annotations

from pathlib import Path

from shacs_bot.workflow.runtime import WorkflowRuntime


def _three_step_plan() -> dict:
    return {
        "kind": "planned_workflow",
        "steps": [
            {"kind": "research", "description": "조사", "depends_on": []},
            {"kind": "summarize", "description": "요약", "depends_on": [0]},
            {"kind": "send_result", "description": "전달", "depends_on": [1]},
        ],
    }


def test_step_cursor_advances_and_persists(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    record = runtime.register_planned_workflow(
        goal="테스트 목표",
        plan=_three_step_plan(),
        channel="test",
        chat_id="test-chat",
        session_key="test-session",
    )
    wf_id = record.workflow_id

    runtime.start(wf_id)

    # step 0 실행 시작 → cursor = 0
    runtime.update_step_cursor(wf_id, step_index=0, step_kind="research")
    # step 0 성공 → cursor = 1
    runtime.annotate_step_result(wf_id, "조사 결과")
    runtime.annotate_result(wf_id, "조사 결과")
    runtime.update_step_cursor(wf_id, step_index=1, step_kind="summarize")

    refreshed = runtime.store.get(wf_id)
    assert refreshed is not None
    assert refreshed.metadata.get("currentStepIndex") == 1, (
        f"step 0 완료 후 cursor는 1이어야 함. 실제: {refreshed.metadata.get('currentStepIndex')}"
    )

    # 재디스패치 시뮬레이션: 런타임 재생성 후 cursor 읽기
    runtime2 = WorkflowRuntime(workspace=tmp_path)
    recovered = runtime2.store.get(wf_id)
    assert recovered is not None
    resume_idx = recovered.metadata.get("currentStepIndex", 0)
    if not isinstance(resume_idx, int) or resume_idx < 0:
        resume_idx = 0

    assert resume_idx == 1, (
        f"재디스패치 후 시작 index는 1이어야 함. 실제: {resume_idx}"
    )

    # step 1 성공 → cursor = 2
    runtime2.update_step_cursor(wf_id, step_index=2, step_kind="send_result")
    refreshed2 = runtime2.store.get(wf_id)
    assert refreshed2 is not None
    assert refreshed2.metadata.get("currentStepIndex") == 2

    # 두 번째 재디스패치: step 2부터 시작
    runtime3 = WorkflowRuntime(workspace=tmp_path)
    recovered2 = runtime3.store.get(wf_id)
    assert recovered2 is not None
    resume_idx2 = recovered2.metadata.get("currentStepIndex", 0)
    if not isinstance(resume_idx2, int) or resume_idx2 < 0:
        resume_idx2 = 0
    assert resume_idx2 == 2, (
        f"두 번째 재디스패치 후 시작 index는 2여야 함. 실제: {resume_idx2}"
    )
