# Assistant Workflow Planner M3 — 플래닝 상태 저장 및 워크플로우 런타임 핸드오프

**날짜**: 2026-04-03  
**브랜치**: `feature/assistant-workflow-planner-m1`  
**스코프**: M3 전용, M4 제외

---

## 사용자 프롬프트

> H: 1. TASK: Implement Assistant Workflow Planner PRD M3 only on the current branch by persisting planning state in session metadata and handing planned workflows off into the existing workflow runtime.
> 2. EXPECTED OUTCOME: Make the smallest safe multi-file changes so `Session` can store/read `current plan` and `last planning result`, `AgentLoop` records planning results into the session, and `planned_workflow` requests create a workflow record in the existing workflow runtime with notify target + plan metadata. Add the required dated docs work-log file for this M3 work.
> ...

---

## 변경 파일

### `shacs_bot/workflow/runtime.py`

- `NotifyTarget` import 추가 (`WorkflowRecord`, `WorkflowState`와 함께)
- `WorkflowRuntime.register_planned_workflow(...)` 메서드 추가:
  - 인자: `goal`, `plan` (dict), `channel`, `chat_id`, `session_key`
  - `source_kind="manual"`, `NotifyTarget`으로 notify_target 구성
  - `metadata={"plan": plan_dict}` 로 플래너 산출물 포함
  - `store.create()` + `store.upsert_and_get()` 로 즉시 저장 후 반환

### `shacs_bot/agent/loop.py`

- `_process_message` 내 `_classify_request` 호출 직후:
  - `session.metadata["last_planning_result"] = _plan.model_dump()` — 모든 비-command 메시지에 대해 마지막 분류 결과 기록
  - `clarification` 분기: `session.metadata["current_plan"]` 저장 + `self._sessions.save(session)` (early return 전 영속화)
  - `planned_workflow` 분기: `session.metadata["current_plan"]` 저장 + `register_planned_workflow(...)` 호출 + `self._sessions.save(session)` + 응답에 워크플로우 ID 포함

---

## 설계 결정

| 결정 | 이유 |
|------|------|
| `session.metadata` 직접 사용 | `Session` 데이터클래스는 이미 `metadata: dict` 필드를 가지며 직렬화/저장 경로가 확립되어 있음. 단일 사용을 위해 별도 accessor 메서드를 추가하지 않음 |
| `register_planned_workflow`를 `WorkflowRuntime`에 추가 | 워크플로우 생성 로직을 loop.py에 직접 산포하지 않고, 기존 런타임 인터페이스 위에 집약 |
| `source_kind="manual"` | 사용자 채팅 입력을 통해 트리거된 워크플로우는 heartbeat/cron/subagent가 아닌 manual이 가장 정확 |
| early return 전 `_sessions.save` | `clarification`/`planned_workflow` 분기는 `_run_agent_loop`를 거치지 않아 기존 save 경로를 타지 않음 |
| M2 `_classify_request` 보존 | 분류 로직은 그대로 유지, M3는 결과 소비(consume)만 추가 |

---

## 검증

```
$ uv run python -c "from shacs_bot.workflow.runtime import WorkflowRuntime; ..."
→ register_planned_workflow 서명 확인, notify_target/metadata assertions 통과

$ uv run python -c "from shacs_bot.agent.loop import AgentLoop; ..."
→ 루프 임포트 성공

$ uv run python -c "(WorkflowRuntime 통합 테스트)"
→ workflow_id 생성, notify_target, metadata['plan'] 모두 정상
```

---

## 미구현 (M3 스코프 외)

- **M4 시나리오 테스트**: 별도 마일스톤
- **planned_workflow 실제 실행**: 전용 플래너 executor가 없으므로 레코드 생성(queued) 에서 멈춤. 향후 executor 구현 시 `state` 전환 담당
- **wait_until step 자동 스케줄링**: M4+ 범위
