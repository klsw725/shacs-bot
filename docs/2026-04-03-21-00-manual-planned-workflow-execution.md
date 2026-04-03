# Manual Planned Workflow 실행 경로 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> branch 새로 파서 진행해

---

## 구현 지시 프롬프트

> 1. TASK: Implement the smallest codebase-consistent execution path so `manual` planned workflows created by the assistant planner are actually consumed and run, instead of remaining indefinitely queued.
> 2. EXPECTED OUTCOME: A minimal multi-file change where a `planned_workflow` request can create a workflow record and then be executed through an existing runtime/redispatch path, with state transitions and user-visible result delivery working end-to-end. Include the required dated docs work-log file under `docs/` with the user's prompt text.
> 3. REQUIRED TOOLS: read, grep, lsp_diagnostics, apply_patch. Use bash only if you truly need a project command for verification. No web tools.
> 4. MUST DO: Read and use these files as primary context before editing: `shacs_bot/agent/loop.py`, `shacs_bot/agent/planner.py`, `shacs_bot/workflow/runtime.py`, `shacs_bot/workflow/redispatcher.py`, `shacs_bot/workflow/models.py`, `shacs_bot/cli/commands.py`, and the assistant workflow planner docs under `docs/specs/assistant-workflow-planner/` plus the dated work logs on 2026-04-03. Keep changes surgical. Reuse existing workflow runtime and redispatcher patterns rather than inventing a new subsystem. Preserve current direct_answer / clarification / planned_workflow behavior. Ensure manual planned workflows no longer stall at `queued` with no consumer. Run diagnostics on changed Python files.
> 5. MUST NOT DO: Do not refactor unrelated workflow infrastructure. Do not change planner classification heuristics unless strictly required. Do not add heavyweight frameworks. Do not commit. Do not touch unrelated specs or docs. Do not use type-suppression hacks.
> 6. CONTEXT: Current evidence shows `AgentLoop` stores `current_plan` and calls `WorkflowRuntime.register_planned_workflow(...)`, but `WorkflowRedispatcher` only executes `subagent` and `cron` sources and skips `manual`. The PRD/work log explicitly notes planned workflow execution is still missing beyond record creation. Goal is to close that gap with the minimal safe implementation that fits existing patterns.

---

## 문제

`AgentLoop._classify_request()` 가 `planned_workflow`로 분류된 요청을 처리할 때:

1. `WorkflowRuntime.register_planned_workflow()` 로 레코드 생성 (`source_kind="manual"`, `state="queued"`)
2. 워크플로우 ID를 사용자에게 반환

여기서 멈춤. `WorkflowRedispatcher._tick()` 은 `subagent` / `cron` 만 처리하고 `manual` 을 무시해 레코드가 영구 `queued` 상태로 잔류.

---

## 변경 내용

### `shacs_bot/workflow/redispatcher.py`

- `__init__` 에 `agent_loop: AgentLoop | None = None` 파라미터 추가 (`TYPE_CHECKING` 임포트)
- `_tick()` 에 `source_kind == "manual"` 브랜치 추가: `agent_loop.execute_existing_workflow(workflow_id)` 호출

### `shacs_bot/agent/loop.py`

- `execute_existing_workflow(workflow_id: str) -> bool` 메서드 추가
  - `WorkflowRuntime.start()` 로 `running` 전이
  - `asyncio.create_task()` 로 비동기 실행: `_run_agent_loop()` → 결과 annotate → `complete()` / `fail()`
  - `_bus.publish_outbound()` 로 사용자에게 결과 전달
  - `source_kind != "manual"` 인 경우 False 반환 (안전 가드)

### `shacs_bot/cli/commands.py`

- `WorkflowRedispatcher` 생성 시 `agent_loop=agent_loop` 주입

---

## 설계 포인트

- `subagent` / `cron` 패턴을 그대로 따름: `execute_existing_workflow(id) -> bool` 인터페이스, TYPE_CHECKING 임포트
- 분류 재실행 없음: `execute_existing_workflow` 는 `_run_agent_loop()` 를 직접 호출해 `_classify_request()` 를 우회
- 기존 `direct_answer` / `clarification` / `planned_workflow` 분기 영향 없음
- 실패 시 `WorkflowRuntime.fail()` 전이 후 사용자에게 오류 메시지 전달

---

## 검증

- Python AST 파싱: 3개 파일 모두 PASS
- 임포트 확인: `WorkflowRedispatcher`, `AgentLoop` 모두 정상 로드
- 리디스패치 스모크 테스트: queued manual 레코드 → `_tick()` → `execute_existing_workflow` 호출 확인
- 실행 경로 단위 테스트: `queued` → `running` → `completed` 전이, `publish_outbound` 호출 확인
