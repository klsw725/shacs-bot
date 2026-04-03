#!/usr/bin/env python3
"""
E2E 스모크 테스트: planner → workflow 등록 → redispatch → executor 상태 시뮬레이션

기존 단위 스모크(smoke_planner_metadata, smoke_wait_until, smoke_ask_user_resume,
smoke_request_approval)가 커버하지 않는 통합 경로를 검증합니다.

커버리지:
  검증 1-3:  _classify_request() 출력 → register_planned_workflow() 저장 왕복
             (wait_until / ask_user / request_approval 각각 플랜 키 보존 확인)
  검증 4:    WorkflowRedispatcher._tick()이 queued manual 워크플로우를 stub 루프에 dispatch
  검증 5-9:  wait_until 전체 흐름
             (등록 → running → retry_wait → 만료 복구(queued) → completed)
  검증 10-14: ask_user 전체 흐름
             (등록 → running → waiting_input → reply 소비 → queued → completed)
  검증 15-21: request_approval 전체 흐름
             (등록 → running → waiting_input → 승인 → completed;
              거절 → failed; running 상태에서 approve 불가)

실행 방법:
    uv run python scripts/smoke_e2e_planner_to_workflow.py

성공 시 "PASS" 라인만 출력하고 종료 코드 0.
실패 시 AssertionError 메시지를 출력하고 종료 코드 1.
"""
from __future__ import annotations

import asyncio
import sys
import tempfile
from datetime import datetime, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.agent.loop import AgentLoop
from shacs_bot.workflow.redispatcher import WorkflowRedispatcher
from shacs_bot.workflow.runtime import WorkflowRuntime


# ---------------------------------------------------------------------------
# 최소 스텁
# ---------------------------------------------------------------------------


class _StubCronService:
    """WorkflowRedispatcher 생성자를 만족시키는 최소 스텁 (manual 워크플로우 테스트에서는 호출 안 됨)."""


class _StubAgentLoop:
    """WorkflowRedispatcher가 manual 워크플로우를 dispatch할 때 호출하는 최소 스텁."""

    def __init__(self) -> None:
        self.dispatched: list[str] = []

    async def execute_existing_workflow(self, workflow_id: str) -> bool:
        self.dispatched.append(workflow_id)
        return True


# ---------------------------------------------------------------------------
# 헬퍼
# ---------------------------------------------------------------------------


def _classify(text: str):
    return AgentLoop._classify_request(text)


# ---------------------------------------------------------------------------
# 스모크 본체
# ---------------------------------------------------------------------------


