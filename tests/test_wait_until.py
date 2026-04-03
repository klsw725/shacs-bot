"""wait_until 시간 파싱 및 retry_wait 스케줄링 테스트."""
from __future__ import annotations

from datetime import datetime, timedelta
from pathlib import Path

import pytest

from shacs_bot.agent.planner import AssistantPlan, PlanStep
from shacs_bot.workflow.runtime import WorkflowRuntime
from shacs_bot.workflow.wait_until import parse_wait_until_time


# ──────────────────────────────────────────────
# parse_wait_until_time
# ──────────────────────────────────────────────


def test_parse_wait_until_iso_datetime() -> None:
    now = datetime.now().astimezone()
    future = now + timedelta(hours=2)
    iso_str = future.strftime("%Y-%m-%dT%H:%M:%S")
    parsed = parse_wait_until_time(f"예약 시각: {iso_str}")
    assert abs((parsed - future).total_seconds()) < 2, (
        f"ISO datetime 파싱 실패. expected ≈ {future.isoformat()}, got {parsed.isoformat()}"
    )


def test_parse_wait_until_relative_minutes() -> None:
    before = datetime.now().astimezone()
    parsed = parse_wait_until_time("30분 후에 재시도")
    delta = parsed - before
    assert timedelta(minutes=29, seconds=58) <= delta <= timedelta(minutes=30, seconds=2), (
        f"'30분' 파싱 실패. delta={delta}"
    )


def test_parse_wait_until_relative_hours() -> None:
    before = datetime.now().astimezone()
    parsed = parse_wait_until_time("wait 2 hours before sending")
    delta = parsed - before
    assert timedelta(hours=1, minutes=59, seconds=58) <= delta <= timedelta(hours=2, seconds=2), (
        f"'2 hours' 파싱 실패. delta={delta}"
    )


def test_parse_wait_until_tomorrow_korean() -> None:
    now = datetime.now().astimezone()
    parsed = parse_wait_until_time("내일 09:30에 요약 보내기")
    expected = (now + timedelta(days=1)).replace(hour=9, minute=30, second=0, microsecond=0)
    assert abs((parsed - expected).total_seconds()) < 2, (
        f"'내일 09:30' 파싱 실패. expected {expected.isoformat()}, got {parsed.isoformat()}"
    )


def test_parse_wait_until_tomorrow_english() -> None:
    now = datetime.now().astimezone()
    parsed = parse_wait_until_time("send tomorrow 14:00")
    expected = (now + timedelta(days=1)).replace(hour=14, minute=0, second=0, microsecond=0)
    assert abs((parsed - expected).total_seconds()) < 2, (
        f"'tomorrow 14:00' 파싱 실패. expected {expected.isoformat()}, got {parsed.isoformat()}"
    )


def test_parse_wait_until_fallback_five_minutes() -> None:
    before = datetime.now().astimezone()
    parsed = parse_wait_until_time("잠시 대기")
    delta = parsed - before
    assert timedelta(minutes=4, seconds=58) <= delta <= timedelta(minutes=5, seconds=2), (
        f"폴백 5분 실패. delta={delta}"
    )


# ──────────────────────────────────────────────
# WorkflowRuntime.schedule_retry / _is_retry_due
# ──────────────────────────────────────────────


def _register_workflow(runtime: WorkflowRuntime) -> str:
    record = runtime.register_planned_workflow(
        goal="wait_until 테스트",
        plan={
            "kind": "planned_workflow",
            "steps": [
                {"kind": "wait_until", "description": "30분 후 재시도", "depends_on": []},
                {"kind": "send_result", "description": "결과 전달", "depends_on": [0]},
            ],
        },
        channel="test",
        chat_id="test-chat",
        session_key="test-session",
    )
    return record.workflow_id


