# ask_user Resume 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> 3
>
> 진행시켜

---

## 작업 내용

- `ask_user` / `request_approval` step 진입 시 세션 metadata에 `waiting_workflow_id` 저장
- 다음 일반 인바운드 메시지에서 같은 세션의 `waiting_workflow_id`를 감지해 답변으로 소비
- `WorkflowRuntime.resume_with_user_answer()` 추가
  - `waiting_input` → `queued`
  - `userAnswer`, `lastStepResultSummary` 저장
  - `currentStepIndex`, `currentStepKind`를 다음 step 기준으로 전진
- `scripts/smoke_ask_user_resume.py` 추가

## 검증

- `uv run python scripts/smoke_ask_user_resume.py`
- `uv run python scripts/smoke_step_cursor.py`
- `uv run python -m py_compile shacs_bot/agent/loop.py shacs_bot/workflow/runtime.py scripts/smoke_ask_user_resume.py scripts/smoke_step_cursor.py`

## 메모

- 재개 자체는 메시지 처리 시점에 `queued`로 복원하고, 실제 다음 step 실행은 기존 `WorkflowRedispatcher`가 이어받는 구조를 유지했다.
- non-waiting 세션의 일반 메시지 처리 경로는 바꾸지 않았다.
