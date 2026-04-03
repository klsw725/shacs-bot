# request_approval Resume 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> 진행

---

## 작업 내용

- `request_approval`를 `ask_user`와 분리
- 세션 metadata에 `waiting_workflow_approval_id` 추가
- 승인 응답은 `y`, `yes`, `승인`만 허용
- 거절 응답은 `n`, `no`, `거절`만 허용
- 일반 텍스트는 approval 응답으로 소비하지 않고 안내 메시지만 반환
- `WorkflowRuntime.approve_workflow()` 추가
  - `approvalDecision=approved`
  - `currentStepIndex`, `currentStepKind`를 다음 step 기준으로 전진
- `scripts/smoke_request_approval.py` 추가

## 검증

- `uv run python scripts/smoke_request_approval.py`
- `uv run python scripts/smoke_ask_user_resume.py`
- `uv run python scripts/smoke_step_cursor.py`
- `uv run python -m py_compile shacs_bot/agent/loop.py shacs_bot/workflow/runtime.py scripts/smoke_request_approval.py scripts/smoke_ask_user_resume.py scripts/smoke_step_cursor.py`

## 메모

- 기존 subagent tool approval(`agent/approval.py`) 경로는 건드리지 않았다.
- `ask_user`의 자유 입력 재개 경로는 유지하고, `request_approval`만 별도 승인 게이트로 분리했다.
