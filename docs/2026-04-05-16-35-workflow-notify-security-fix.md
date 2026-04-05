# Workflow notify security fix

**날짜**: 2026-04-05

## 사용자 프롬프트

> 진행시켜

## 작업 맥락

재리뷰 후 남아 있던 보안 블로킹 이슈 2건을 수정했다.

1. outbound queue 적재만으로 `notifyDelivered=True`가 기록되던 문제
2. `manual_recover()`가 workflow owner/session 검증 없이 replay를 허용하던 문제

## 변경 요약

### `shacs_bot/workflow/runtime.py`
- `ManualRecoverStatus`에 `unauthorized` 추가
- `mark_notify_enqueued()` 추가
  - `notifyEnqueued=True`
  - `notifyChannel`, `notifyChatId`, `notifyEnqueuedAt` 기록
- `manual_recover()`에서 recover 권한 검증 추가
  - `cli` 채널은 로컬 사용자 경로로 허용
  - 그 외 채널은 persisted `notify_target.channel/chat_id`와 일치해야만 recover 허용

### `shacs_bot/heartbeat/service.py`
- heartbeat notify callback 반환 타깃은 실제 delivery가 아니라 outbound enqueue 결과로 간주
- `_record_notification()`이 `mark_notified()` 대신 `mark_notify_enqueued()`를 사용하도록 변경

### `shacs_bot/cli/commands.py`
- cron response outbound publish 후 `mark_notified()` 대신 `mark_notify_enqueued()` 사용
- CLI workflow recover 명령에서 `unauthorized` 상태 메시지 처리 추가

### `shacs_bot/agent/loop.py`
- `/workflow recover <id>` 경로에서 `unauthorized` 응답 처리 추가
- `_publish_workflow_outbound()`가 queue 적재 후 `mark_notify_enqueued()`를 사용하도록 변경

### 테스트
- `tests/test_heartbeat_workflow.py`
  - heartbeat outbound enqueue가 `notifyEnqueued`를 기록하는지 검증
- `tests/test_workflow_policy_runtime.py`
  - 다른 채팅의 manual recover 차단
  - CLI recover bypass 허용

## 검증

- `uv run pytest tests/test_heartbeat_workflow.py tests/test_workflow_policy_runtime.py tests/test_step_cursor.py tests/test_ask_user_resume.py tests/test_request_approval.py tests/test_wait_until.py tests/test_e2e_planner_to_workflow.py`
- 결과: **42 passed**

## 비고

- `commands.py`, `agent/loop.py`, `heartbeat/service.py`에는 이번 변경과 무관한 기존 basedpyright 경고/오류가 남아 있다.
- 이번 변경 범위에서 새로 추가/수정한 테스트 파일과 `workflow/runtime.py`는 diagnostics clean 상태를 확인했다.
