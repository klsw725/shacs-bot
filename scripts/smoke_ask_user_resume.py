#!/usr/bin/env python3
"""
스모크 테스트: ask_user waiting 워크플로우가 다음 인바운드 메시지로 재개됩니다.

커버리지:
  1. planned workflow가 ask_user 스텝에서 waiting_input 상태로 전환됨
  2. resume_with_user_answer() 가 사용자 답변을 저장하고 queued로 재전환
  3. currentStepIndex 가 ask_user 다음 스텝으로 전진 (재질의 없음)

실행 방법:
    uv run python scripts/smoke_ask_user_resume.py

성공 시 "PASS" 라인만 출력하고 종료 코드 0.
실패 시 AssertionError 메시지를 출력하고 종료 코드 1.
"""
from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.workflow.runtime import WorkflowRuntime


def run_smoke() -> None:
    with tempfile.TemporaryDirectory() as tmpdir:
        runtime = WorkflowRuntime(workspace=Path(tmpdir))

        # 1. ask_user 스텝을 포함한 워크플로우 등록
        record = runtime.register_planned_workflow(
            goal="테스트 목표",
            plan={
                "kind": "planned_workflow",
                "steps": [
                    {"kind": "research", "description": "조사", "depends_on": []},
                    {"kind": "ask_user", "description": "사용자 입력 요청", "depends_on": [0]},
                    {"kind": "send_result", "description": "전달", "depends_on": [1]},
                ],
            },
            channel="test",
            chat_id="test-chat",
            session_key="test-session",
        )
        wf_id = record.workflow_id

        # 2. step 0 (research) 완료 → step 1 (ask_user) cursor 설정
        runtime.start(wf_id)
        runtime.update_step_cursor(wf_id, step_index=0, step_kind="research")
        runtime.annotate_step_result(wf_id, "조사 완료")
        runtime.update_step_cursor(wf_id, step_index=1, step_kind="ask_user")

        rec = runtime.store.get(wf_id)
        assert rec is not None
        assert rec.metadata.get("currentStepIndex") == 1, (
            f"[FAIL] ask_user 스텝 cursor는 1이어야 함. 실제: {rec.metadata.get('currentStepIndex')}"
        )
        print("PASS [검증 1] ask_user 스텝에 cursor 도달 (index=1)")

        # 3. ask_user 스텝 → waiting_input 전환 (loop._execute_plan_step 시뮬레이션)
        runtime.wait_for_input(wf_id)
        rec = runtime.store.get(wf_id)
        assert rec is not None and rec.state == "waiting_input", (
            f"[FAIL] waiting_input 상태여야 함. 실제: {rec.state if rec else 'None'}"
        )
        print("PASS [검증 2] 워크플로우 waiting_input 상태 진입")

        # 4. waiting_input 이 아닌 경우 resume_with_user_answer 는 None 반환
        bad = runtime.resume_with_user_answer("nonexistent-id", answer="x")
        assert bad is None, "[FAIL] 존재하지 않는 id에 대해 None이어야 함"
        print("PASS [검증 3] 잘못된 id 는 None 반환")

        # 5. 사용자 답변 소비 → resume_with_user_answer (loop._process_message 시뮬레이션)
        user_answer = "테스트 답변입니다"
        resumed = runtime.resume_with_user_answer(wf_id, answer=user_answer)
        assert resumed is not None, "[FAIL] resume_with_user_answer 가 None을 반환"
        print("PASS [검증 4] resume_with_user_answer 반환값 있음")

        # 6. 상태가 queued로 전환됐는지 확인
        rec = runtime.store.get(wf_id)
        assert rec is not None and rec.state == "queued", (
            f"[FAIL] queued 상태여야 함. 실제: {rec.state if rec else 'None'}"
        )
        print("PASS [검증 5] 워크플로우 queued 상태로 재전환 (재디스패치 가능)")

        # 7. currentStepIndex가 2로 전진됐는지 확인 (ask_user 스텝 건너뜀)
        step_idx = rec.metadata.get("currentStepIndex")
        assert step_idx == 2, (
            f"[FAIL] ask_user 이후 cursor는 2여야 함. 실제: {step_idx}"
        )
        print("PASS [검증 6] cursor가 ask_user 다음 스텝(2=send_result)으로 전진")

        # 8. 사용자 답변이 메타데이터에 저장됐는지 확인
        stored_answer = rec.metadata.get("userAnswer")
        assert stored_answer == user_answer, (
            f"[FAIL] userAnswer 가 저장됐어야 함. 실제: {stored_answer!r}"
        )
        print("PASS [검증 7] 사용자 답변이 userAnswer 메타데이터에 저장됨")

        last_step_result = rec.metadata.get("lastStepResultSummary")
        assert last_step_result == user_answer, (
            f"[FAIL] lastStepResultSummary 가 답변이어야 함. 실제: {last_step_result!r}"
        )
        print("PASS [검증 8] lastStepResultSummary 가 사용자 답변으로 설정됨")

        # 9. 재디스패치 후 ask_user 스텝을 다시 실행하지 않고 send_result 부터 시작 확인
        runtime2 = WorkflowRuntime(workspace=Path(tmpdir))
        redispatched = runtime2.store.get(wf_id)
        assert redispatched is not None
        resume_idx = redispatched.metadata.get("currentStepIndex", 0)
        if not isinstance(resume_idx, int) or resume_idx < 0:
            resume_idx = 0
        assert resume_idx == 2, (
            f"[FAIL] 재디스패치 후 send_result(index=2) 부터 시작해야 함. 실제: {resume_idx}"
        )
        print("PASS [검증 9] 재디스패치 후 ask_user 재질의 없이 send_result 부터 시작")

    print("\n모든 검증 통과 ✓")


if __name__ == "__main__":
    try:
        run_smoke()
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
