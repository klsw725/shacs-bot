"""_classify_request() 규칙 기반 플래너 메타데이터 테스트."""
from __future__ import annotations

from datetime import datetime, timedelta

import pytest

from shacs_bot.agent.loop import AgentLoop


def classify(text: str):
    return AgentLoop._classify_request(text)


# ──────────────────────────────────────────────
# wait_until: iso_time 포함 여부
# ──────────────────────────────────────────────


@pytest.mark.parametrize("text", [
    "30분 후에 날씨 조사해서 알려줘",
    "wait 45 minutes then summarize the report",
    "30분 지나서 보고서 전달해줘",
    "wait for 20 minutes and then send the status update",
    "after 1 hour send the completed report to the team",
])
def test_classify_wait_until_has_iso_time(text: str) -> None:
    plan = classify(text)
    assert plan.kind == "planned_workflow", f"kind={plan.kind!r}"
    kinds = [s.kind for s in plan.steps]
    assert "wait_until" in kinds, f"wait_until 스텝 없음. steps={kinds}"
    wu_step = next(s for s in plan.steps if s.kind == "wait_until")
    iso_time = wu_step.step_meta.get("iso_time")
    assert isinstance(iso_time, str) and iso_time, (
        f"wait_until step_meta.iso_time 없음. step_meta={wu_step.step_meta!r}"
    )
    parsed_dt = datetime.fromisoformat(iso_time)
    if parsed_dt.tzinfo is None:
        parsed_dt = parsed_dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
    assert parsed_dt > datetime.now().astimezone(), f"iso_time이 미래여야 함. iso_time={iso_time!r}"


def test_classify_wait_until_two_hours_delta() -> None:
    plan = classify("2시간 뒤에 보고서 정리해서 보내줘")
    kinds = [s.kind for s in plan.steps]
    assert "wait_until" in kinds
    wu = next(s for s in plan.steps if s.kind == "wait_until")
    parsed = datetime.fromisoformat(wu.step_meta["iso_time"])
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=datetime.now().astimezone().tzinfo)
    delta = parsed - datetime.now().astimezone()
    assert timedelta(hours=1, minutes=55) <= delta <= timedelta(hours=2, minutes=5), (
        f"2시간 후 iso_time delta={delta}"
    )


def test_classify_wait_until_tomorrow_korean() -> None:
    plan = classify("내일 09:00에 일정 정리해서 보내줘")
    kinds = [s.kind for s in plan.steps]
    assert "wait_until" in kinds
    wu = next(s for s in plan.steps if s.kind == "wait_until")
    parsed = datetime.fromisoformat(wu.step_meta["iso_time"])
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=datetime.now().astimezone().tzinfo)
    tomorrow_9am = (datetime.now().astimezone() + timedelta(days=1)).replace(
        hour=9, minute=0, second=0, microsecond=0
    )
    assert abs((parsed - tomorrow_9am).total_seconds()) < 5, (
        f"내일 09:00 iso_time 불일치. got={wu.step_meta['iso_time']!r}"
    )


def test_classify_wait_until_one_hour_delta() -> None:
    plan = classify("after 1 hour send the completed report to the team")
    kinds = [s.kind for s in plan.steps]
    assert "wait_until" in kinds
    wu = next(s for s in plan.steps if s.kind == "wait_until")
    parsed = datetime.fromisoformat(wu.step_meta["iso_time"])
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=datetime.now().astimezone().tzinfo)
    delta = parsed - datetime.now().astimezone()
    assert timedelta(minutes=55) <= delta <= timedelta(hours=1, minutes=5), (
        f"1시간 후 iso_time delta={delta}"
    )


def test_classify_wait_until_plan_structure() -> None:
    """wait_until 플랜은 wait_until → research → send_result 순서."""
    plan = classify("30분 후에 이메일 확인해줘")
    kinds = [s.kind for s in plan.steps]
    assert kinds == ["wait_until", "research", "send_result"], (
        f"wait_until 플랜 step 순서 오류. steps={kinds}"
    )


# ──────────────────────────────────────────────
# request_approval
# ──────────────────────────────────────────────


def test_classify_request_approval_korean() -> None:
    plan = classify("파일을 삭제하기 전에 확인 후에 진행해줘")
    assert plan.kind == "planned_workflow", f"kind={plan.kind!r}"
    kinds = [s.kind for s in plan.steps]
    assert "request_approval" in kinds, f"request_approval 없음. steps={kinds}"
    ra_step = next(s for s in plan.steps if s.kind == "request_approval")
    prompt = ra_step.step_meta.get("prompt")
    assert isinstance(prompt, str) and prompt, (
        f"request_approval prompt 없음. step_meta={ra_step.step_meta!r}"
    )


def test_classify_request_approval_english() -> None:
    plan = classify("please confirm before sending the email to all users")
    kinds = [s.kind for s in plan.steps]
    assert "request_approval" in kinds, f"request_approval 없음. steps={kinds}"
    ra = next(s for s in plan.steps if s.kind == "request_approval")
    assert isinstance(ra.step_meta.get("prompt"), str), (
        f"prompt 없음. step_meta={ra.step_meta!r}"
    )


# ──────────────────────────────────────────────
# ask_user
# ──────────────────────────────────────────────


@pytest.mark.parametrize("text", [
    "사용자에게 선택을 물어보고 그에 맞게 조사해줘",
    "선호도를 묻고 나서 그에 맞게 조사해줘",
])
def test_classify_ask_user_korean(text: str) -> None:
    plan = classify(text)
    assert plan.kind == "planned_workflow", f"kind={plan.kind!r}"
    kinds = [s.kind for s in plan.steps]
    assert "ask_user" in kinds, f"ask_user 없음. steps={kinds}"
    au = next(s for s in plan.steps if s.kind == "ask_user")
    assert isinstance(au.step_meta.get("prompt"), str) and au.step_meta.get("prompt"), (
        f"ask_user prompt 없음. step_meta={au.step_meta!r}"
    )


def test_classify_ask_user_english() -> None:
    plan = classify("ask me what format to use before generating the report")
    kinds = [s.kind for s in plan.steps]
    assert "ask_user" in kinds, f"ask_user 없음. steps={kinds}"


# ──────────────────────────────────────────────
# 회귀: generic / direct_answer / clarification
# ──────────────────────────────────────────────


def test_classify_generic_compound_has_no_step_meta() -> None:
    plan = classify("먼저 데이터를 수집하고 그 다음에 분석해서 보내줘")
    assert plan.kind == "planned_workflow"
    kinds = [s.kind for s in plan.steps]
    assert "research" in kinds
    assert "send_result" in kinds
    for s in plan.steps:
        assert s.step_meta == {}, f"generic 플랜 step에 step_meta 있음: {s.step_meta!r}"


def test_classify_short_text_is_direct_answer() -> None:
    plan = classify("안녕")
    assert plan.kind == "direct_answer", f"kind={plan.kind!r}"


def test_classify_vague_request_is_clarification() -> None:
    plan = classify("이거 해줘")
    assert plan.kind == "clarification", f"kind={plan.kind!r}"


# ──────────────────────────────────────────────
# planned_workflow 스케줄 키워드
# ──────────────────────────────────────────────


@pytest.mark.parametrize("text", [
    "매주 월요일 오전에 주간 보고서 자동으로 전송해줘",
    "remind me every Monday to review the weekly metrics dashboard",
    "send a monthly summary report to the entire team automatically",
])
def test_classify_scheduled_keywords_produce_planned_workflow(text: str) -> None:
    plan = classify(text)
    assert plan.kind == "planned_workflow", f"kind={plan.kind!r}"
