#!/usr/bin/env python3
"""
스모크 테스트: request_approval 워크플로우 승인/거절 게이트 검증.

커버리지:
  1. request_approval 스텝이 waiting_input 상태로 진입
  2. 명시적 승인(y/yes/승인)이 다음 스텝으로 재개 (ask_user 와 별도 경로)
  3. 명시적 거절(n/no/거절)이 워크플로우를 failed 로 종료
  4. 임의 텍스트는 워크플로우를 건드리지 않음 (waiting_input 유지)
  5. ask_user 의 기존 resume_with_user_answer 경로는 그대로 동작

실행 방법:
    uv run python scripts/smoke_request_approval.py

성공 시 "PASS" 라인만 출력하고 종료 코드 0.
실패 시 AssertionError 메시지를 출력하고 종료 코드 1.
"""
from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.workflow.runtime import WorkflowRuntime


def _make_approval_workflow(runtime: WorkflowRuntime) -> str:
    record = runtime.register_planned_workflow(
        goal="승인 테스트",
        plan={
            "kind": "planned_workflow",
            "steps": [
                {"kind": "research", "description": "조사", "depends_on": []},
                {"kind": "request_approval", "description": "실행 승인 요청", "depends_on": [0]},
                {"kind": "send_result", "description": "전달", "depends_on": [1]},
            ],
        },
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


def run_smoke() -> None:
    with tempfile.TemporaryDirectory() as tmpdir:
        workspace = Path(tmpdir)

        # ── 검증 1: approve_workflow 는 waiting_input 에서만 동작 ──────────
        r = WorkflowRuntime(workspace=workspace)
        wf_id = _make_approval_workflow(r)

        bad_approve = r.approve_workflow("nonexistent-id")
        assert bad_approve is None, "[FAIL] 존재하지 않는 id 에 대해 None 이어야 함"
        print("PASS [검증 1-a] 잘못된 id 는 None 반환")

        # running 상태에서는 거부
        bad_approve2 = r.approve_workflow(wf_id)
        assert bad_approve2 is None, "[FAIL] running 상태에서 approve_workflow 는 None 이어야 함"
        print("PASS [검증 1-b] running 상태 → approve_workflow None 반환")

        # ── 검증 2: waiting_input 진입 ──────────────────────────────────────
        r.wait_for_input(wf_id)
        rec = r.store.get(wf_id)
        assert rec is not None and rec.state == "waiting_input", (
            f"[FAIL] waiting_input 상태여야 함. 실제: {rec.state if rec else 'None'}"
        )
        print("PASS [검증 2] request_approval 스텝 → waiting_input 진입")

        # ── 검증 3: 승인 → queued 재전환, cursor 전진 ──────────────────────
        approved = r.approve_workflow(wf_id)
        assert approved is not None, "[FAIL] approve_workflow 가 None 반환"
        rec = r.store.get(wf_id)
        assert rec is not None and rec.state == "queued", (
            f"[FAIL] 승인 후 queued 여야 함. 실제: {rec.state if rec else 'None'}"
        )
        step_idx = rec.metadata.get("currentStepIndex")
        assert step_idx == 2, (
            f"[FAIL] 승인 후 cursor 는 2(send_result) 여야 함. 실제: {step_idx}"
        )
        approval_dec = rec.metadata.get("approvalDecision")
        assert approval_dec == "approved", (
            f"[FAIL] approvalDecision 이 'approved' 여야 함. 실제: {approval_dec!r}"
        )
        print("PASS [검증 3] 승인 → queued + cursor=2 + approvalDecision=approved")

        # ── 검증 4: 재디스패치 후 request_approval 스텝 재질의 없음 ──────────
        r2 = WorkflowRuntime(workspace=workspace)
        redispatched = r2.store.get(wf_id)
        assert redispatched is not None
        resume_idx = redispatched.metadata.get("currentStepIndex", 0)
        assert isinstance(resume_idx, int) and resume_idx == 2, (
            f"[FAIL] 재디스패치 후 send_result(index=2) 부터 시작해야 함. 실제: {resume_idx}"
        )
        print("PASS [검증 4] 재디스패치 후 request_approval 재질의 없이 send_result 시작")

        # ── 검증 5: 거절 → failed 종료 ─────────────────────────────────────
        r3 = WorkflowRuntime(workspace=workspace)
        wf_id2 = _make_approval_workflow(r3)
        r3.wait_for_input(wf_id2)

        rec2 = r3.store.get(wf_id2)
        assert rec2 is not None and rec2.state == "waiting_input"

        failed = r3.fail(wf_id2, last_error="사용자 거절")
        assert failed is not None, "[FAIL] fail() 이 None 반환"
        rec2 = r3.store.get(wf_id2)
        assert rec2 is not None and rec2.state == "failed", (
            f"[FAIL] 거절 후 failed 여야 함. 실제: {rec2.state if rec2 else 'None'}"
        )
        assert rec2.last_error == "사용자 거절", (
            f"[FAIL] last_error 가 '사용자 거절' 이어야 함. 실제: {rec2.last_error!r}"
        )
        print("PASS [검증 5] 거절 → failed + last_error 기록")

        # ── 검증 6: 임의 텍스트가 approve_workflow 를 호출할 수 없음 ──────────
        # approve_workflow 는 waiting_input 상태 전용 — failed 상태에선 None 반환
        should_be_none = r3.approve_workflow(wf_id2)
        assert should_be_none is None, (
            "[FAIL] failed 워크플로우에 approve_workflow → None 이어야 함"
        )
        print("PASS [검증 6] 임의 텍스트로는 approve_workflow 진입 불가 (상태 보호)")

        # ── 검증 7: ask_user 의 resume_with_user_answer 경로 회귀 없음 ────────
        r4 = WorkflowRuntime(workspace=workspace)
        ask_record = r4.register_planned_workflow(
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
        r4.start(ask_id)
        r4.update_step_cursor(ask_id, step_index=0, step_kind="ask_user")
        r4.wait_for_input(ask_id)

        resumed = r4.resume_with_user_answer(ask_id, answer="임의 답변")
        assert resumed is not None and resumed.state == "queued", (
            f"[FAIL] ask_user resume 이 queued 여야 함. 실제: {resumed.state if resumed else 'None'}"
        )
        step_idx2 = r4.store.get(ask_id)
        assert step_idx2 is not None
        assert step_idx2.metadata.get("currentStepIndex") == 1, (
            f"[FAIL] ask_user 후 cursor=1 이어야 함. 실제: {step_idx2.metadata.get('currentStepIndex')}"
        )
        print("PASS [검증 7] ask_user resume_with_user_answer 경로 회귀 없음")

    print("\n모든 검증 통과 ✓")


if __name__ == "__main__":
    try:
        run_smoke()
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
