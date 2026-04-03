# 2026-04-03 — planned workflow step cursor 전진 (재디스패치/복구 시 완료 step 재실행 방지)

## 사용자 프롬프트

> Implement the smallest safe follow-up to the new planned-workflow executor so completed steps are not re-run after redispatch/recovery.
>
> EXPECTED OUTCOME: A surgical code change where successful step execution persists the next step cursor (not just the current step), plus a smoke/integration verification that proves redispatch/resume continues from the next unfinished step. Include the required dated docs work-log file under `docs/` with the user's prompt text.
>
> REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash. Use bash only for targeted compile/smoke verification.
>
> MUST DO: Use existing patterns in `shacs_bot/agent/loop.py` and `shacs_bot/workflow/runtime.py`. Preserve current 3-step executor behavior, waiting_input, retry_wait, and manual fallback. Keep metadata keys in camelCase. Add/adjust the smallest runtime helper(s) needed so that after a successful step the persisted metadata points at the next step index/kind. Add a deterministic smoke test command or script snippet that proves a queued redispatch resumes from the next step instead of re-running the completed one. Add a dated work log in `docs/` capturing the user prompt and what changed.
>
> MUST NOT DO: Do not refactor unrelated workflow code. Do not add a full test framework. Do not change planner heuristics. Do not commit. Do not introduce type-suppression hacks. Do not remove the existing fallback path.
>
> CONTEXT: We already added a step-based planned executor for `research -> summarize -> send_result` and basic waiting states. The next priority is making resume semantics real: if a step succeeds and the workflow is redispatched/recovered, it must continue from the next unfinished step rather than re-running completed work.

---

## 버그 설명

`shacs_bot/agent/loop.py`의 `_run_planned_workflow_steps`에서 step 성공 후:

1. `update_step_cursor(step_index=current_step_index)` 는 step **시작** 시 호출됨 (현재 index 기록)
2. step 완료 후 `current_step_index += 1` 은 **로컬 변수만** 증가
3. 메타데이터의 `currentStepIndex`는 방금 완료된 step을 계속 가리킴

따라서 워크플로우가 재큐(queued)되어 `execute_existing_workflow`가 호출되면, `_run_planned_workflow_steps`가 메타데이터에서 `currentStepIndex`를 읽어 이미 완료된 step부터 다시 실행.

---

## 변경 내용

### `shacs_bot/agent/loop.py` — 6줄 추가 (`current_step_index += 1` 교체)

```python
# 변경 전
_ = self._workflow_runtime.annotate_step_result(workflow_id, current_result)
_ = self._workflow_runtime.annotate_result(workflow_id, current_result)
current_step_index += 1

# 변경 후
_ = self._workflow_runtime.annotate_step_result(workflow_id, current_result)
_ = self._workflow_runtime.annotate_result(workflow_id, current_result)
next_idx = current_step_index + 1
next_kind = plan.steps[next_idx].kind if next_idx < len(plan.steps) else ""
_ = self._workflow_runtime.update_step_cursor(
    workflow_id,
    step_index=next_idx,
    step_kind=next_kind,
)
current_step_index = next_idx
```

- step 성공 직후 `currentStepIndex`를 `next_idx` (다음 step 번호)로 영속화
- `next_idx >= len(plan.steps)` 경계 처리: `step_kind=""` (while 루프 조건에서 즉시 탈출)
- `runtime.py`, `planner.py` 등 다른 파일은 무변경

### `scripts/smoke_step_cursor.py` — 신규 작성

재디스패치/복구 시 완료 step 재실행 방지를 검증하는 결정론적 스모크 스크립트.

```
uv run python scripts/smoke_step_cursor.py
```

4가지 검증 항목:
1. step 0 완료 후 메타데이터 cursor = 1
2. 재디스패치 후 시작 index = 1 (step 0 재실행 없음)
3. step 1 완료 후 메타데이터 cursor = 2
4. 두 번째 재디스패치 후 시작 index = 2 (step 0, 1 재실행 없음)

---

## 영향 범위

- `waiting_input`, `retry_wait`, manual fallback 경로 무변경
- `send_result` (outcome = "completed") 조기 반환 경로도 무변경 — cursor 전진 로직은 `outcome == "continue"` 블록 이후에만 실행됨
- `clear_step_cursor` + `complete` 정상 종료 경로 무변경
