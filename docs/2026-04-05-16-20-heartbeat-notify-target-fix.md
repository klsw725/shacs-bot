# Heartbeat notify target 고정 및 bookkeeping 수정

**날짜**: 2026-04-05

## 사용자 프롬프트

> 진행해

## 작업 맥락

직전 리뷰에서 heartbeat workflow replay는 연결되었지만, 다음 두 문제가 블로킹으로 확인되었다.

1. `cli/direct` fallback이 실제 전송 없이 `notifyDelivered=True`로 기록될 수 있음
2. redispatch된 heartbeat workflow가 저장된 `notify_target`이 아니라 현재 선택된 최신 세션으로 다시 바인딩될 수 있음

## 변경 요약

### `shacs_bot/cli/commands.py`
- heartbeat 실행 시 `workflow_runtime.store.get(workflow_id)`로 기존 레코드를 읽고, 저장된 외부 `notify_target`이 있으면 그것을 우선 사용하도록 변경
- 저장된 외부 대상이 없을 때만 현재 라우팅 가능한 외부 채널을 선택하고, 그 경우에만 `update_notify_target()`로 저장
- 외부 대상이 전혀 없으면 내부 실행용 `cli:direct` fallback으로만 실행하고, notify target으로는 저장하지 않음
- `on_heartbeat_notify()`가 더 이상 대상을 다시 고르지 않고, 저장된 `notify_target`만 사용해 실제 전달 성공 시 `(channel, chat_id)`를 반환하도록 변경

### `shacs_bot/heartbeat/service.py`
- `on_notify` 콜백 시그니처를 `(response, workflow_id) -> tuple[str, str] | None`로 변경
- `_record_notification()`을 추가해 실제 전송 결과가 있을 때만 `mark_notified()`, 아니면 `mark_notify_delegated()`를 기록
- `execute_existing_workflow()`가 `queued` 상태의 heartbeat workflow만 재실행하도록 강화

### `shacs_bot/workflow/runtime.py`
- `update_notify_target()`가 dict 대신 `NotifyTarget` 모델을 저장하도록 변경해 타입/직렬화 일관성을 유지

### `tests/test_heartbeat_workflow.py`
- non-queued heartbeat replay 거부 테스트 추가
- `update_notify_target()`가 `NotifyTarget` 모델을 유지하는지 검증 추가
- direct/delegated bookkeeping 테스트를 새 `on_notify` 계약에 맞게 업데이트

## 검증

- `uv run pytest tests/test_heartbeat_workflow.py tests/test_step_cursor.py tests/test_ask_user_resume.py tests/test_request_approval.py tests/test_wait_until.py tests/test_e2e_planner_to_workflow.py -k "redispatcher or heartbeat or step_cursor or ask_user or request_approval or wait_until"`
- 결과: **39 passed**

## 비고

- `commands.py`에는 이번 변경과 무관한 기존 basedpyright 진단 오류/경고가 다수 남아 있음
- 이번 수정 범위의 신규/변경 테스트 파일(`tests/test_heartbeat_workflow.py`)과 runtime 변경(`shacs_bot/workflow/runtime.py`)은 diagnostics clean 상태를 확인함