def test_schedule_retry_sets_retry_wait_state(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    wf_id = _register_workflow(runtime)
    runtime.start(wf_id)

    parsed_dt = parse_wait_until_time("30분 후 재시도")
    scheduled = runtime.schedule_retry(
        wf_id,
        next_run_at=parsed_dt.isoformat(),
        last_error="wait_until 대기 중",
        increment_retries=False,
    )
    assert scheduled is not None
    assert scheduled.state == "retry_wait", (
        f"상태가 retry_wait이어야 함. 실제: {scheduled.state}"
    )
    stored_dt = datetime.fromisoformat(scheduled.next_run_at)
    assert abs((stored_dt - parsed_dt).total_seconds()) < 1, (
        f"next_run_at 불일치. expected {parsed_dt.isoformat()}, got {scheduled.next_run_at}"
    )


def test_is_retry_due_future_is_false(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    wf_id = _register_workflow(runtime)
    runtime.start(wf_id)

    future_dt = parse_wait_until_time("30분 후 재시도")
    scheduled = runtime.schedule_retry(
        wf_id,
        next_run_at=future_dt.isoformat(),
        last_error="wait_until 대기 중",
        increment_retries=False,
    )
    assert not runtime._is_retry_due(scheduled), "미래 시각인데 retry_due = True"


def test_is_retry_due_past_is_true(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    wf_id = _register_workflow(runtime)
    runtime.start(wf_id)

    future_dt = parse_wait_until_time("30분 후 재시도")
    scheduled = runtime.schedule_retry(
        wf_id,
        next_run_at=future_dt.isoformat(),
        last_error="wait_until 대기 중",
        increment_retries=False,
    )

    past_dt = datetime.now().astimezone() - timedelta(seconds=30)
    due_record = scheduled.model_copy(update={"next_run_at": past_dt.isoformat()})
    assert runtime._is_retry_due(due_record), "과거 시각인데 retry_due = False"


def test_next_run_at_persists_after_reload(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    wf_id = _register_workflow(runtime)
    runtime.start(wf_id)

    parsed_dt = parse_wait_until_time("30분 후 재시도")
    scheduled = runtime.schedule_retry(
        wf_id,
        next_run_at=parsed_dt.isoformat(),
        last_error="wait_until 대기 중",
        increment_retries=False,
    )

    runtime2 = WorkflowRuntime(workspace=tmp_path)
    reloaded = runtime2.store.get(wf_id)
    assert reloaded is not None
    assert reloaded.next_run_at == scheduled.next_run_at, (
        f"재로드 후 next_run_at 달라짐. expected {scheduled.next_run_at}, got {reloaded.next_run_at}"
    )


def test_recover_restart_requeues_expired_retry_wait(tmp_path: Path) -> None:
    runtime = WorkflowRuntime(workspace=tmp_path)
    record = runtime.register_planned_workflow(
        goal="recover_restart 테스트",
        plan={"kind": "planned_workflow", "steps": []},
        channel="test",
        chat_id="test2",
        session_key="s2",
    )
    wf_id = record.workflow_id
    runtime.start(wf_id)

    past_run_at = (datetime.now().astimezone() - timedelta(minutes=10)).isoformat()
    stored = runtime.store.get(wf_id)
    assert stored is not None
    runtime.store.upsert_and_get(
        stored.model_copy(update={"state": "retry_wait", "next_run_at": past_run_at})
    )

    runtime2 = WorkflowRuntime(workspace=tmp_path)
    recovered = runtime2.recover_restart()
    recovered_ids = {r.workflow_id for r in recovered}
    assert wf_id in recovered_ids, (
        f"recover_restart가 만료 항목을 복구하지 않음. recovered={recovered_ids}"
    )
    final = runtime2.store.get(wf_id)
    assert final is not None
    assert final.state == "queued", (
        f"복구 후 상태가 queued이어야 함. 실제: {final.state}"
    )


# ──────────────────────────────────────────────
# PlanStep step_meta 직렬화
# ──────────────────────────────────────────────


def test_plan_step_step_meta_roundtrip() -> None:
    step = PlanStep(
        kind="wait_until",
        description="fallback 텍스트",
        step_meta={"iso_time": "2026-06-01T10:00:00", "duration_minutes": 45},
    )
    dumped = step.model_dump()
    assert dumped["step_meta"] == {"iso_time": "2026-06-01T10:00:00", "duration_minutes": 45}
    roundtrip = PlanStep.model_validate(dumped)
    assert roundtrip.step_meta == step.step_meta


def test_plan_step_step_meta_default_is_empty_dict() -> None:
    old_step = PlanStep.model_validate(
        {"kind": "wait_until", "description": "30분 후 재시도", "depends_on": []}
    )
    assert old_step.step_meta == {}, (
        f"step_meta 기본값이 {{}}이어야 함. got {old_step.step_meta!r}"
    )


def test_assistant_plan_step_meta_parsing() -> None:
    plan = AssistantPlan.model_validate({
        "kind": "planned_workflow",
        "steps": [
            {
                "kind": "wait_until",
                "description": "fallback",
                "step_meta": {"duration_minutes": 90},
            },
            {
                "kind": "ask_user",
                "description": "fallback question",
                "step_meta": {"prompt": "진행할까요?"},
                "depends_on": [0],
            },
            {
                "kind": "request_approval",
                "description": "fallback approval",
                "step_meta": {"prompt": "이 작업을 승인하시겠습니까?"},
                "depends_on": [1],
            },
        ],
    })
    assert plan.steps[0].step_meta.get("duration_minutes") == 90
    assert plan.steps[1].step_meta.get("prompt") == "진행할까요?"
    assert plan.steps[2].step_meta.get("prompt") == "이 작업을 승인하시겠습니까?"


# ──────────────────────────────────────────────
# executor-facing step_meta 우선 소비 로직
# ──────────────────────────────────────────────


def _resolve_wait_time(step: PlanStep) -> datetime:
    """loop.py wait_until 블록과 동일한 우선순위 로직."""
    _iso = step.step_meta.get("iso_time")
    _dur = step.step_meta.get("duration_minutes")
    if isinstance(_iso, str) and _iso:
        _dt = datetime.fromisoformat(_iso)
        if _dt.tzinfo is None:
            _dt = _dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
        return _dt
    elif isinstance(_dur, (int, float)) and _dur > 0:
        return datetime.now().astimezone() + timedelta(minutes=_dur)
    else:
        return parse_wait_until_time(step.description)


def test_executor_iso_time_meta_takes_priority() -> None:
    target_iso = (datetime.now().astimezone() + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%S")
    step = PlanStep(
        kind="wait_until",
        description="이 텍스트는 무시되어야 함",
        step_meta={"iso_time": target_iso},
    )
    result = _resolve_wait_time(step)
    expected = datetime.fromisoformat(target_iso).replace(tzinfo=datetime.now().astimezone().tzinfo)
    assert abs((result - expected).total_seconds()) < 1, (
        f"iso_time 메타데이터 우선 소비 실패. expected {expected.isoformat()}, got {result.isoformat()}"
    )


def test_executor_duration_minutes_meta_takes_priority() -> None:
    step = PlanStep(
        kind="wait_until",
        description="이 텍스트도 무시되어야 함",
        step_meta={"duration_minutes": 45},
    )
    before = datetime.now().astimezone()
    result = _resolve_wait_time(step)
    delta = result - before
    assert timedelta(minutes=44, seconds=58) <= delta <= timedelta(minutes=45, seconds=2), (
        f"duration_minutes=45 우선 소비 실패. delta={delta}"
    )


def test_executor_description_fallback_when_no_step_meta() -> None:
    step = PlanStep(kind="wait_until", description="2 hours later please")
    before = datetime.now().astimezone()
    result = _resolve_wait_time(step)
    delta = result - before
    assert timedelta(hours=1, minutes=59, seconds=58) <= delta <= timedelta(hours=2, seconds=2), (
        f"description fallback 실패. delta={delta}"
    )
