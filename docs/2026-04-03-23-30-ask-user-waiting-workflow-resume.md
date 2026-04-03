# ask_user 대기 워크플로우 재개 구현

## 날짜
2026-04-03

## 사용자 프롬프트
```
1. TASK: Implement the smallest safe path so an `ask_user` waiting planned workflow can consume the next normal inbound user message from the same session and continue to the next unfinished step.
2. EXPECTED OUTCOME: A surgical change where (a) ask_user marks the session/workflow as waiting for user input, (b) the next regular user message in that same session is captured as the answer, stored in workflow metadata/step result, the workflow is resumed, and execution continues from the next step instead of being replanned from scratch. Include a dated docs work-log file under `docs/` with the user's prompt text.
3. REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash. Use bash only for targeted compile/smoke verification.
4. MUST DO: Reuse existing patterns in `shacs_bot/agent/loop.py`, `shacs_bot/workflow/runtime.py`, and session metadata handling. Preserve current direct_answer / clarification / planned_workflow behavior for non-waiting sessions. Keep workflow metadata keys camelCase and session metadata keys consistent with existing Python style. Maintain the existing manual fallback and waiting_input / retry_wait semantics. The resumed workflow must continue from the next step and use the user answer as the step result context. Add at least one deterministic smoke verification covering: planned workflow enters ask_user waiting state -> next inbound message resumes same workflow -> next step executes without re-asking.
5. MUST NOT DO: Do not refactor unrelated workflow or planner code. Do not add a full test framework. Do not commit. Do not change planner heuristics. Do not suppress type errors.
6. CONTEXT: Current code already has a step-based planned executor, next-step cursor persistence, and `ask_user` currently sets `waiting_input`. What is missing is consuming a user's subsequent normal message as the answer for that waiting step. Minimal likely integration points are `_process_message`, session metadata, and workflow runtime metadata updates/resume.
```

## 변경 내용

### 문제
`ask_user` 스텝 실행 시 워크플로우는 `waiting_input` 상태로 전환되고 메시지를 보내지만, 이후 동일 세션의 다음 사용자 메시지를 답변으로 소비하는 로직이 없었음. 결과적으로 다음 메시지는 새 플래닝으로 처리되어 워크플로우가 재개되지 않았음.

### 해결 방법 (최소 변경)

**`shacs_bot/workflow/runtime.py`**
- `resume_with_user_answer(workflow_id, *, answer)` 메서드 추가
  - `waiting_input` 상태인 워크플로우에만 동작
  - `currentStepIndex`를 +1 전진 (ask_user 스텝 건너뜀)
  - `userAnswer`, `lastStepResultSummary`에 답변 저장
  - `waiting_input → queued` 전환 (기존 ALLOWED_TRANSITIONS 에서 허용됨)
  - WorkflowRedispatcher 가 다음 폴링 때 자동 재실행

**`shacs_bot/agent/loop.py` - `_execute_plan_step`**
- `ask_user`/`request_approval` 스텝 실행 시 `session.metadata["waiting_workflow_id"] = workflow_id` 저장
- `session_key or f"{channel}:{chat_id}"` 로 effective session key 계산

**`shacs_bot/agent/loop.py` - `_process_message`**
- 슬래시 명령어 처리 이후, 메모리 통합 이전에 waiting workflow 체크 삽입
- `session.metadata.get("waiting_workflow_id")` 확인
- 해당 워크플로우가 `waiting_input` 상태면 현재 메시지를 답변으로 소비
- `resume_with_user_answer()` 호출 → session에서 `waiting_workflow_id` 제거 → 확인 메시지 반환
- 워크플로우가 이미 다른 상태라면 세션 메타 정리만 수행

### 새 파일
- `scripts/smoke_ask_user_resume.py`: 9개 검증 항목 포함 결정론적 스모크 테스트

## 검증 결과

```
PASS [검증 1] ask_user 스텝에 cursor 도달 (index=1)
PASS [검증 2] 워크플로우 waiting_input 상태 진입
PASS [검증 3] 잘못된 id 는 None 반환
PASS [검증 4] resume_with_user_answer 반환값 있음
PASS [검증 5] 워크플로우 queued 상태로 재전환 (재디스패치 가능)
PASS [검증 6] cursor가 ask_user 다음 스텝(2=send_result)으로 전진
PASS [검증 7] 사용자 답변이 userAnswer 메타데이터에 저장됨
PASS [검증 8] lastStepResultSummary 가 사용자 답변으로 설정됨
PASS [검증 9] 재디스패치 후 ask_user 재질의 없이 send_result 부터 시작

모든 검증 통과 ✓
```

기존 `smoke_step_cursor.py`도 정상 통과 확인.

## 수정된 파일 요약

| 파일 | 변경 종류 | 핵심 |
|------|-----------|------|
| `shacs_bot/workflow/runtime.py` | 메서드 추가 | `resume_with_user_answer()` |
| `shacs_bot/agent/loop.py` | 2곳 수정 | `_execute_plan_step` 세션 기록 + `_process_message` 인터셉트 |
| `scripts/smoke_ask_user_resume.py` | 신규 | 9-step 스모크 테스트 |
