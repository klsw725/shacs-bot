#!/usr/bin/env python3
"""
스모크 테스트: _classify_request() 플래너 출력에 구조화 메타데이터 포함 여부 검증.

실행 방법:
    uv run python scripts/smoke_planner_metadata.py

성공 시 "PASS" 라인만 출력하고 종료 코드 0.
실패 시 AssertionError 메시지를 출력하고 종료 코드 1.
"""
from __future__ import annotations

import sys
from datetime import datetime, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.agent.loop import AgentLoop


def classify(text: str):
    return AgentLoop._classify_request(text)


def run_smoke() -> None:
    # ------------------------------------------------------------------ #
    # 검증 1: wait_until — 한국어 "N분 후"
    # ------------------------------------------------------------------ #
    plan = classify("30분 후에 날씨 조사해서 알려줘")
    assert plan.kind == "planned_workflow", f"[FAIL] kind={plan.kind!r}, expected planned_workflow"
    kinds = [s.kind for s in plan.steps]
    assert "wait_until" in kinds, f"[FAIL] wait_until 스텝 없음. steps={kinds}"
    wu_step = next(s for s in plan.steps if s.kind == "wait_until")
    iso_time = wu_step.step_meta.get("iso_time")
    assert isinstance(iso_time, str) and iso_time, (
        f"[FAIL] wait_until step_meta.iso_time 없음. step_meta={wu_step.step_meta!r}"
    )
    # iso_time은 현재보다 미래여야 함
    parsed_dt = datetime.fromisoformat(iso_time)
    if parsed_dt.tzinfo is None:
        parsed_dt = parsed_dt.replace(tzinfo=datetime.now().astimezone().tzinfo)
    assert parsed_dt > datetime.now().astimezone(), (
        f"[FAIL] iso_time이 미래여야 함. iso_time={iso_time!r}"
    )
    print("PASS [검증 1] '30분 후에 ~' → wait_until 스텝 + iso_time 메타데이터")

    # ------------------------------------------------------------------ #
    # 검증 2: wait_until — 한국어 "N시간 뒤"
    # ------------------------------------------------------------------ #
    plan2 = classify("2시간 뒤에 보고서 정리해서 보내줘")
    kinds2 = [s.kind for s in plan2.steps]
    assert "wait_until" in kinds2, f"[FAIL] wait_until 없음. steps={kinds2}"
    wu2 = next(s for s in plan2.steps if s.kind == "wait_until")
    assert isinstance(wu2.step_meta.get("iso_time"), str), (
        f"[FAIL] iso_time 없음. step_meta={wu2.step_meta!r}"
    )
    # 2시간 후 → iso_time 이 약 2시간 뒤임을 검증
    parsed2 = datetime.fromisoformat(wu2.step_meta["iso_time"])
    if parsed2.tzinfo is None:
        parsed2 = parsed2.replace(tzinfo=datetime.now().astimezone().tzinfo)
    delta2 = parsed2 - datetime.now().astimezone()
    assert timedelta(hours=1, minutes=55) <= delta2 <= timedelta(hours=2, minutes=5), (
        f"[FAIL] 2시간 후 iso_time delta={delta2}"
    )
    print("PASS [검증 2] '2시간 뒤에 ~' → wait_until + iso_time ≈ +2h")

    # ------------------------------------------------------------------ #
    # 검증 3: wait_until — 영어 "wait N minutes"
    # ------------------------------------------------------------------ #
    plan3 = classify("wait 45 minutes then summarize the report")
    kinds3 = [s.kind for s in plan3.steps]
    assert "wait_until" in kinds3, f"[FAIL] wait_until 없음. steps={kinds3}"
    wu3 = next(s for s in plan3.steps if s.kind == "wait_until")
    assert isinstance(wu3.step_meta.get("iso_time"), str), (
        f"[FAIL] iso_time 없음. step_meta={wu3.step_meta!r}"
    )
    print("PASS [검증 3] 'wait 45 minutes then ~' → wait_until + iso_time")

    # ------------------------------------------------------------------ #
    # 검증 4: wait_until — 내일 HH:MM
    # ------------------------------------------------------------------ #
    plan4 = classify("내일 09:00에 일정 정리해서 보내줘")
    kinds4 = [s.kind for s in plan4.steps]
    assert "wait_until" in kinds4, f"[FAIL] wait_until 없음. steps={kinds4}"
    wu4 = next(s for s in plan4.steps if s.kind == "wait_until")
    parsed4 = datetime.fromisoformat(wu4.step_meta["iso_time"])
    if parsed4.tzinfo is None:
        parsed4 = parsed4.replace(tzinfo=datetime.now().astimezone().tzinfo)
    tomorrow_9am = (datetime.now().astimezone() + timedelta(days=1)).replace(
        hour=9, minute=0, second=0, microsecond=0
    )
    assert abs((parsed4 - tomorrow_9am).total_seconds()) < 5, (
        f"[FAIL] 내일 09:00 iso_time 불일치. got={wu4.step_meta['iso_time']!r}"
    )
    print("PASS [검증 4] '내일 09:00에 ~' → wait_until + iso_time = 내일 09:00")

    # ------------------------------------------------------------------ #
    # 검증 5: request_approval — 한국어 "확인 후"
    # ------------------------------------------------------------------ #
    plan5 = classify("파일을 삭제하기 전에 확인 후에 진행해줘")
    assert plan5.kind == "planned_workflow", f"[FAIL] kind={plan5.kind!r}"
    kinds5 = [s.kind for s in plan5.steps]
    assert "request_approval" in kinds5, f"[FAIL] request_approval 없음. steps={kinds5}"
    ra_step = next(s for s in plan5.steps if s.kind == "request_approval")
    prompt5 = ra_step.step_meta.get("prompt")
    assert isinstance(prompt5, str) and prompt5, (
        f"[FAIL] request_approval prompt 없음. step_meta={ra_step.step_meta!r}"
    )
    assert "파일을 삭제하기" in prompt5 or "확인 후에" in prompt5, (
        f"[FAIL] prompt에 원문 없음. prompt={prompt5!r}"
    )
    print("PASS [검증 5] '확인 후에 ~' → request_approval + prompt 메타데이터")

    # ------------------------------------------------------------------ #
    # 검증 6: request_approval — 영어 "confirm before"
    # ------------------------------------------------------------------ #
    plan6 = classify("please confirm before sending the email to all users")
    kinds6 = [s.kind for s in plan6.steps]
    assert "request_approval" in kinds6, f"[FAIL] request_approval 없음. steps={kinds6}"
    ra6 = next(s for s in plan6.steps if s.kind == "request_approval")
    assert isinstance(ra6.step_meta.get("prompt"), str), (
        f"[FAIL] prompt 없음. step_meta={ra6.step_meta!r}"
    )
    print("PASS [검증 6] 'confirm before ~' → request_approval + prompt 메타데이터")

    # ------------------------------------------------------------------ #
    # 검증 7: ask_user — 한국어 "물어보고"
    # ------------------------------------------------------------------ #
    plan7 = classify("사용자에게 선택을 물어보고 그에 맞게 조사해줘")
    assert plan7.kind == "planned_workflow", f"[FAIL] kind={plan7.kind!r}"
    kinds7 = [s.kind for s in plan7.steps]
    assert "ask_user" in kinds7, f"[FAIL] ask_user 없음. steps={kinds7}"
    au_step = next(s for s in plan7.steps if s.kind == "ask_user")
    prompt7 = au_step.step_meta.get("prompt")
    assert isinstance(prompt7, str) and prompt7, (
        f"[FAIL] ask_user prompt 없음. step_meta={au_step.step_meta!r}"
    )
    print("PASS [검증 7] '물어보고 ~' → ask_user + prompt 메타데이터")

    # ------------------------------------------------------------------ #
    # 검증 8: ask_user — 영어 "ask me"
    # ------------------------------------------------------------------ #
    plan8 = classify("ask me what format to use before generating the report")
    kinds8 = [s.kind for s in plan8.steps]
    assert "ask_user" in kinds8, f"[FAIL] ask_user 없음. steps={kinds8}"
    print("PASS [검증 8] 'ask me ~' → ask_user 스텝")

    # ------------------------------------------------------------------ #
    # 검증 9: 회귀 — 일반 복합 요청은 기존 generic 플랜 유지
    # ------------------------------------------------------------------ #
    plan9 = classify("먼저 데이터를 수집하고 그 다음에 분석해서 보내줘")
    assert plan9.kind == "planned_workflow", f"[FAIL] kind={plan9.kind!r}"
    kinds9 = [s.kind for s in plan9.steps]
    assert "research" in kinds9, f"[FAIL] research 없음. steps={kinds9}"
    assert "send_result" in kinds9, f"[FAIL] send_result 없음. steps={kinds9}"
    # generic 플랜은 step_meta가 비어있어야 함
    for s in plan9.steps:
        assert s.step_meta == {}, f"[FAIL] generic 플랜 step에 step_meta 있음: {s.step_meta!r}"
    print("PASS [검증 9] 일반 복합 요청 → 기존 generic 플랜 (step_meta 없음)")

    # ------------------------------------------------------------------ #
    # 검증 10: 회귀 — 짧은 텍스트는 direct_answer
    # ------------------------------------------------------------------ #
    plan10 = classify("안녕")
    assert plan10.kind == "direct_answer", f"[FAIL] kind={plan10.kind!r}"
    print("PASS [검증 10] 짧은 텍스트 → direct_answer")

    # ------------------------------------------------------------------ #
    # 검증 11: 회귀 — 모호한 요청은 clarification
    # ------------------------------------------------------------------ #
    plan11 = classify("이거 해줘")
    assert plan11.kind == "clarification", f"[FAIL] kind={plan11.kind!r}"
    print("PASS [검증 11] 모호한 요청 → clarification")

    # ------------------------------------------------------------------ #
    # 검증 12: wait_until 플랜은 research + send_result 도 포함
    # ------------------------------------------------------------------ #
    plan12 = classify("30분 후에 이메일 확인해줘")
    kinds12 = [s.kind for s in plan12.steps]
    assert kinds12 == ["wait_until", "research", "send_result"], (
        f"[FAIL] wait_until 플랜 step 순서 오류. steps={kinds12}"
    )
    print("PASS [검증 12] wait_until 플랜 구조: wait_until → research → send_result")

    # ------------------------------------------------------------------ #
    # 검증 13: wait_until — 한국어 "N분 지나서"
    # ------------------------------------------------------------------ #
    plan13 = classify("30분 지나서 보고서 전달해줘")
    assert plan13.kind == "planned_workflow", f"[FAIL] kind={plan13.kind!r}"
    kinds13 = [s.kind for s in plan13.steps]
    assert "wait_until" in kinds13, f"[FAIL] wait_until 없음. steps={kinds13}"
    wu13 = next(s for s in plan13.steps if s.kind == "wait_until")
    assert isinstance(wu13.step_meta.get("iso_time"), str), (
        f"[FAIL] iso_time 없음. step_meta={wu13.step_meta!r}"
    )
    print("PASS [검증 13] '30분 지나서 ~' → wait_until + iso_time")

    # ------------------------------------------------------------------ #
    # 검증 14: wait_until — 영어 "wait for N minutes"
    # ------------------------------------------------------------------ #
    plan14 = classify("wait for 20 minutes and then send the status update")
    kinds14 = [s.kind for s in plan14.steps]
    assert "wait_until" in kinds14, f"[FAIL] wait_until 없음. steps={kinds14}"
    wu14 = next(s for s in plan14.steps if s.kind == "wait_until")
    assert isinstance(wu14.step_meta.get("iso_time"), str), (
        f"[FAIL] iso_time 없음. step_meta={wu14.step_meta!r}"
    )
    print("PASS [검증 14] 'wait for 20 minutes then ~' → wait_until + iso_time")

    # ------------------------------------------------------------------ #
    # 검증 15: wait_until — 영어 "after N hours"
    # ------------------------------------------------------------------ #
    plan15 = classify("after 1 hour send the completed report to the team")
    kinds15 = [s.kind for s in plan15.steps]
    assert "wait_until" in kinds15, f"[FAIL] wait_until 없음. steps={kinds15}"
    wu15 = next(s for s in plan15.steps if s.kind == "wait_until")
    parsed15 = datetime.fromisoformat(wu15.step_meta["iso_time"])
    if parsed15.tzinfo is None:
        parsed15 = parsed15.replace(tzinfo=datetime.now().astimezone().tzinfo)
    delta15 = parsed15 - datetime.now().astimezone()
    assert timedelta(minutes=55) <= delta15 <= timedelta(hours=1, minutes=5), (
        f"[FAIL] 1시간 후 iso_time delta={delta15}"
    )
    print("PASS [검증 15] 'after 1 hour ~' → wait_until + iso_time ≈ +1h")

    # ------------------------------------------------------------------ #
    # 검증 16: ask_user — 한국어 "묻고 나서"
    # ------------------------------------------------------------------ #
    plan16 = classify("선호도를 묻고 나서 그에 맞게 조사해줘")
    assert plan16.kind == "planned_workflow", f"[FAIL] kind={plan16.kind!r}"
    kinds16 = [s.kind for s in plan16.steps]
    assert "ask_user" in kinds16, f"[FAIL] ask_user 없음. steps={kinds16}"
    print("PASS [검증 16] '묻고 나서 ~' → ask_user 스텝")

    # ------------------------------------------------------------------ #
    # 검증 17: planned_workflow — 한국어 "매주 월요일"
    # ------------------------------------------------------------------ #
    plan17 = classify("매주 월요일 오전에 주간 보고서 자동으로 전송해줘")
    assert plan17.kind == "planned_workflow", f"[FAIL] kind={plan17.kind!r}"
    print("PASS [검증 17] '매주 월요일 ~' → planned_workflow")

    # ------------------------------------------------------------------ #
    # 검증 18: planned_workflow — 영어 "every Monday"
    # ------------------------------------------------------------------ #
    plan18 = classify("remind me every Monday to review the weekly metrics dashboard")
    assert plan18.kind == "planned_workflow", f"[FAIL] kind={plan18.kind!r}"
    print("PASS [검증 18] 'every Monday ~' → planned_workflow")

    # ------------------------------------------------------------------ #
    # 검증 19: planned_workflow — 영어 "monthly"
    # ------------------------------------------------------------------ #
    plan19 = classify("send a monthly summary report to the entire team automatically")
    assert plan19.kind == "planned_workflow", f"[FAIL] kind={plan19.kind!r}"
    print("PASS [검증 19] 'monthly ~' → planned_workflow")

    print("\n모든 검증 통과 ✓")


if __name__ == "__main__":
    try:
        run_smoke()
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
