#!/usr/bin/env python3
"""
스모크 테스트: wait_until 단계의 시간 파싱 및 retry_wait 스케줄링을 검증합니다.

실행 방법:
    uv run python scripts/smoke_wait_until.py

성공 시 "PASS" 라인만 출력하고 종료 코드 0.
실패 시 AssertionError 메시지를 출력하고 종료 코드 1.
"""
from __future__ import annotations

import sys
import tempfile
from datetime import datetime, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.workflow.models import WorkflowRecord  # noqa: F401 (type hint in 검증 8)
from shacs_bot.workflow.runtime import WorkflowRuntime
from shacs_bot.workflow.wait_until import parse_wait_until_time


def run_smoke() -> None:
    now = datetime.now().astimezone()

    # ------------------------------------------------------------------ #
    # 검증 1: ISO datetime 파싱
    # ------------------------------------------------------------------ #
    future = now + timedelta(hours=2)
    iso_str = future.strftime("%Y-%m-%dT%H:%M:%S")
    parsed = parse_wait_until_time(f"예약 시각: {iso_str}")
    assert abs((parsed - future).total_seconds()) < 2, (
        f"[FAIL] ISO datetime 파싱 실패. expected ≈ {future.isoformat()}, got {parsed.isoformat()}"
    )
    print("PASS [검증 1] ISO datetime 파싱")

    # ------------------------------------------------------------------ #
    # 검증 2: 상대 기간 - 30분
    # ------------------------------------------------------------------ #
    before = datetime.now().astimezone()
    parsed_min = parse_wait_until_time("30분 후에 재시도")
    assert timedelta(minutes=29, seconds=58) <= (parsed_min - before) <= timedelta(minutes=30, seconds=2), (
        f"[FAIL] '30분' 파싱 실패. delta={parsed_min - before}"
    )
    print("PASS [검증 2] 상대 기간 '30분' 파싱")

    # ------------------------------------------------------------------ #
    # 검증 3: 상대 기간 - 2 hours
    # ------------------------------------------------------------------ #
    before = datetime.now().astimezone()
    parsed_hrs = parse_wait_until_time("wait 2 hours before sending")
    assert timedelta(hours=1, minutes=59, seconds=58) <= (parsed_hrs - before) <= timedelta(hours=2, seconds=2), (
        f"[FAIL] '2 hours' 파싱 실패. delta={parsed_hrs - before}"
    )
    print("PASS [검증 3] 상대 기간 '2 hours' 파싱")

    # ------------------------------------------------------------------ #
    # 검증 4: tomorrow/내일 HH:MM
    # ------------------------------------------------------------------ #
    parsed_tmr = parse_wait_until_time("내일 09:30에 요약 보내기")
    expected_tmr = (now + timedelta(days=1)).replace(hour=9, minute=30, second=0, microsecond=0)
    assert abs((parsed_tmr - expected_tmr).total_seconds()) < 2, (
        f"[FAIL] '내일 09:30' 파싱 실패. expected {expected_tmr.isoformat()}, got {parsed_tmr.isoformat()}"
    )
    print("PASS [검증 4] '내일 09:30' 파싱")

    parsed_tmr_en = parse_wait_until_time("send tomorrow 14:00")
    expected_tmr_en = (now + timedelta(days=1)).replace(hour=14, minute=0, second=0, microsecond=0)
    assert abs((parsed_tmr_en - expected_tmr_en).total_seconds()) < 2, (
        f"[FAIL] 'tomorrow 14:00' 파싱 실패. expected {expected_tmr_en.isoformat()}, got {parsed_tmr_en.isoformat()}"
    )
    print("PASS [검증 4b] 'tomorrow 14:00' 파싱")

    # ------------------------------------------------------------------ #
    # 검증 5: 폴백 - 5분
    # ------------------------------------------------------------------ #
    before = datetime.now().astimezone()
    parsed_fallback = parse_wait_until_time("잠시 대기")
    assert timedelta(minutes=4, seconds=58) <= (parsed_fallback - before) <= timedelta(minutes=5, seconds=2), (
        f"[FAIL] 폴백 5분 실패. delta={parsed_fallback - before}"
    )
    print("PASS [검증 5] 폴백 5분")

    with tempfile.TemporaryDirectory() as tmpdir:
        ws = Path(tmpdir)
        runtime = WorkflowRuntime(workspace=ws)

        record = runtime.register_planned_workflow(
            goal="wait_until 스모크 테스트",
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
        wf_id = record.workflow_id
        runtime.start(wf_id)

        # ------------------------------------------------------------------ #
        # 검증 6: schedule_retry → retry_wait + next_run_at 저장
        # ------------------------------------------------------------------ #
        parsed_dt = parse_wait_until_time("30분 후 재시도")
        scheduled = runtime.schedule_retry(
            wf_id,
            next_run_at=parsed_dt.isoformat(),
            last_error="wait_until 대기 중",
            increment_retries=False,
        )
        assert scheduled is not None
        assert scheduled.state == "retry_wait", (
            f"[FAIL] 상태가 retry_wait 이어야 함. 실제: {scheduled.state}"
        )
        stored_dt = datetime.fromisoformat(scheduled.next_run_at)
        assert abs((stored_dt - parsed_dt).total_seconds()) < 1, (
            f"[FAIL] next_run_at 불일치. expected {parsed_dt.isoformat()}, got {scheduled.next_run_at}"
        )
        print("PASS [검증 6] schedule_retry → retry_wait + next_run_at 저장")

        # ------------------------------------------------------------------ #
        # 검증 7: 미래 next_run_at → _is_retry_due = False
        # ------------------------------------------------------------------ #
        assert not runtime._is_retry_due(scheduled), "[FAIL] 미래 시각인데 retry_due = True"
        print("PASS [검증 7] 미래 next_run_at → retry_due = False")

        # ------------------------------------------------------------------ #
        # 검증 8: 과거 next_run_at → _is_retry_due = True (직접 레코드 조작)
        # ------------------------------------------------------------------ #
        past_dt = datetime.now().astimezone() - timedelta(seconds=30)
        due_record = scheduled.model_copy(update={"next_run_at": past_dt.isoformat()})
        assert runtime._is_retry_due(due_record), "[FAIL] 과거 시각인데 retry_due = False"
        print("PASS [검증 8] 과거 next_run_at → retry_due = True")

        # ------------------------------------------------------------------ #
        # 검증 9: 재시작/재로드 후 next_run_at 보존
        # ------------------------------------------------------------------ #
        runtime2 = WorkflowRuntime(workspace=ws)
        reloaded = runtime2.store.get(wf_id)
        assert reloaded is not None
        assert reloaded.next_run_at == scheduled.next_run_at, (
            f"[FAIL] 재로드 후 next_run_at 달라짐. "
            f"expected {scheduled.next_run_at}, got {reloaded.next_run_at}"
        )
        print("PASS [검증 9] 재시작/재로드 후 next_run_at 보존")

        # ------------------------------------------------------------------ #
        # 검증 10: 만료된 retry_wait → recover_restart → queued 복구
        # ------------------------------------------------------------------ #
        record2 = runtime2.register_planned_workflow(
            goal="recover_restart 테스트",
            plan={"kind": "planned_workflow", "steps": []},
            channel="test",
            chat_id="test2",
            session_key="s2",
        )
        wf2_id = record2.workflow_id
        runtime2.start(wf2_id)

        past_run_at = (datetime.now().astimezone() - timedelta(minutes=10)).isoformat()
        due_stored = runtime2.store.get(wf2_id)
        assert due_stored is not None
        runtime2.store.upsert_and_get(
            due_stored.model_copy(update={"state": "retry_wait", "next_run_at": past_run_at})
        )

        runtime3 = WorkflowRuntime(workspace=ws)
        recovered = runtime3.recover_restart()
        recovered_ids = {r.workflow_id for r in recovered}
        assert wf2_id in recovered_ids, (
            f"[FAIL] recover_restart가 만료 항목을 복구하지 않음. recovered={recovered_ids}"
        )
        final = runtime3.store.get(wf2_id)
        assert final is not None
        assert final.state == "queued", (
            f"[FAIL] 복구 후 상태가 queued 이어야 함. 실제: {final.state}"
        )
        print("PASS [검증 10] recover_restart → 만료 retry_wait → queued 복구")

    # ------------------------------------------------------------------ #
    # 검증 11: PlanStep step_meta 직렬화/역직렬화 라운드트립
    # ------------------------------------------------------------------ #
    from shacs_bot.agent.planner import PlanStep, AssistantPlan

    step_with_meta = PlanStep(
        kind="wait_until",
        description="fallback 텍스트",
        step_meta={"iso_time": "2026-06-01T10:00:00", "duration_minutes": 45},
    )
    dumped = step_with_meta.model_dump()
    assert dumped["step_meta"] == {"iso_time": "2026-06-01T10:00:00", "duration_minutes": 45}, (
        f"[FAIL] model_dump step_meta 불일치. got {dumped['step_meta']!r}"
    )
    roundtrip = PlanStep.model_validate(dumped)
    assert roundtrip.step_meta == step_with_meta.step_meta, (
        f"[FAIL] 라운드트립 step_meta 불일치. got {roundtrip.step_meta!r}"
    )
    print("PASS [검증 11] PlanStep step_meta 직렬화/역직렬화 라운드트립")

    # ------------------------------------------------------------------ #
    # 검증 12: step_meta 없는 구버전 PlanStep → step_meta = {} (하위 호환)
    # ------------------------------------------------------------------ #
    old_step = PlanStep.model_validate(
        {"kind": "wait_until", "description": "30분 후 재시도", "depends_on": []}
    )
    assert old_step.step_meta == {}, (
        f"[FAIL] step_meta 기본값이 {{}} 이어야 함. got {old_step.step_meta!r}"
    )
    print("PASS [검증 12] step_meta 없는 구버전 스텝 → step_meta={} (하위 호환)")

    # ------------------------------------------------------------------ #
    # 검증 13: AssistantPlan 안의 PlanStep에 step_meta 포함 → 정상 validate
    # ------------------------------------------------------------------ #
    plan_with_meta = AssistantPlan.model_validate({
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
    assert plan_with_meta.steps[0].step_meta.get("duration_minutes") == 90
    assert plan_with_meta.steps[1].step_meta.get("prompt") == "진행할까요?"
    assert plan_with_meta.steps[2].step_meta.get("prompt") == "이 작업을 승인하시겠습니까?"
    print("PASS [검증 13] AssistantPlan 내 PlanStep step_meta 파싱")

    # ------------------------------------------------------------------ #
    # 검증 14: executor-facing - iso_time 메타데이터 우선 소비
    # ------------------------------------------------------------------ #
    target_iso = (datetime.now().astimezone() + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%S")
    step_iso = PlanStep(
        kind="wait_until",
        description="이 텍스트는 무시되어야 함",
        step_meta={"iso_time": target_iso},
    )
    # executor 로직 인라인 시뮬레이션 (loop.py wait_until 블록과 동일 순서)
    _iso = step_iso.step_meta.get("iso_time")
    _dur = step_iso.step_meta.get("duration_minutes")
    if isinstance(_iso, str) and _iso:
        _dt = datetime.fromisoformat(_iso)
        if _dt.tzinfo is None:
            _dt = _dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
    elif isinstance(_dur, (int, float)) and _dur > 0:
        _dt = datetime.now().astimezone() + timedelta(minutes=_dur)
    else:
        _dt = parse_wait_until_time(step_iso.description)
    expected_dt = datetime.fromisoformat(target_iso).replace(tzinfo=datetime.now().astimezone().tzinfo)
    assert abs((_dt - expected_dt).total_seconds()) < 1, (
        f"[FAIL] iso_time 메타데이터 우선 소비 실패. expected {expected_dt.isoformat()}, got {_dt.isoformat()}"
    )
    print("PASS [검증 14] executor: iso_time 메타데이터 우선 소비")

    # ------------------------------------------------------------------ #
    # 검증 15: executor-facing - duration_minutes 메타데이터 우선 소비
    # ------------------------------------------------------------------ #
    step_dur = PlanStep(
        kind="wait_until",
        description="이 텍스트도 무시되어야 함",
        step_meta={"duration_minutes": 45},
    )
    _iso2 = step_dur.step_meta.get("iso_time")
    _dur2 = step_dur.step_meta.get("duration_minutes")
    _before = datetime.now().astimezone()
    if isinstance(_iso2, str) and _iso2:
        _dt2 = datetime.fromisoformat(_iso2)
    elif isinstance(_dur2, (int, float)) and _dur2 > 0:
        _dt2 = datetime.now().astimezone() + timedelta(minutes=_dur2)
    else:
        _dt2 = parse_wait_until_time(step_dur.description)
    assert timedelta(minutes=44, seconds=58) <= (_dt2 - _before) <= timedelta(minutes=45, seconds=2), (
        f"[FAIL] duration_minutes=45 우선 소비 실패. delta={_dt2 - _before}"
    )
    print("PASS [검증 15] executor: duration_minutes 메타데이터 우선 소비")

    # ------------------------------------------------------------------ #
    # 검증 16: executor-facing - step_meta 없으면 description fallback
    # ------------------------------------------------------------------ #
    step_fallback = PlanStep(kind="wait_until", description="2 hours later please")
    _iso3 = step_fallback.step_meta.get("iso_time")
    _dur3 = step_fallback.step_meta.get("duration_minutes")
    _before3 = datetime.now().astimezone()
    if isinstance(_iso3, str) and _iso3:
        _dt3 = datetime.fromisoformat(_iso3)
    elif isinstance(_dur3, (int, float)) and _dur3 > 0:
        _dt3 = datetime.now().astimezone() + timedelta(minutes=_dur3)
    else:
        _dt3 = parse_wait_until_time(step_fallback.description)
    assert timedelta(hours=1, minutes=59, seconds=58) <= (_dt3 - _before3) <= timedelta(hours=2, seconds=2), (
        f"[FAIL] description fallback 실패. delta={_dt3 - _before3}"
    )
    print("PASS [검증 16] executor: step_meta 없음 → description fallback")

    print("\n모든 검증 통과 ✓")


if __name__ == "__main__":
    try:
        run_smoke()
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
