# E2E 스모크 테스트: planner → workflow → redispatch → executor

## 사용자 프롬프트

```
1. TASK: Implement the smallest safe end-to-end verification asset for planner→workflow→redispatch→executor flows covering `wait_until`, `ask_user`, and `request_approval`.
2. EXPECTED OUTCOME: A deterministic smoke/eval-style script (or minimal extension of an existing smoke asset) that exercises these three scenarios end-to-end: planner output creation, workflow registration, queued/running/waiting/retry state transitions, redispatch/resume, and final completion/failure as appropriate. Include the required dated docs work-log file under `docs/` with the user's prompt text.
3. REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash. Use bash only for targeted compile/smoke verification.
4. MUST DO: Reuse the current planner (`AgentLoop._classify_request()`), `WorkflowRuntime`, `WorkflowRedispatcher`, and existing smoke-script patterns. Keep this deterministic and lightweight: do not rely on live external services or flaky timing. Use dummy provider patterns already present in smoke scripts if needed. Cover at least these cases: (a) `wait_until` plan gets scheduled then resumes when due; (b) `ask_user` plan enters waiting state, consumes user reply, then completes; (c) `request_approval` plan waits for approval, handles approval/denial correctly, and does not consume arbitrary text as approval. Verify planner output and final workflow behavior in one path where practical.
5. MUST NOT DO: Do not refactor unrelated workflow or planner logic. Do not add a test framework. Do not commit. Do not change planner heuristics unless strictly required by the verification asset. Do not use type-suppression hacks.
6. CONTEXT: Recent work added step-based executor support, cursor persistence, ask_user resume, request_approval gating, wait_until time parsing, and structured step metadata. The remaining risk is combination regressions across the full planner→executor path, so this task is specifically about robust end-to-end verification.
```

## 변경 파일

- `scripts/smoke_e2e_planner_to_workflow.py` (신규)

## 배경

기존 단위 스모크 스크립트들은 각 기능을 개별적으로 검증하지만, 다음 통합 경로를 커버하지 않았습니다:

1. `_classify_request()` 출력 → `register_planned_workflow()` 저장 왕복
2. `WorkflowRedispatcher._tick()` 이 queued manual 워크플로우를 실제로 dispatch
3. 세 가지 step kind 의 전체 흐름 (등록 → 대기 → 재개 → 완료/실패)

## 구현 방식

### 스텁 설계
- `_StubCronService`: `WorkflowRedispatcher` 생성자 요구사항 충족 (manual 워크플로우 테스트에서는 호출 안 됨)
- `_StubAgentLoop`: `execute_existing_workflow` 구현만 포함, 호출된 workflow ID를 `dispatched` 리스트에 기록

### 격리 전략
- `tempfile.TemporaryDirectory()` 내 테스트 그룹별 서브디렉토리 (`reg/`, `rd/`, `wu/`, `au/`, `ra/`, `ra_arb/`, `ra_deny/`) 사용
- 각 `WorkflowRuntime` 인스턴스가 독립된 스토어를 가지므로 크로스 오염 없음

### 비결정성 제거
- 실제 LLM 호출 없음: `_classify_request()`는 순수 regex 기반 정적 메서드
- 실제 시간 대기 없음: `next_run_at`을 과거 시각으로 직접 조작하여 만료 시뮬레이션
- `asyncio.run(redispatcher._tick())` 으로 폴링 루프 없이 단일 tick만 실행

## 검증 목록

| 번호 | 시나리오 | 검증 내용 |
|------|----------|-----------|
| 1 | wait_until 플랜 등록 | `_classify_request()` iso_time → `register_planned_workflow()` → metadata.plan.step_meta.iso_time 보존 |
| 2 | ask_user 플랜 등록 | `_classify_request()` prompt → metadata.plan.step_meta.prompt 보존 |
| 3 | request_approval 플랜 등록 | `_classify_request()` prompt → metadata.plan.step_meta.prompt 보존 |
| 4 | Redispatcher dispatch | `_tick()` 이 queued manual 워크플로우를 stub AgentLoop에 dispatch |
| 5-9 | wait_until 전체 흐름 | 등록(queued) → start(running) → schedule_retry(retry_wait) → next_run_at 만료 → recover_restart(queued) → complete(completed) |
| 10-14 | ask_user 전체 흐름 | 등록(queued) → start(running) → wait_for_input(waiting_input) → resume_with_user_answer(queued+cursor전진+userAnswer저장) → complete(completed) |
| 15-21 | request_approval 전체 흐름 | 등록 → waiting_input → 승인(queued+cursor전진+approvalDecision=approved) → completed; 거절 → failed; running에서 approve 불가 |

## 실행 결과

```
PASS [검증 1] wait_until planner 출력 → register_planned_workflow → iso_time 보존
PASS [검증 2] ask_user planner 출력 → register_planned_workflow → prompt 보존
PASS [검증 3] request_approval planner 출력 → register_planned_workflow → prompt 보존
PASS [검증 4] WorkflowRedispatcher._tick() → queued manual 워크플로우 dispatch
PASS [검증 5] wait_until 워크플로우 등록 → state=queued
PASS [검증 6] start() → state=running
PASS [검증 7] _execute_plan_step(wait_until) 시뮬레이션 → retry_wait + next_run_at 저장
PASS [검증 8] next_run_at 만료 → recover_restart → state=queued 복구
PASS [검증 9] wait_until 흐름 전체 완료 → state=completed
PASS [검증 10] ask_user 워크플로우 등록 → state=queued
PASS [검증 11] start() + cursor → currentStepIndex=0 (ask_user)
PASS [검증 12] _execute_plan_step(ask_user) 시뮬레이션 → state=waiting_input
PASS [검증 13] resume_with_user_answer → queued + cursor 전진 + userAnswer 저장
PASS [검증 14] ask_user 흐름 전체 완료 → state=completed
PASS [검증 15] request_approval 워크플로우 등록 → state=queued
PASS [검증 16] start() + cursor → currentStepIndex=1 (request_approval)
PASS [검증 17] _execute_plan_step(request_approval) 시뮬레이션 → state=waiting_input
PASS [검증 18] running 상태에서 approve_workflow 호출 → None (상태 보호)
PASS [검증 19] approve_workflow → queued + cursor 전진 + approvalDecision=approved
PASS [검증 20] request_approval 승인 흐름 완료 → state=completed
PASS [검증 21] 거절 → state=failed + last_error 기록 + failed 후 approve 불가

모든 E2E 검증 통과 ✓
```
