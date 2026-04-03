"""어시스턴트 플래너 데이터 모델 및 step taxonomy.

M1: 플래너 데이터 모델 정의. AgentLoop 분기 또는 실행 로직은 포함하지 않는다.
"""

from __future__ import annotations

from typing import ClassVar, Literal

from pydantic import BaseModel, ConfigDict, Field


# ---------------------------------------------------------------------------
# Taxonomy
# ---------------------------------------------------------------------------

StepKind = Literal[
    "ask_user",
    "research",
    "summarize",
    "wait_until",
    "request_approval",
    "send_result",
]
"""단일 plan step의 종류."""

PlanKind = Literal["direct_answer", "clarification", "planned_workflow"]
"""플래너가 내리는 라우팅 결정 종류.

- ``direct_answer``: 추가 계획 없이 즉답 가능.
- ``clarification``: 계속 진행하기 전에 사용자 확인이 필요.
- ``planned_workflow``: step 기반 계획을 생성하여 순서대로 실행.
"""


# ---------------------------------------------------------------------------
# Models
# ---------------------------------------------------------------------------


class _PlannerBase(BaseModel):
    model_config: ClassVar[ConfigDict] = ConfigDict(frozen=True)


class PlanStep(_PlannerBase):
    """AssistantPlan 내 단일 실행 단계."""

    kind: StepKind
    description: str
    depends_on: list[int] = Field(default_factory=list)
    """이 step 시작 전 완료되어야 하는 선행 step의 인덱스 목록 (0-based)."""
    notify: bool = False
    """이 step 완료 시 사용자에게 알릴지 여부."""
    step_meta: dict[str, object] = Field(default_factory=dict)
    """executor가 description 파싱 대신 우선적으로 소비하는 구조화 메타데이터.

    ``wait_until`` 전용 키:
    - ``iso_time``        (str)   — 절대 재시도 시각 (ISO 8601)
    - ``duration_minutes`` (int | float) — 현재 시각 기준 대기 시간(분)

    ``ask_user`` / ``request_approval`` 전용 키:
    - ``prompt``          (str)   — 사용자에게 노출할 메시지 (미설정 시 description 사용)
    """


class AssistantPlan(_PlannerBase):
    """플래너가 생성하는 요청의 처리 계획."""

    kind: PlanKind
    steps: list[PlanStep] = Field(default_factory=list)
    """``planned_workflow`` 일 때만 사용되는 step 목록."""
    clarification_question: str | None = None
    """``clarification`` 일 때 사용자에게 보낼 질문."""
    summary: str = ""
    """계획에 대한 간략한 설명 (선택)."""


class ClarificationResult(_PlannerBase):
    """사용자 응답으로 clarification이 해소된 결과."""

    question: str
    """원래 clarification 질문."""
    answer: str
    """사용자의 응답."""
    original_request: str
    """clarification이 발생한 원본 요청."""