def run_smoke() -> None:
    with tempfile.TemporaryDirectory() as tmpdir:
        base = Path(tmpdir)

        # ==================================================================
        # 검증 1-3: _classify_request() → register_planned_workflow() 왕복
        # ==================================================================

        rt_reg = WorkflowRuntime(workspace=base / "reg")

        # --- 검증 1: wait_until 플랜 저장 왕복 ---
        wu_plan = _classify("30분 후에 날씨 조사해서 알려줘")
        assert wu_plan.kind == "planned_workflow", f"[FAIL] kind={wu_plan.kind!r}"
        wu_kinds = [s.kind for s in wu_plan.steps]
        assert "wait_until" in wu_kinds, f"[FAIL] wait_until 스텝 없음. steps={wu_kinds}"

        wu_rec = rt_reg.register_planned_workflow(
            goal="30분 후에 날씨 조사해서 알려줘",
            plan=wu_plan.model_dump(),
            channel="test",
            chat_id="chat-wu",
            session_key="session-wu",
        )
        assert wu_rec.state == "queued", f"[FAIL] 등록 직후 state=queued여야 함. got={wu_rec.state!r}"

        stored = wu_rec.metadata.get("plan", {})
        assert isinstance(stored, dict), "[FAIL] metadata.plan이 dict여야 함"
        stored_wu_step = next(
            (s for s in stored.get("steps", []) if s.get("kind") == "wait_until"), None
        )
        assert stored_wu_step is not None, "[FAIL] 저장된 플랜에 wait_until 스텝 없음"
        iso_time = stored_wu_step.get("step_meta", {}).get("iso_time")
        assert isinstance(iso_time, str) and iso_time, (
            f"[FAIL] 저장된 wait_until step_meta.iso_time 없음. step={stored_wu_step!r}"
        )
        parsed_dt = datetime.fromisoformat(iso_time)
        if parsed_dt.tzinfo is None:
            parsed_dt = parsed_dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
        assert parsed_dt > datetime.now().astimezone(), "[FAIL] iso_time이 미래여야 함"
        print("PASS [검증 1] wait_until planner 출력 → register_planned_workflow → iso_time 보존")

        # --- 검증 2: ask_user 플랜 저장 왕복 ---
        au_plan = _classify("사용자에게 선택을 물어보고 그에 맞게 조사해줘")
        assert au_plan.kind == "planned_workflow", f"[FAIL] kind={au_plan.kind!r}"
        assert any(s.kind == "ask_user" for s in au_plan.steps), "[FAIL] ask_user 스텝 없음"

        au_rec = rt_reg.register_planned_workflow(
            goal="사용자에게 선택을 물어보고 그에 맞게 조사해줘",
            plan=au_plan.model_dump(),
            channel="test",
            chat_id="chat-au",
            session_key="session-au",
        )
        stored_au = au_rec.metadata.get("plan", {})
        stored_au_step = next(
            (s for s in stored_au.get("steps", []) if s.get("kind") == "ask_user"), None
        )
        assert stored_au_step is not None, "[FAIL] 저장된 플랜에 ask_user 스텝 없음"
        assert isinstance(stored_au_step.get("step_meta", {}).get("prompt"), str), (
            f"[FAIL] ask_user step_meta.prompt 없음. step={stored_au_step!r}"
        )
        print("PASS [검증 2] ask_user planner 출력 → register_planned_workflow → prompt 보존")

        # --- 검증 3: request_approval 플랜 저장 왕복 ---
        ra_plan = _classify("파일을 삭제하기 전에 확인 후에 진행해줘")
        assert ra_plan.kind == "planned_workflow", f"[FAIL] kind={ra_plan.kind!r}"
        assert any(s.kind == "request_approval" for s in ra_plan.steps), (
            "[FAIL] request_approval 스텝 없음"
        )

        ra_rec = rt_reg.register_planned_workflow(
            goal="파일을 삭제하기 전에 확인 후에 진행해줘",
            plan=ra_plan.model_dump(),
            channel="test",
            chat_id="chat-ra",
            session_key="session-ra",
        )
        stored_ra = ra_rec.metadata.get("plan", {})
        stored_ra_step = next(
            (s for s in stored_ra.get("steps", []) if s.get("kind") == "request_approval"), None
        )
        assert stored_ra_step is not None, "[FAIL] 저장된 플랜에 request_approval 스텝 없음"
        assert isinstance(stored_ra_step.get("step_meta", {}).get("prompt"), str), (
            f"[FAIL] request_approval step_meta.prompt 없음. step={stored_ra_step!r}"
        )
        print("PASS [검증 3] request_approval planner 출력 → register_planned_workflow → prompt 보존")

        # ==================================================================
        # 검증 4: WorkflowRedispatcher._tick() → queued manual 워크플로우 dispatch
        # ==================================================================

        rt_rd = WorkflowRuntime(workspace=base / "rd")
        stub_loop = _StubAgentLoop()

        dispatch_rec = rt_rd.register_planned_workflow(
            goal="redispatcher dispatch 테스트",
            plan={"kind": "planned_workflow", "steps": []},
            channel="test",
            chat_id="chat-dispatch",
            session_key="session-dispatch",
        )
        dispatch_wf_id = dispatch_rec.workflow_id
        assert dispatch_rec.state == "queued"

        redispatcher = WorkflowRedispatcher(
            workflow_runtime=rt_rd,
            cron_service=_StubCronService(),
            agent_loop=stub_loop,
            poll_interval_s=9999,
        )
        asyncio.run(redispatcher._tick())

        assert dispatch_wf_id in stub_loop.dispatched, (
            f"[FAIL] _tick()이 {dispatch_wf_id}를 dispatch하지 않음. "
            f"dispatched={stub_loop.dispatched}"
        )
        print("PASS [검증 4] WorkflowRedispatcher._tick() → queued manual 워크플로우 dispatch")

        # ==================================================================
        # 검증 5-9: wait_until 전체 흐름
        # ==================================================================

        rt_wu = WorkflowRuntime(workspace=base / "wu")

        # 검증 5: _classify_request → register → queued
        wf_wu = rt_wu.register_planned_workflow(
            goal="30분 후에 날씨 조사해서 알려줘",
            plan=wu_plan.model_dump(),
            channel="test",
            chat_id="chat-wu-e2e",
            session_key="session-wu-e2e",
        )
        wf_wu_id = wf_wu.workflow_id
        assert wf_wu.state == "queued"
        print("PASS [검증 5] wait_until 워크플로우 등록 → state=queued")

        # 검증 6: start → running
        started = rt_wu.start(wf_wu_id)
        assert started is not None and started.state == "running", (
            f"[FAIL] start() 후 state=running이어야 함. got={started!r}"
        )
        rt_wu.update_step_cursor(wf_wu_id, step_index=0, step_kind="wait_until")
        print("PASS [검증 6] start() → state=running")

        # 검증 7: _execute_plan_step(wait_until) 시뮬레이션 → retry_wait + next_run_at
        wu_step = next(s for s in wu_plan.steps if s.kind == "wait_until")
        iso_from_meta = wu_step.step_meta.get("iso_time")
        assert isinstance(iso_from_meta, str)
        scheduled = rt_wu.schedule_retry(
            wf_wu_id,
            next_run_at=iso_from_meta,
            last_error="wait_until 대기 중",
            increment_retries=False,
        )
        assert scheduled is not None and scheduled.state == "retry_wait", (
            f"[FAIL] schedule_retry 후 state=retry_wait여야 함. got={scheduled!r}"
        )
        assert scheduled.next_run_at == iso_from_meta, (
            f"[FAIL] next_run_at 불일치. expected={iso_from_meta!r}, got={scheduled.next_run_at!r}"
        )
        print("PASS [검증 7] _execute_plan_step(wait_until) 시뮬레이션 → retry_wait + next_run_at 저장")

        # 검증 8: next_run_at 만료 시뮬레이션 → recover_restart → queued 복구
        past_dt = (datetime.now().astimezone() - timedelta(minutes=1)).isoformat()
        rt_wu.store.upsert_and_get(scheduled.model_copy(update={"next_run_at": past_dt}))
        recovered_list = rt_wu.recover_restart()
        recovered_ids = {r.workflow_id for r in recovered_list}
        assert wf_wu_id in recovered_ids, (
            f"[FAIL] recover_restart가 {wf_wu_id}를 복구하지 않음. recovered={recovered_ids}"
        )
        rec_after_recover = rt_wu.store.get(wf_wu_id)
        assert rec_after_recover is not None and rec_after_recover.state == "queued", (
            f"[FAIL] 복구 후 state=queued여야 함. got={rec_after_recover!r}"
        )
        print("PASS [검증 8] next_run_at 만료 → recover_restart → state=queued 복구")

        # 검증 9: 재실행 → 나머지 스텝 완료 → completed
        rt_wu.start(wf_wu_id)
        rt_wu.update_step_cursor(wf_wu_id, step_index=1, step_kind="research")
        rt_wu.annotate_step_result(wf_wu_id, "날씨 조사 완료")
        rt_wu.update_step_cursor(wf_wu_id, step_index=2, step_kind="send_result")
        rt_wu.annotate_step_result(wf_wu_id, "날씨 결과 전달 완료")
        rt_wu.complete(wf_wu_id)
        final_wu = rt_wu.store.get(wf_wu_id)
        assert final_wu is not None and final_wu.state == "completed", (
            f"[FAIL] 최종 state=completed여야 함. got={final_wu!r}"
        )
        print("PASS [검증 9] wait_until 흐름 전체 완료 → state=completed")

        # ==================================================================
        # 검증 10-14: ask_user 전체 흐름
        # ==================================================================

        rt_au = WorkflowRuntime(workspace=base / "au")

        # 검증 10: 등록 → queued
        wf_au = rt_au.register_planned_workflow(
            goal="사용자 입력 요청 테스트",
            plan=au_plan.model_dump(),
            channel="test",
            chat_id="chat-au-e2e",
            session_key="session-au-e2e",
        )
        wf_au_id = wf_au.workflow_id
        assert wf_au.state == "queued"
        print("PASS [검증 10] ask_user 워크플로우 등록 → state=queued")

        # 검증 11: start → running + cursor 설정
        rt_au.start(wf_au_id)
        au_step_idx = next(i for i, s in enumerate(au_plan.steps) if s.kind == "ask_user")
        rt_au.update_step_cursor(wf_au_id, step_index=au_step_idx, step_kind="ask_user")
        rec_au = rt_au.store.get(wf_au_id)
        assert rec_au is not None and rec_au.metadata.get("currentStepIndex") == au_step_idx, (
            f"[FAIL] cursor가 {au_step_idx}여야 함. got={rec_au.metadata.get('currentStepIndex')!r}"
        )
        print(f"PASS [검증 11] start() + cursor → currentStepIndex={au_step_idx} (ask_user)")

        # 검증 12: _execute_plan_step(ask_user) 시뮬레이션 → waiting_input
        rt_au.wait_for_input(wf_au_id)
        rec_au = rt_au.store.get(wf_au_id)
        assert rec_au is not None and rec_au.state == "waiting_input", (
            f"[FAIL] wait_for_input 후 state=waiting_input여야 함. got={rec_au.state!r}"
        )
        print("PASS [검증 12] _execute_plan_step(ask_user) 시뮬레이션 → state=waiting_input")

        # 검증 13: 사용자 답변 소비 → queued + cursor 전진 + 답변 저장
        user_answer = "선택 A를 원합니다"
        resumed_au = rt_au.resume_with_user_answer(wf_au_id, answer=user_answer)
        assert resumed_au is not None and resumed_au.state == "queued", (
            f"[FAIL] resume 후 state=queued여야 함. got={resumed_au!r}"
        )
        rec_au = rt_au.store.get(wf_au_id)
        assert rec_au is not None
        expected_cursor_au = au_step_idx + 1
        actual_cursor_au = rec_au.metadata.get("currentStepIndex")
        assert actual_cursor_au == expected_cursor_au, (
            f"[FAIL] cursor가 {expected_cursor_au}여야 함. got={actual_cursor_au!r}"
        )
        assert rec_au.metadata.get("userAnswer") == user_answer, (
            f"[FAIL] userAnswer 저장 오류. got={rec_au.metadata.get('userAnswer')!r}"
        )
        print("PASS [검증 13] resume_with_user_answer → queued + cursor 전진 + userAnswer 저장")

        # 검증 14: 재실행 → 이후 스텝 완료 → completed
        rt_au.start(wf_au_id)
        rt_au.update_step_cursor(wf_au_id, step_index=expected_cursor_au, step_kind="research")
        rt_au.annotate_step_result(wf_au_id, "조사 완료")
        rt_au.update_step_cursor(wf_au_id, step_index=expected_cursor_au + 1, step_kind="send_result")
        rt_au.complete(wf_au_id)
        final_au = rt_au.store.get(wf_au_id)
        assert final_au is not None and final_au.state == "completed", (
            f"[FAIL] 최종 state=completed여야 함. got={final_au!r}"
        )
        print("PASS [검증 14] ask_user 흐름 전체 완료 → state=completed")

        # ==================================================================
        # 검증 15-21: request_approval 전체 흐름
        # ==================================================================

        rt_ra = WorkflowRuntime(workspace=base / "ra")

        # 검증 15: 등록 → queued
        wf_ra = rt_ra.register_planned_workflow(
            goal="파일 삭제 승인 요청 테스트",
            plan=ra_plan.model_dump(),
            channel="test",
            chat_id="chat-ra-e2e",
            session_key="session-ra-e2e",
        )
        wf_ra_id = wf_ra.workflow_id
        assert wf_ra.state == "queued"
        print("PASS [검증 15] request_approval 워크플로우 등록 → state=queued")

        # 검증 16: start → running + cursor 설정
        rt_ra.start(wf_ra_id)
        ra_step_idx = next(i for i, s in enumerate(ra_plan.steps) if s.kind == "request_approval")
        rt_ra.update_step_cursor(wf_ra_id, step_index=ra_step_idx, step_kind="request_approval")
        print(f"PASS [검증 16] start() + cursor → currentStepIndex={ra_step_idx} (request_approval)")

        # 검증 17: _execute_plan_step(request_approval) 시뮬레이션 → waiting_input
        rt_ra.wait_for_input(wf_ra_id)
        rec_ra = rt_ra.store.get(wf_ra_id)
        assert rec_ra is not None and rec_ra.state == "waiting_input", (
            f"[FAIL] wait_for_input 후 state=waiting_input여야 함. got={rec_ra.state!r}"
        )
        print("PASS [검증 17] _execute_plan_step(request_approval) 시뮬레이션 → state=waiting_input")

        # 검증 18: running 상태에서 approve_workflow → None (임의 텍스트 보호)
        rt_ra_arb = WorkflowRuntime(workspace=base / "ra_arb")
        arb_rec = rt_ra_arb.register_planned_workflow(
            goal="임의 텍스트 보호 테스트",
            plan=ra_plan.model_dump(),
            channel="test",
            chat_id="chat-ra-arb",
            session_key="session-ra-arb",
        )
        arb_id = arb_rec.workflow_id
        rt_ra_arb.start(arb_id)  # queued → running (waiting_input 아님)
        arb_result = rt_ra_arb.approve_workflow(arb_id)
        assert arb_result is None, (
            f"[FAIL] running 상태에서 approve_workflow → None이어야 함. got={arb_result!r}"
        )
        print("PASS [검증 18] running 상태에서 approve_workflow 호출 → None (상태 보호)")

        # 검증 19: 승인 → queued + cursor 전진 + approvalDecision=approved
        approved = rt_ra.approve_workflow(wf_ra_id)
        assert approved is not None, "[FAIL] approve_workflow가 None 반환"
        rec_ra = rt_ra.store.get(wf_ra_id)
        assert rec_ra is not None and rec_ra.state == "queued", (
            f"[FAIL] 승인 후 state=queued여야 함. got={rec_ra.state!r}"
        )
        expected_cursor_ra = ra_step_idx + 1
        actual_cursor_ra = rec_ra.metadata.get("currentStepIndex")
        assert actual_cursor_ra == expected_cursor_ra, (
            f"[FAIL] cursor가 {expected_cursor_ra}여야 함. got={actual_cursor_ra!r}"
        )
        assert rec_ra.metadata.get("approvalDecision") == "approved", (
            f"[FAIL] approvalDecision=approved여야 함. got={rec_ra.metadata.get('approvalDecision')!r}"
        )
        print("PASS [검증 19] approve_workflow → queued + cursor 전진 + approvalDecision=approved")

        # 검증 20: 승인 후 재실행 → 이후 스텝 완료 → completed
        rt_ra.start(wf_ra_id)
        rt_ra.update_step_cursor(wf_ra_id, step_index=expected_cursor_ra, step_kind="send_result")
        rt_ra.complete(wf_ra_id)
        final_ra = rt_ra.store.get(wf_ra_id)
        assert final_ra is not None and final_ra.state == "completed", (
            f"[FAIL] 최종 state=completed여야 함. got={final_ra!r}"
        )
        print("PASS [검증 20] request_approval 승인 흐름 완료 → state=completed")

        # 검증 21: 거절 → failed + last_error 기록
        rt_ra_deny = WorkflowRuntime(workspace=base / "ra_deny")
        deny_rec = rt_ra_deny.register_planned_workflow(
            goal="거절 테스트",
            plan=ra_plan.model_dump(),
            channel="test",
            chat_id="chat-ra-deny",
            session_key="session-ra-deny",
        )
        deny_id = deny_rec.workflow_id
        rt_ra_deny.start(deny_id)
        rt_ra_deny.update_step_cursor(deny_id, step_index=ra_step_idx, step_kind="request_approval")
        rt_ra_deny.wait_for_input(deny_id)
        denied = rt_ra_deny.fail(deny_id, last_error="사용자 거절")
        assert denied is not None and denied.state == "failed", (
            f"[FAIL] 거절 후 state=failed여야 함. got={denied!r}"
        )
        assert denied.last_error == "사용자 거절", (
            f"[FAIL] last_error='사용자 거절'이어야 함. got={denied.last_error!r}"
        )
        # failed 상태에서 approve_workflow → None (임의 텍스트로 failed 재개 불가)
        post_fail_approve = rt_ra_deny.approve_workflow(deny_id)
        assert post_fail_approve is None, (
            f"[FAIL] failed 상태에서 approve_workflow → None이어야 함. got={post_fail_approve!r}"
        )
        print("PASS [검증 21] 거절 → state=failed + last_error 기록 + failed 후 approve 불가")

    print("\n모든 E2E 검증 통과 ✓")


if __name__ == "__main__":
    try:
        run_smoke()
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
