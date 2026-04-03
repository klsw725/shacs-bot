"""ask_user waiting 워크플로우 재개 테스트."""
from __future__ import annotations

from pathlib import Path

from shacs_bot.workflow.runtime import WorkflowRuntime


def _ask_user_plan() -> dict:
    return {
        "kind": "planned_workflow",
        "steps": [
            {"kind": "research", "description": "조사", "depends_on": []},
            {"kind": "ask_user", "description": "사용자 입력 요청", "depends_on": [0]},
            {"kind": "send_result", "description": "전달", "depends_on": [1]},
        ],
    }


def test_ask_user_resume_full_flow(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    record = runtime.register_planned_workflow(
        goal="테스트 목표",
        plan=_ask_user_plan(),
        channel="test",
        chat_id="test-chat",
        session_key="test-session",
    )
    wf_id = record.workflow_id

    # step 0 완료 → step 1 (ask_user) cursor 설정
    runtime.start(wf_id)
    runtime.update_step_cursor(wf_id, step_index=0, step_kind="research")
    runtime.annotate_step_result(wf_id, "조사 완료")
    runtime.update_step_cursor(wf_id, step_index=1, step_kind="ask_user")

    rec = runtime.store.get(wf_id)
    assert rec is not None
    assert rec.metadata.get("currentStepIndex") == 1, (
        f"ask_user 스텝 cursor는 1이어야 함. 실제: {rec.metadata.get('currentStepIndex')}"
    )

    # ask_user 스텝 → waiting_input 전환
    runtime.wait_for_input(wf_id)
    rec = runtime.store.get(wf_id)
    assert rec is not None and rec.state == "waiting_input", (
        f"waiting_input 상태여야 함. 실제: {rec.state if rec else 'None'}"
    )

    # 존재하지 않는 id → None 반환
    bad = runtime.resume_with_user_answer("nonexistent-id", answer="x")
    assert bad is None, "존재하지 않는 id에 대해 None이어야 함"

    # 사용자 답변 소비 → resume
    user_answer = "테스트 답변입니다"
    resumed = runtime.resume_with_user_answer(wf_id, answer=user_answer)
    assert resumed is not None, "resume_with_user_answer 가 None을 반환"

    # 상태가 queued로 전환
    rec = runtime.store.get(wf_id)
    assert rec is not None and rec.state == "queued", (
        f"queued 상태여야 함. 실제: {rec.state if rec else 'None'}"
    )

    # cursor가 2로 전진 (ask_user 스텝 건너뜀)
    step_idx = rec.metadata.get("currentStepIndex")
    assert step_idx == 2, f"ask_user 이후 cursor는 2여야 함. 실제: {step_idx}"

    # 사용자 답변이 메타데이터에 저장
    assert rec.metadata.get("userAnswer") == user_answer, (
        f"userAnswer 가 저장됐어야 함. 실제: {rec.metadata.get('userAnswer')!r}"
    )
    assert rec.metadata.get("lastStepResultSummary") == user_answer, (
        f"lastStepResultSummary 가 답변이어야 함. 실제: {rec.metadata.get('lastStepResultSummary')!r}"
    )

    # 재디스패치 후 ask_user 재질의 없이 send_result(index=2) 시작
    runtime2 = WorkflowRuntime(workspace=tmp_path)
    redispatched = runtime2.store.get(wf_id)
    assert redispatched is not None
    resume_idx = redispatched.metadata.get("currentStepIndex", 0)
    if not isinstance(resume_idx, int) or resume_idx < 0:
        resume_idx = 0
    assert resume_idx == 2, (
        f"재디스패치 후 send_result(index=2) 부터 시작해야 함. 실제: {resume_idx}"
    )
