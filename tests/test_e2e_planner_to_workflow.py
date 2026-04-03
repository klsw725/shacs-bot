"""E2E: planner → workflow 등록 → redispatch → executor 상태 시뮬레이션."""
from __future__ import annotations

from datetime import datetime, timedelta
from pathlib import Path

from shacs_bot.agent.loop import AgentLoop
from shacs_bot.workflow.redispatcher import WorkflowRedispatcher
from shacs_bot.workflow.runtime import WorkflowRuntime


# ──────────────────────────────────────────────
# 스텁
# ──────────────────────────────────────────────


class _StubCronService:
    pass


class _StubAgentLoop:
    def __init__(self) -> None:
        self.dispatched: list[str] = []

    async def execute_existing_workflow(self, workflow_id: str) -> bool:
        self.dispatched.append(workflow_id)
        return True


# ──────────────────────────────────────────────
# 검증 1-3: _classify_request → register_planned_workflow 왕복
# ──────────────────────────────────────────────


def test_e2e_register_wait_until_preserves_iso_time(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    wu_plan = AgentLoop._classify_request("30분 후에 날씨 조사해서 알려줘")
    assert wu_plan.kind == "planned_workflow"
    assert any(s.kind == "wait_until" for s in wu_plan.steps)

    wu_rec = rt.register_planned_workflow(
        goal="30분 후에 날씨 조사해서 알려줘",
        plan=wu_plan.model_dump(),
        channel="test",
        chat_id="chat-wu",
        session_key="session-wu",
    )
    assert wu_rec.state == "queued"

    stored = wu_rec.metadata.get("plan", {})
    stored_wu = next((s for s in stored.get("steps", []) if s.get("kind") == "wait_until"), None)
    assert stored_wu is not None, "저장된 플랜에 wait_until 스텝 없음"
    iso_time = stored_wu.get("step_meta", {}).get("iso_time")
    assert isinstance(iso_time, str) and iso_time, (
        f"저장된 wait_until step_meta.iso_time 없음. step={stored_wu!r}"
    )
    parsed_dt = datetime.fromisoformat(iso_time)
    if parsed_dt.tzinfo is None:
        parsed_dt = parsed_dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
    assert parsed_dt > datetime.now().astimezone(), "iso_time이 미래여야 함"


def test_e2e_register_ask_user_preserves_prompt(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    au_plan = AgentLoop._classify_request("사용자에게 선택을 물어보고 그에 맞게 조사해줘")
    assert au_plan.kind == "planned_workflow"
    assert any(s.kind == "ask_user" for s in au_plan.steps)

    au_rec = rt.register_planned_workflow(
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
    assert stored_au_step is not None, "저장된 플랜에 ask_user 스텝 없음"
    assert isinstance(stored_au_step.get("step_meta", {}).get("prompt"), str), (
        f"ask_user step_meta.prompt 없음. step={stored_au_step!r}"
    )


def test_e2e_register_request_approval_preserves_prompt(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    ra_plan = AgentLoop._classify_request("파일을 삭제하기 전에 확인 후에 진행해줘")
    assert ra_plan.kind == "planned_workflow"
    assert any(s.kind == "request_approval" for s in ra_plan.steps)

    ra_rec = rt.register_planned_workflow(
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
    assert stored_ra_step is not None, "저장된 플랜에 request_approval 스텝 없음"
    assert isinstance(stored_ra_step.get("step_meta", {}).get("prompt"), str), (
        f"request_approval step_meta.prompt 없음. step={stored_ra_step!r}"
    )


# ──────────────────────────────────────────────
# 검증 4: WorkflowRedispatcher._tick() → queued 워크플로우 dispatch
# ──────────────────────────────────────────────


async def test_e2e_redispatcher_tick_dispatches_queued(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    stub_loop = _StubAgentLoop()

    record = rt.register_planned_workflow(
        goal="redispatcher 테스트",
        plan={"kind": "planned_workflow", "steps": []},
        channel="test",
        chat_id="chat-dispatch",
        session_key="session-dispatch",
    )
    wf_id = record.workflow_id
    assert record.state == "queued"

    redispatcher = WorkflowRedispatcher(
        workflow_runtime=rt,
        cron_service=_StubCronService(),
        agent_loop=stub_loop,
        poll_interval_s=9999,
    )
    await redispatcher._tick()

    assert wf_id in stub_loop.dispatched, (
        f"_tick()이 {wf_id}를 dispatch하지 않음. dispatched={stub_loop.dispatched}"
    )


# ──────────────────────────────────────────────
# 검증 5-9: wait_until 전체 흐름
# ──────────────────────────────────────────────


def test_e2e_wait_until_full_flow(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    wu_plan = AgentLoop._classify_request("30분 후에 날씨 조사해서 알려줘")

    # 등록 → queued
    wf = rt.register_planned_workflow(
        goal="30분 후에 날씨 조사해서 알려줘",
        plan=wu_plan.model_dump(),
        channel="test",
        chat_id="chat-wu-e2e",
        session_key="session-wu-e2e",
    )
    wf_id = wf.workflow_id
    assert wf.state == "queued"

    # start → running
    started = rt.start(wf_id)
    assert started is not None and started.state == "running"
    rt.update_step_cursor(wf_id, step_index=0, step_kind="wait_until")

    # wait_until 실행 시뮬레이션 → retry_wait
    wu_step = next(s for s in wu_plan.steps if s.kind == "wait_until")
    iso_from_meta = wu_step.step_meta.get("iso_time")
    assert isinstance(iso_from_meta, str)
    scheduled = rt.schedule_retry(
        wf_id,
        next_run_at=iso_from_meta,
        last_error="wait_until 대기 중",
        increment_retries=False,
    )
    assert scheduled is not None and scheduled.state == "retry_wait"
    assert scheduled.next_run_at == iso_from_meta

    # next_run_at 만료 시뮬레이션 → recover_restart → queued
    past_dt = (datetime.now().astimezone() - timedelta(minutes=1)).isoformat()
    rt.store.upsert_and_get(scheduled.model_copy(update={"next_run_at": past_dt}))
    recovered_list = rt.recover_restart()
    assert wf_id in {r.workflow_id for r in recovered_list}
    rec = rt.store.get(wf_id)
    assert rec is not None and rec.state == "queued"

    # 재실행 → completed
    rt.start(wf_id)
    rt.update_step_cursor(wf_id, step_index=1, step_kind="research")
    rt.annotate_step_result(wf_id, "날씨 조사 완료")
    rt.update_step_cursor(wf_id, step_index=2, step_kind="send_result")
    rt.annotate_step_result(wf_id, "날씨 결과 전달 완료")
    rt.complete(wf_id)
    final = rt.store.get(wf_id)
    assert final is not None and final.state == "completed"


# ──────────────────────────────────────────────
# 검증 10-14: ask_user 전체 흐름
# ──────────────────────────────────────────────


def test_e2e_ask_user_full_flow(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    au_plan = AgentLoop._classify_request("사용자에게 선택을 물어보고 그에 맞게 조사해줘")

    # 등록 → queued
    wf = rt.register_planned_workflow(
        goal="사용자 입력 요청 테스트",
        plan=au_plan.model_dump(),
        channel="test",
        chat_id="chat-au-e2e",
        session_key="session-au-e2e",
    )
    wf_id = wf.workflow_id
    assert wf.state == "queued"

    # start + cursor
    rt.start(wf_id)
    au_step_idx = next(i for i, s in enumerate(au_plan.steps) if s.kind == "ask_user")
    rt.update_step_cursor(wf_id, step_index=au_step_idx, step_kind="ask_user")
    rec = rt.store.get(wf_id)
    assert rec is not None and rec.metadata.get("currentStepIndex") == au_step_idx

    # waiting_input 전환
    rt.wait_for_input(wf_id)
    rec = rt.store.get(wf_id)
    assert rec is not None and rec.state == "waiting_input"

    # 사용자 답변 소비 → queued + cursor 전진 + 답변 저장
    user_answer = "선택 A를 원합니다"
    resumed = rt.resume_with_user_answer(wf_id, answer=user_answer)
    assert resumed is not None and resumed.state == "queued"
    rec = rt.store.get(wf_id)
    assert rec is not None
    expected_cursor = au_step_idx + 1
    assert rec.metadata.get("currentStepIndex") == expected_cursor
    assert rec.metadata.get("userAnswer") == user_answer

    # 재실행 → completed
    rt.start(wf_id)
    rt.update_step_cursor(wf_id, step_index=expected_cursor, step_kind="research")
    rt.annotate_step_result(wf_id, "조사 완료")
    rt.update_step_cursor(wf_id, step_index=expected_cursor + 1, step_kind="send_result")
    rt.complete(wf_id)
    final = rt.store.get(wf_id)
    assert final is not None and final.state == "completed"


# ──────────────────────────────────────────────
# 검증 15-21: request_approval 전체 흐름
# ──────────────────────────────────────────────


def test_e2e_request_approval_full_flow(tmp_path: Path) -> None:
    ra_plan = AgentLoop._classify_request("파일을 삭제하기 전에 확인 후에 진행해줘")
    ra_step_idx = next(i for i, s in enumerate(ra_plan.steps) if s.kind == "request_approval")

    # 등록 → queued
    rt = WorkflowRuntime(workspace=tmp_path / "ra")
    wf = rt.register_planned_workflow(
        goal="파일 삭제 승인 요청 테스트",
        plan=ra_plan.model_dump(),
        channel="test",
        chat_id="chat-ra-e2e",
        session_key="session-ra-e2e",
    )
    wf_id = wf.workflow_id
    assert wf.state == "queued"

    # start + cursor + waiting_input
    rt.start(wf_id)
    rt.update_step_cursor(wf_id, step_index=ra_step_idx, step_kind="request_approval")
    rt.wait_for_input(wf_id)
    rec = rt.store.get(wf_id)
    assert rec is not None and rec.state == "waiting_input"

    # running 상태에서 approve_workflow → None (상태 보호)
    rt_arb = WorkflowRuntime(workspace=tmp_path / "arb")
    arb_rec = rt_arb.register_planned_workflow(
        goal="임의 텍스트 보호 테스트",
        plan=ra_plan.model_dump(),
        channel="test",
        chat_id="chat-arb",
        session_key="session-arb",
    )
    arb_id = arb_rec.workflow_id
    rt_arb.start(arb_id)
    assert rt_arb.approve_workflow(arb_id) is None, (
        "running 상태에서 approve_workflow → None이어야 함"
    )

    # 승인 → queued + cursor 전진 + approvalDecision=approved
    approved = rt.approve_workflow(wf_id)
    assert approved is not None
    rec = rt.store.get(wf_id)
    assert rec is not None and rec.state == "queued"
    expected_cursor = ra_step_idx + 1
    assert rec.metadata.get("currentStepIndex") == expected_cursor
    assert rec.metadata.get("approvalDecision") == "approved"

    # 승인 후 재실행 → completed
    rt.start(wf_id)
    rt.update_step_cursor(wf_id, step_index=expected_cursor, step_kind="send_result")
    rt.complete(wf_id)
    final = rt.store.get(wf_id)
    assert final is not None and final.state == "completed"


def test_e2e_request_approval_reject_flow(tmp_path: Path) -> None:
    ra_plan = AgentLoop._classify_request("파일을 삭제하기 전에 확인 후에 진행해줘")
    ra_step_idx = next(i for i, s in enumerate(ra_plan.steps) if s.kind == "request_approval")

    rt = WorkflowRuntime(workspace=tmp_path)
    wf = rt.register_planned_workflow(
        goal="거절 테스트",
        plan=ra_plan.model_dump(),
        channel="test",
        chat_id="chat-deny",
        session_key="session-deny",
    )
    wf_id = wf.workflow_id
    rt.start(wf_id)
    rt.update_step_cursor(wf_id, step_index=ra_step_idx, step_kind="request_approval")
    rt.wait_for_input(wf_id)

    # 거절 → failed + last_error 기록
    denied = rt.fail(wf_id, last_error="사용자 거절")
    assert denied is not None and denied.state == "failed"
    assert denied.last_error == "사용자 거절"

    # failed 상태에서 approve_workflow → None
    assert rt.approve_workflow(wf_id) is None, (
        "failed 상태에서 approve_workflow → None이어야 함"
    )
