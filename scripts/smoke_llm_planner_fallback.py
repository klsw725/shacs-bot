#!/usr/bin/env python3
"""
스모크 테스트: 하이브리드 플래너 LLM 폴백 게이팅 로직 검증.

실행 방법:
    uv run python scripts/smoke_llm_planner_fallback.py

테스트 항목:
1. 규칙 기반 케이스(wait_until, clarification 등)는 LLM을 호출하지 않음
2. 짧은 텍스트(<30자)는 LLM 폴백을 시도하지 않음
3. 비자명 요청(>=30자, direct_answer)에서 LLM 폴백이 호출됨
4. LLM이 유효한 planned_workflow JSON을 반환하면 AssistantPlan으로 파싱
5. LLM이 유효하지 않은 JSON을 반환하면 direct_answer로 안전 폴백
6. LLM이 빈 응답을 반환하면 direct_answer로 안전 폴백
7. _is_nontrivial_for_llm_fallback 임계값(30자) 경계 검증
"""
from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path
from typing import Any
from dataclasses import dataclass, field

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.agent.loop import AgentLoop
from shacs_bot.agent.planner import AssistantPlan, PlanStep
from shacs_bot.providers.base import LLMProvider, LLMResponse


# ---------------------------------------------------------------------------
# 스텁 프로바이더
# ---------------------------------------------------------------------------


class _StubProvider(LLMProvider):
    """테스트용 스텁 프로바이더. call_log로 호출 여부를 추적한다."""

    def __init__(self, response_content: str, finish_reason: str = "stop") -> None:
        super().__init__()
        self._response_content = response_content
        self._finish_reason = finish_reason
        self.call_log: list[list[dict]] = []

    async def chat(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        model: str | None = None,
        max_tokens: int = 4096,
        temperature: float = 0.7,
        reasoning_effort: str | None = None,
        tool_choice: str | dict[str, Any] | None = None,
    ) -> LLMResponse:
        self.call_log.append(messages)
        return LLMResponse(
            content=self._response_content,
            finish_reason=self._finish_reason,
        )

    def get_default_model(self) -> str:
        return "stub-model"


# ---------------------------------------------------------------------------
# 헬퍼
# ---------------------------------------------------------------------------


def _make_loop(provider: LLMProvider) -> AgentLoop:
    """AgentLoop 인스턴스를 최소 의존성으로 생성합니다."""
    from unittest.mock import MagicMock

    bus = MagicMock()
    return AgentLoop(
        bus=bus,
        provider=provider,
        workspace=Path("/tmp/shacs_smoke_test"),
    )


def _valid_plan_json(kind: str = "planned_workflow") -> str:
    plan = {
        "kind": kind,
        "summary": "리서치 후 결과 전달",
        "steps": [
            {"kind": "research", "description": "정보 수집", "depends_on": []},
            {"kind": "summarize", "description": "수집 내용 정리", "depends_on": [0]},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [1]},
        ],
    }
    return json.dumps(plan, ensure_ascii=False)


# ---------------------------------------------------------------------------
# 검증
# ---------------------------------------------------------------------------


