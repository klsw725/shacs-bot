# Hybrid Planner Eval 확장

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> gogo

---

## 작업 내용

- `shacs_bot/evals/models.py`
  - `EvaluationCase.expected_planner_kind` 추가
  - `TraceArtifact.planner_kind`, `TraceArtifact.fallback_engaged` 추가
  - `EvaluationResult.planner_kind` 추가
- `shacs_bot/evals/runner.py`
  - `TraceCollector.on_planner_decision()` 구현
  - planner kind / fallback engaged를 trace에 기록
  - `_classify_status()`에서 `expected_planner_kind` 검증 추가
  - 결과에 planner kind 반영
- `shacs_bot/templates/evals/cases/planner-scenarios.json`
  - hybrid fallback 관련 케이스 추가
    - rule-based match로 fallback 미개입
    - fallback이 planned_workflow로 승격
    - fallback 후 direct_answer 안전 복귀

## 검증

- `uv run python scripts/smoke_llm_planner_fallback.py`
- `uv run python scripts/smoke_e2e_planner_to_workflow.py`
- `uv run python scripts/smoke_planner_metadata.py`
- `uv run python scripts/smoke_wait_until.py`
- `uv run python scripts/smoke_request_approval.py`
- `uv run python scripts/smoke_ask_user_resume.py`
- `uv run python scripts/smoke_step_cursor.py`
- `uv run python -m py_compile shacs_bot/agent/loop.py shacs_bot/evals/models.py shacs_bot/evals/runner.py scripts/smoke_llm_planner_fallback.py scripts/smoke_e2e_planner_to_workflow.py`

## 메모

- fallback 검증은 실제 LLM 의존 eval 대신, observer를 통한 planner decision trace 기록으로 보강했다.
- 기존 eval schema는 optional field 추가만으로 확장해 하위 호환을 유지했다.
