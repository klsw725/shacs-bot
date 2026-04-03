"""request_approval 승인/거절 게이트 테스트."""
from __future__ import annotations

from pathlib import Path

from shacs_bot.workflow.runtime import WorkflowRuntime


def _approval_plan() -> dict:
    return {
        "kind": "planned_workflow",
        "steps": [
            {"kind": "research", "description": "조사", "depends_on": []},
            {"kind": "request_approval", "description": "실행 승인 요청", "depends_on": [0]},
            {"kind": "send_result", "description": "전달", "depends_on": [1]},
        ],
    }


def _register_and_advance_to_approval(runtime: WorkflowRuntime) -> str:
    record = runtime.register_planned_workflow(
        goal="승인 테스트",
        plan=_approval_plan(),
        channel="test",
        chat_id="test-chat",
        session_key="test-session",
    )
    wf_id = record.workflow_id
    runtime.start(wf_id)
    runtime.update_step_cursor(wf_id, step_index=0, step_kind="research")
    runtime.annotate_step_result(wf_id, "조사 완료")
    runtime.update_step_cursor(wf_id, step_index=1, step_kind="request_approval")
    return wf_id


def test_approve_workflow_only_works_in_waiting_input_state(tmp_path: Path) -> None:
    r = WorkflowRuntime(workspace=tmp_path / "a")
    wf_id = _register_and_advance_to_approval(r)

    # 존재하지 않는 id → None
    assert r.approve_workflow("nonexistent-id") is None, (
        "존재하지 않는 id에 대해 None이어야 함"
    )

    # running 상태에서는 None 반환
    assert r.approve_workflow(wf_id) is None, (
        "running 상태에서 approve_workflow는 None이어야 함"
    )

    # waiting_input 진입
    r.wait_for_input(wf_id)
    rec = r.store.get(wf_id)
    assert rec is not None and rec.state == "waiting_input", (
        f"waiting_input 상태여야 함. 실제: {rec.state if rec else 'None'}"
    )

    # 승인 → queued + cursor=2 + approvalDecision=approved
    approved = r.approve_workflow(wf_id)
    assert approved is not None, "approve_workflow가 None 반환"
    rec = r.store.get(wf_id)
    assert rec is not None and rec.state == "queued", (
        f"승인 후 queued여야 함. 실제: {rec.state if rec else 'None'}"
    )
    assert rec.metadata.get("currentStepIndex") == 2, (
        f"승인 후 cursor는 2(send_result)여야 함. 실제: {rec.metadata.get('currentStepIndex')}"
    )
    assert rec.metadata.get("approvalDecision") == "approved", (
        f"approvalDecision이 'approved'여야 함. 실제: {rec.metadata.get('approvalDecision')!r}"
    )

    # 재디스패치 후 request_approval 재질의 없이 send_result 시작
    r2 = WorkflowRuntime(workspace=tmp_path / "a")
    redispatched = r2.store.get(wf_id)
    assert redispatched is not None
    resume_idx = redispatched.metadata.get("currentStepIndex", 0)
    assert isinstance(resume_idx, int) and resume_idx == 2, (
        f"재디스패치 후 send_result(index=2)부터 시작해야 함. 실제: {resume_idx}"
    )


def test_reject_workflow_transitions_to_failed(tmp_path: Path) -> None:
    r = WorkflowRuntime(workspace=tmp_path / "b")
    wf_id = _register_and_advance_to_approval(r)
    r.wait_for_input(wf_id)

    failed = r.fail(wf_id, last_error="사용자 거절")
    assert failed is not None, "fail()이 None 반환"
    rec = r.store.get(wf_id)
    assert rec is not None and rec.state == "failed", (
        f"거절 후 failed여야 함. 실제: {rec.state if rec else 'None'}"
    )
    assert rec.last_error == "사용자 거절", (
        f"last_error가 '사용자 거절'이어야 함. 실제: {rec.last_error!r}"
    )

    # failed 상태에서 approve_workflow → None (상태 보호)
    assert r.approve_workflow(wf_id) is None, (
        "failed 워크플로우에 approve_workflow → None이어야 함"
    )


def test_ask_user_resume_regression(tmp_path: Path) -> None:
    """ask_user의 resume_with_user_answer 경로가 request_approval 추가 후에도 동작함."""
    r = WorkflowRuntime(workspace=tmp_path / "c")
    ask_record = r.register_planned_workflow(
        goal="ask_user 회귀 테스트",
        plan={
            "kind": "planned_workflow",
            "steps": [
                {"kind": "ask_user", "description": "입력 요청", "depends_on": []},
                {"kind": "send_result", "description": "전달", "depends_on": [0]},
            ],
        },
        channel="test",
        chat_id="test-chat",
        session_key="test-session-ask",
    )
    ask_id = ask_record.workflow_id
    r.start(ask_id)
    r.update_step_cursor(ask_id, step_index=0, step_kind="ask_user")
    r.wait_for_input(ask_id)

    resumed = r.resume_with_user_answer(ask_id, answer="임의 답변")
    assert resumed is not None and resumed.state == "queued", (
        f"ask_user resume이 queued여야 함. 실제: {resumed.state if resumed else 'None'}"
    )
    stored = r.store.get(ask_id)
    assert stored is not None
    assert stored.metadata.get("currentStepIndex") == 1, (
        f"ask_user 후 cursor=1이어야 함. 실제: {stored.metadata.get('currentStepIndex')}"
    )