async def run_smoke() -> None:
    # ------------------------------------------------------------------
    # 검증 1: 규칙 기반 케이스 → LLM 미호출
    # ------------------------------------------------------------------
    stub1 = _StubProvider(_valid_plan_json())
    loop1 = _make_loop(stub1)

    plan1 = await loop1._classify_request_with_llm_fallback("30분 후에 최신 날씨 정보를 조사해서 알려줘")
    assert plan1.kind == "planned_workflow", f"[FAIL] kind={plan1.kind!r}"
    assert stub1.call_log == [], f"[FAIL] 규칙 기반 케이스에서 LLM 호출됨: {len(stub1.call_log)}회"
    print("PASS [검증 1] 규칙 기반(wait_until) → LLM 미호출")

    # ------------------------------------------------------------------
    # 검증 2: 모호한 요청(clarification) → LLM 미호출
    # ------------------------------------------------------------------
    stub2 = _StubProvider(_valid_plan_json())
    loop2 = _make_loop(stub2)

    plan2 = await loop2._classify_request_with_llm_fallback("이거 해줘")
    assert plan2.kind == "clarification", f"[FAIL] kind={plan2.kind!r}"
    assert stub2.call_log == [], f"[FAIL] clarification에서 LLM 호출됨"
    print("PASS [검증 2] 규칙 기반(clarification) → LLM 미호출")

    # ------------------------------------------------------------------
    # 검증 3: 짧은 텍스트(<30자) direct_answer → LLM 미호출
    # ------------------------------------------------------------------
    stub3 = _StubProvider(_valid_plan_json())
    loop3 = _make_loop(stub3)

    short_text = "안녕하세요 반갑습니다"  # 10자
    assert len(short_text.strip()) < 30
    plan3 = await loop3._classify_request_with_llm_fallback(short_text)
    assert plan3.kind == "direct_answer", f"[FAIL] kind={plan3.kind!r}"
    assert stub3.call_log == [], f"[FAIL] 짧은 텍스트에서 LLM 호출됨"
    print("PASS [검증 3] 짧은 텍스트 direct_answer → LLM 미호출")

    # ------------------------------------------------------------------
    # 검증 4: 비자명 요청(>=30자) + LLM이 planned_workflow 반환 → 폴백 적용
    # ------------------------------------------------------------------
    stub4 = _StubProvider(_valid_plan_json("planned_workflow"))
    loop4 = _make_loop(stub4)

    long_text = "인공지능과 머신러닝의 차이점을 조사해서 비교 요약본으로 정리해줘"
    assert len(long_text.strip()) >= 30
    plan4 = await loop4._classify_request_with_llm_fallback(long_text)
    assert plan4.kind == "planned_workflow", f"[FAIL] kind={plan4.kind!r}"
    assert len(stub4.call_log) == 1, f"[FAIL] LLM 호출 횟수 오류: {len(stub4.call_log)}"
    assert len(plan4.steps) == 3, f"[FAIL] steps 개수: {len(plan4.steps)}"
    print("PASS [검증 4] 비자명 요청 → LLM 폴백 호출 → planned_workflow 파싱")

    # ------------------------------------------------------------------
    # 검증 5: LLM이 유효하지 않은 JSON 반환 → direct_answer 안전 폴백
    # ------------------------------------------------------------------
    stub5 = _StubProvider("이건 JSON이 아닙니다 그냥 텍스트입니다 뭔가 잘못됨")
    loop5 = _make_loop(stub5)

    plan5 = await loop5._classify_request_with_llm_fallback(long_text)
    assert plan5.kind == "direct_answer", f"[FAIL] 파싱 실패 시 kind={plan5.kind!r}"
    assert len(stub5.call_log) == 1, f"[FAIL] LLM 호출 횟수 오류: {len(stub5.call_log)}"
    print("PASS [검증 5] LLM 무효 JSON → direct_answer 안전 폴백")

    # ------------------------------------------------------------------
    # 검증 6: LLM이 빈 응답 반환 → direct_answer 안전 폴백
    # ------------------------------------------------------------------
    stub6 = _StubProvider("", finish_reason="stop")
    loop6 = _make_loop(stub6)

    plan6 = await loop6._classify_request_with_llm_fallback(long_text)
    assert plan6.kind == "direct_answer", f"[FAIL] 빈 응답 시 kind={plan6.kind!r}"
    print("PASS [검증 6] LLM 빈 응답 → direct_answer 안전 폴백")

    # ------------------------------------------------------------------
    # 검증 7: LLM이 error finish_reason 반환 → direct_answer 안전 폴백
    # (비 일시적 오류 사용 → 재시도 없이 즉시 반환)
    # ------------------------------------------------------------------
    stub7 = _StubProvider("알 수 없는 오류", finish_reason="error")
    loop7 = _make_loop(stub7)

    plan7 = await loop7._classify_request_with_llm_fallback(long_text)
    assert plan7.kind == "direct_answer", f"[FAIL] error finish_reason 시 kind={plan7.kind!r}"
    print("PASS [검증 7] LLM error finish_reason → direct_answer 안전 폴백")

    # ------------------------------------------------------------------
    # 검증 8: LLM이 planned_workflow + 빈 steps 반환 → direct_answer 안전 폴백
    # ------------------------------------------------------------------
    empty_steps_plan = json.dumps(
        {"kind": "planned_workflow", "summary": "test", "steps": []},
        ensure_ascii=False,
    )
    stub8 = _StubProvider(empty_steps_plan)
    loop8 = _make_loop(stub8)

    plan8 = await loop8._classify_request_with_llm_fallback(long_text)
    assert plan8.kind == "direct_answer", f"[FAIL] 빈 steps planned_workflow 시 kind={plan8.kind!r}"
    print("PASS [검증 8] LLM planned_workflow + 빈 steps → direct_answer 안전 폴백")

    # ------------------------------------------------------------------
    # 검증 9: LLM이 마크다운 코드 블록으로 감싼 JSON 반환 → 파싱 성공
    # ------------------------------------------------------------------
    md_wrapped = f"```json\n{_valid_plan_json()}\n```"
    stub9 = _StubProvider(md_wrapped)
    loop9 = _make_loop(stub9)

    plan9 = await loop9._classify_request_with_llm_fallback(long_text)
    assert plan9.kind == "planned_workflow", f"[FAIL] 마크다운 랩 파싱 실패: kind={plan9.kind!r}"
    print("PASS [검증 9] LLM 마크다운 코드블록 JSON → 파싱 성공")

    # ------------------------------------------------------------------
    # 검증 10: LLM이 direct_answer 반환 → 규칙 기반 결과(direct_answer) 유지
    # ------------------------------------------------------------------
    da_json = json.dumps({"kind": "direct_answer", "summary": "", "steps": []})
    stub10 = _StubProvider(da_json)
    loop10 = _make_loop(stub10)

    plan10 = await loop10._classify_request_with_llm_fallback(long_text)
    assert plan10.kind == "direct_answer", f"[FAIL] kind={plan10.kind!r}"
    assert len(stub10.call_log) == 1, "[FAIL] LLM 호출됐어야 함"
    print("PASS [검증 10] LLM direct_answer 반환 → 원래 direct_answer 유지")

    # ------------------------------------------------------------------
    # 검증 11: _is_nontrivial_for_llm_fallback 경계 — 정확히 30자
    # ------------------------------------------------------------------
    text_30 = "a" * 30
    text_29 = "a" * 29
    assert AgentLoop._is_nontrivial_for_llm_fallback(text_30), "[FAIL] 30자 → True 이어야 함"
    assert not AgentLoop._is_nontrivial_for_llm_fallback(text_29), "[FAIL] 29자 → False 이어야 함"
    print("PASS [검증 11] _is_nontrivial_for_llm_fallback 경계: 30자=True, 29자=False")

    # ------------------------------------------------------------------
    # 검증 12: 규칙 기반 planned_workflow(sequential) → LLM 미호출
    # ------------------------------------------------------------------
    stub12 = _StubProvider(_valid_plan_json())
    loop12 = _make_loop(stub12)

    plan12 = await loop12._classify_request_with_llm_fallback(
        "먼저 전체 데이터를 수집하고 그 다음에 상세히 분석해서 보내줘"
    )
    assert plan12.kind == "planned_workflow", f"[FAIL] kind={plan12.kind!r}"
    assert stub12.call_log == [], "[FAIL] 규칙 기반 planned_workflow에서 LLM 호출됨"
    print("PASS [검증 12] 규칙 기반(sequential) planned_workflow → LLM 미호출")

    print("\n모든 검증 통과 ✓")


if __name__ == "__main__":
    try:
        asyncio.run(run_smoke())
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
