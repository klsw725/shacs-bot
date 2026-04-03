# request_approval 독립 승인 게이트 구현

## 날짜
2026-04-03

## 사용자 프롬프트
```
1. TASK: Implement the smallest safe follow-up so `request_approval` no longer shares the same free-form reply path as `ask_user`.
2. EXPECTED OUTCOME: A surgical code change where `request_approval` enters a distinct waiting/resume path, only explicit approval/denial replies (`y/n`, `yes/no`, `승인/거절`) are consumed, approval advances to the next unfinished step, denial ends the workflow explicitly, and normal non-approval text does not get mistaken as approval. Include the required dated docs work-log file under `docs/` with the user's prompt text.
3. REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash. Use bash only for targeted compile/smoke verification.
4. MUST DO: Reuse existing patterns in `shacs_bot/agent/approval.py`, `shacs_bot/agent/loop.py`, and `shacs_bot/workflow/runtime.py`. Preserve the current `ask_user` resume behavior exactly for non-approval waiting steps. Keep metadata keys camelCase and Python/session metadata naming consistent with existing code. Add at least one deterministic smoke verification for: `request_approval` enters waiting state -> explicit approval resumes to next step without re-asking -> explicit denial stops the workflow -> arbitrary text does not count as approval. Add/update the smallest runtime helper(s) needed instead of broad refactors.
5. MUST NOT DO: Do not refactor unrelated workflow/planner code. Do not add a test framework. Do not change planner heuristics. Do not commit. Do not break existing pending tool approval handling in `agent/approval.py`. Do not suppress type errors.
6. CONTEXT: Current code stores `waiting_workflow_id` for both `ask_user` and `request_approval`, and `resume_with_user_answer()` lets any next message advance the workflow. We need a distinct approval gate semantics now.
```

## 변경 내용

### 문제
이전 구현에서 `ask_user`와 `request_approval`가 동일한 `if step.kind in {"ask_user", "request_approval"}:` 블록으로 처리되어, 두 스텝 모두 `waiting_workflow_id`를 세션에 저장하고 `resume_with_user_answer()`로 임의 텍스트가 재개할 수 있었음. `request_approval`은 명시적 y/n 만 받아야 함.

### 해결 방법 (최소 변경)

**`shacs_bot/workflow/runtime.py`**
- `approve_workflow(workflow_id)` 메서드 추가
  - `waiting_input` 상태 전용 (다른 상태에선 `None` 반환)
  - `currentStepIndex` +1 전진 (request_approval 스텝 건너뜀)
  - `approvalDecision="approved"` 메타데이터 기록
  - `waiting_input → queued` 전환

**`shacs_bot/agent/loop.py` - `_execute_plan_step`**
- 기존 `if step.kind in {"ask_user", "request_approval"}:` 블록을 두 개로 분리:
  - `ask_user`: `waiting_workflow_id` 세션 메타 저장 (기존 동작 유지)
  - `request_approval`: `waiting_workflow_approval_id` 세션 메타 저장 + 승인/거절 안내 메시지

**`shacs_bot/agent/loop.py` - `_process_message`**
- `waiting_workflow_id` 체크 직전에 `waiting_workflow_approval_id` 체크 삽입
- `y/yes/승인` → `approve_workflow()` 호출 → 확인 메시지 반환
- `n/no/거절` → `fail()` 호출 → 종료 메시지 반환
- 그 외 텍스트 → 워크플로우 상태 변경 없이 알림 메시지만 반환
- 워크플로우가 더 이상 `waiting_input`이 아니면 세션 메타만 정리

### 새 파일
- `scripts/smoke_request_approval.py`: 7개 검증 (승인·거절·상태보호·ask_user 회귀)

## 검증 결과

```
smoke_request_approval.py:
PASS [검증 1-a] 잘못된 id 는 None 반환
PASS [검증 1-b] running 상태 → approve_workflow None 반환
PASS [검증 2] request_approval 스텝 → waiting_input 진입
PASS [검증 3] 승인 → queued + cursor=2 + approvalDecision=approved
PASS [검증 4] 재디스패치 후 request_approval 재질의 없이 send_result 시작
PASS [검증 5] 거절 → failed + last_error 기록
PASS [검증 6] 임의 텍스트로는 approve_workflow 진입 불가 (상태 보호)
PASS [검증 7] ask_user resume_with_user_answer 경로 회귀 없음

smoke_ask_user_resume.py → 9/9 PASS (회귀 없음)
smoke_step_cursor.py → 4/4 PASS (회귀 없음)
import check → OK
```

## 수정된 파일 요약

| 파일 | 변경 종류 | 핵심 |
|------|-----------|------|
| `shacs_bot/workflow/runtime.py` | 메서드 추가 | `approve_workflow()` |
| `shacs_bot/agent/loop.py` | 2곳 수정 | `_execute_plan_step` 분리 + `_process_message` 승인 게이트 |
| `scripts/smoke_request_approval.py` | 신규 | 7-step 스모크 테스트 |
