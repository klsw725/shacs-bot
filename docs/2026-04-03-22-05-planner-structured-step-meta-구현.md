# Planner Structured Step Metadata 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> 진행

---

## 작업 내용

- `PlanStep`에 `step_meta` 필드 추가
  - `wait_until`: `iso_time`, `duration_minutes`
  - `ask_user`, `request_approval`: `prompt`
- executor가 `description`보다 `step_meta`를 우선 소비하도록 반영
- rule-based planner가 다음 패턴에서 structured metadata를 실제로 채우도록 확장
  - `wait_until`
  - `request_approval`
  - `ask_user`
- `scripts/smoke_planner_metadata.py` 추가
  - planner 출력 자체에서 `step_meta` 생성 여부 검증

## 검증

- `uv run python scripts/smoke_planner_metadata.py`
- `uv run python scripts/smoke_wait_until.py`
- `uv run python scripts/smoke_request_approval.py`
- `uv run python scripts/smoke_ask_user_resume.py`
- `uv run python scripts/smoke_step_cursor.py`
- `uv run python -m py_compile shacs_bot/agent/planner.py shacs_bot/agent/loop.py scripts/smoke_planner_metadata.py`

## 메모

- 기존 generic planner 경로는 유지하고, 특수 의도(`wait_until`, `request_approval`, `ask_user`)만 최소 heuristic으로 분기했다.
- 기존 저장 플랜과 description fallback은 그대로 유지해 하위 호환성을 보존했다.
