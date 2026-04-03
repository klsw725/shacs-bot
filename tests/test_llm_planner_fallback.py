"""LLM 폴백 게이팅 로직 테스트 (비동기)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from shacs_bot.agent.loop import AgentLoop
from shacs_bot.providers.base import LLMProvider, LLMResponse


# ──────────────────────────────────────────────
# 헬퍼
# ──────────────────────────────────────────────


class _StubProvider(LLMProvider):
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


def _make_loop(provider: LLMProvider, workspace: Path) -> AgentLoop:
    return AgentLoop(
        bus=MagicMock(),
        provider=provider,
        workspace=workspace,
    )


def _valid_plan_json(kind: str = "planned_workflow") -> str:
    return json.dumps({
        "kind": kind,
        "summary": "리서치 후 결과 전달",
        "steps": [
            {"kind": "research", "description": "정보 수집", "depends_on": []},
            {"kind": "summarize", "description": "수집 내용 정리", "depends_on": [0]},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [1]},
        ],
    }, ensure_ascii=False)


_LONG_TEXT = "인공지능과 머신러닝의 차이점을 조사해서 비교 요약본으로 정리해줘"


# ──────────────────────────────────────────────
# 규칙 기반 → LLM 미호출
# ──────────────────────────────────────────────


async def test_rule_based_wait_until_no_llm_call(tmp_path: Path) -> None:
    stub = _StubProvider(_valid_plan_json())
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback("30분 후에 최신 날씨 정보를 조사해서 알려줘")
    assert plan.kind == "planned_workflow"
    assert stub.call_log == [], f"규칙 기반 케이스에서 LLM 호출됨: {len(stub.call_log)}회"


async def test_rule_based_clarification_no_llm_call(tmp_path: Path) -> None:
    stub = _StubProvider(_valid_plan_json())
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback("이거 해줘")
    assert plan.kind == "clarification"
    assert stub.call_log == [], "clarification에서 LLM 호출됨"


async def test_short_text_direct_answer_no_llm_call(tmp_path: Path) -> None:
    stub = _StubProvider(_valid_plan_json())
    loop = _make_loop(stub, tmp_path)
    short_text = "안녕하세요 반갑습니다"  # 10자
    assert len(short_text.strip()) < 30
    plan = await loop._classify_request_with_llm_fallback(short_text)
    assert plan.kind == "direct_answer"
    assert stub.call_log == [], "짧은 텍스트에서 LLM 호출됨"


async def test_rule_based_sequential_no_llm_call(tmp_path: Path) -> None:
    stub = _StubProvider(_valid_plan_json())
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(
        "먼저 전체 데이터를 수집하고 그 다음에 상세히 분석해서 보내줘"
    )
    assert plan.kind == "planned_workflow"
    assert stub.call_log == [], "규칙 기반 planned_workflow에서 LLM 호출됨"


# ──────────────────────────────────────────────
# LLM 폴백 호출
# ──────────────────────────────────────────────


async def test_nontrivial_request_calls_llm_and_returns_plan(tmp_path: Path) -> None:
    assert len(_LONG_TEXT.strip()) >= 30
    stub = _StubProvider(_valid_plan_json("planned_workflow"))
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "planned_workflow"
    assert len(stub.call_log) == 1, f"LLM 호출 횟수 오류: {len(stub.call_log)}"
    assert len(plan.steps) == 3


async def test_invalid_json_falls_back_to_direct_answer(tmp_path: Path) -> None:
    stub = _StubProvider("이건 JSON이 아닙니다 그냥 텍스트입니다 뭔가 잘못됨")
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "direct_answer"
    assert len(stub.call_log) == 1


async def test_empty_response_falls_back_to_direct_answer(tmp_path: Path) -> None:
    stub = _StubProvider("", finish_reason="stop")
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "direct_answer"


async def test_error_finish_reason_falls_back_to_direct_answer(tmp_path: Path) -> None:
    stub = _StubProvider("알 수 없는 오류", finish_reason="error")
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "direct_answer"


async def test_empty_steps_plan_falls_back_to_direct_answer(tmp_path: Path) -> None:
    empty_steps = json.dumps(
        {"kind": "planned_workflow", "summary": "test", "steps": []}, ensure_ascii=False
    )
    stub = _StubProvider(empty_steps)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "direct_answer"


async def test_markdown_wrapped_json_parsed(tmp_path: Path) -> None:
    md_wrapped = f"```json\n{_valid_plan_json()}\n```"
    stub = _StubProvider(md_wrapped)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "planned_workflow", f"마크다운 랩 파싱 실패: kind={plan.kind!r}"


async def test_llm_direct_answer_stays_direct_answer(tmp_path: Path) -> None:
    da_json = json.dumps({"kind": "direct_answer", "summary": "", "steps": []})
    stub = _StubProvider(da_json)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "direct_answer"
    assert len(stub.call_log) == 1, "LLM 호출됐어야 함"


# ──────────────────────────────────────────────
# _is_nontrivial_for_llm_fallback 경계
# ──────────────────────────────────────────────


def test_nontrivial_threshold_30_chars_true() -> None:
    assert AgentLoop._is_nontrivial_for_llm_fallback("a" * 30), "30자 → True이어야 함"


def test_nontrivial_threshold_29_chars_false() -> None:
    assert not AgentLoop._is_nontrivial_for_llm_fallback("a" * 29), "29자 → False이어야 함"


# ──────────────────────────────────────────────
# step_meta 보존 / 정규화
# ──────────────────────────────────────────────


async def test_wait_until_step_meta_preserved_from_llm(tmp_path: Path) -> None:
    wait_with_meta = json.dumps({
        "kind": "planned_workflow",
        "summary": "30분 후 실행",
        "steps": [
            {"kind": "wait_until", "description": "30분 대기", "depends_on": [],
             "step_meta": {"iso_time": "2026-04-03T15:00:00+09:00"}},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [0], "step_meta": {}},
        ],
    }, ensure_ascii=False)
    stub = _StubProvider(wait_with_meta)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "planned_workflow"
    wu = next(s for s in plan.steps if s.kind == "wait_until")
    assert wu.step_meta.get("iso_time") == "2026-04-03T15:00:00+09:00", (
        f"iso_time 보존 실패: {wu.step_meta!r}"
    )


async def test_wait_until_no_meta_normalized_from_description(tmp_path: Path) -> None:
    wait_no_meta = json.dumps({
        "kind": "planned_workflow",
        "summary": "30분 후 실행",
        "steps": [
            {"kind": "wait_until", "description": "30분 후에 실행", "depends_on": []},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [0]},
        ],
    }, ensure_ascii=False)
    stub = _StubProvider(wait_no_meta)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    assert plan.kind == "planned_workflow"
    wu = next(s for s in plan.steps if s.kind == "wait_until")
    iso = wu.step_meta.get("iso_time")
    assert isinstance(iso, str) and iso, (
        f"wait_until 정규화 실패: step_meta={wu.step_meta!r}"
    )


async def test_ask_user_step_meta_preserved_from_llm(tmp_path: Path) -> None:
    ask_with_meta = json.dumps({
        "kind": "planned_workflow",
        "summary": "사용자 입력 후 처리",
        "steps": [
            {"kind": "ask_user", "description": "입력 요청", "depends_on": [],
             "step_meta": {"prompt": "어떤 형식을 원하십니까?"}},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [0], "step_meta": {}},
        ],
    }, ensure_ascii=False)
    stub = _StubProvider(ask_with_meta)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    au = next(s for s in plan.steps if s.kind == "ask_user")
    assert au.step_meta.get("prompt") == "어떤 형식을 원하십니까?", (
        f"ask_user prompt 보존 실패: {au.step_meta!r}"
    )


async def test_ask_user_no_meta_normalized_from_description(tmp_path: Path) -> None:
    ask_no_meta = json.dumps({
        "kind": "planned_workflow",
        "summary": "사용자 입력 후 처리",
        "steps": [
            {"kind": "ask_user", "description": "원하는 형식을 알려주세요", "depends_on": []},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [0]},
        ],
    }, ensure_ascii=False)
    stub = _StubProvider(ask_no_meta)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    au = next(s for s in plan.steps if s.kind == "ask_user")
    assert au.step_meta.get("prompt") == "원하는 형식을 알려주세요", (
        f"ask_user prompt 정규화 실패: {au.step_meta!r}"
    )


async def test_request_approval_no_meta_normalized_from_description(tmp_path: Path) -> None:
    approval_no_meta = json.dumps({
        "kind": "planned_workflow",
        "summary": "승인 후 처리",
        "steps": [
            {"kind": "request_approval", "description": "파일을 삭제해도 될까요?", "depends_on": []},
            {"kind": "send_result", "description": "결과 전달", "depends_on": [0]},
        ],
    }, ensure_ascii=False)
    stub = _StubProvider(approval_no_meta)
    loop = _make_loop(stub, tmp_path)
    plan = await loop._classify_request_with_llm_fallback(_LONG_TEXT)
    ra = next(s for s in plan.steps if s.kind == "request_approval")
    assert ra.step_meta.get("prompt") == "파일을 삭제해도 될까요?", (
        f"request_approval prompt 정규화 실패: {ra.step_meta!r}"
    )
