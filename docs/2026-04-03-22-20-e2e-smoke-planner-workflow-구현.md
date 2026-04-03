# E2E Smoke: Planner → Workflow 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> gogo

---

## 작업 내용

- `scripts/smoke_e2e_planner_to_workflow.py` 추가
- planner 출력부터 workflow 등록, redispatch, 상태 전이, 최종 완료/실패까지 한 번에 검증하는 E2E 스모크 추가
- 시나리오 3종 검증
  - `wait_until`
  - `ask_user`
  - `request_approval`
- `WorkflowRedispatcher._tick()`의 queued manual workflow dispatch도 함께 검증

## 검증

- `uv run python scripts/smoke_e2e_planner_to_workflow.py`
- `uv run python -m py_compile scripts/smoke_e2e_planner_to_workflow.py`

## 메모

- 기존 개별 smoke들은 유지하고, 이번 스크립트는 planner→workflow→executor 조합 회귀를 잡는 통합 검증 자산으로 추가했다.
- 외부 서비스나 실제 LLM 호출 없이, `_classify_request()`와 `WorkflowRuntime`/`WorkflowRedispatcher`를 재사용해 결정론적으로 구성했다.
