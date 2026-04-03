# Planned Workflow Executor M1 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> 2

---

## 작업 내용

- `WorkflowRuntime`에 step metadata helper 추가
  - `update_step_cursor`
  - `annotate_step_result`
  - `clear_step_cursor`
- `AgentLoop.execute_existing_workflow()`가 `metadata.plan`을 읽어 step executor 경로로 진입하도록 확장
- 최소 3-step path 구현
  - `research`
  - `summarize`
  - `send_result`
- 대기형 step의 명시 처리 추가
  - `ask_user`, `request_approval` → `waiting_input`
  - `wait_until` → `retry_wait`

## 설계 메모

- 기존 manual workflow fallback(goal 재실행)은 plan parse 실패 또는 step 부재 시 유지
- metadata 키는 기존 workflow 패턴에 맞춰 camelCase 사용
- 결과 전달은 `_publish_workflow_outbound()`로 묶고 `mark_notified()`도 함께 기록

## 검증 계획

- changed file compile
- 3-step happy path smoke
- ask_user/request_approval waiting_input 전이 smoke
- wait_until retry_wait 전이 smoke
