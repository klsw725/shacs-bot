# Workflow Production Hardening

## 사용자 프롬프트

- "그럼 프로덕셕 품질이 되게 코드를 수정해"

## 변경 요약

- `shacs_bot/workflow/runtime.py` 수정
  - `start()`를 idempotent하게 변경
  - 이미 `running`인 workflow를 다시 시작해도 예외 없이 현재 record를 반환
  - `update_notify_target()` 추가
- `shacs_bot/agent/subagent.py` 수정
  - `execute_existing_workflow()`에서 thread limit를 먼저 검사
  - replay 전에 workflow를 `running`으로 claim
- `shacs_bot/workflow/redispatcher.py` 수정
  - cron/subagent redispatch false return을 warning으로 기록
- `shacs_bot/cli/commands.py` 수정
  - gateway에서 `agent_loop` 생성 후 `redispatcher`를 만들도록 순서 수정
  - heartbeat execute callback이 선택된 target을 workflow에 반영하도록 연결
- `shacs_bot/heartbeat/service.py` 수정
  - `on_execute(tasks, workflow_id)` 시그니처로 정렬
  - `trigger_now()`도 새 시그니처에 맞게 workflow를 생성 후 실행

## 왜 수정했는가

- gateway startup 시 `agent_loop` 생성 전에 `redispatcher`가 `agent_loop.subagent_manager`를 참조하던 순서 버그 제거
- subagent replay에서 duplicate dispatch 가능성을 줄이기 위해 replay 전에 workflow state를 `running`으로 claim
- redispatch 실패가 조용히 묻히지 않도록 warning 로그 추가
- heartbeat workflow도 실행 대상 channel/chat/session 정보를 기록할 수 있게 정렬

## 검증

- verifier 결과: production blockers 0, PASS
- `runtime.py`, `redispatcher.py`, `subagent.py` 진단 clean
- `heartbeat/service.py`는 warning만 남음 (기존 타입 스타일 경고 위주)
- 통합 스모크 테스트 수행
  - heartbeat 실행 완료 + notify metadata 반영
  - queued cron redispatch 완료
  - queued subagent redispatch 완료
