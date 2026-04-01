# Lifecycle Hooks 구현

## 사용자 프롬프트

```text
구현 가자
```

## 변경 요약

- `shacs_bot/agent/hooks.py` 신규 추가
  - `HookRegistry`, `NoOpHookRegistry`, `HookContext`
  - lifecycle 이벤트 상수 정의
  - handler 순차 실행 + 예외 격리
- `shacs_bot/config/schema.py`
  - `HooksConfig` 추가
  - 루트 `Config`에 `hooks` 설정 추가 (`enabled` 기본값 `False`)
- `shacs_bot/agent/loop.py`
  - hooks 주입
  - `before_context_build`, `before_llm_call`, `after_llm_call` emit 연결
- `shacs_bot/agent/tools/registry.py`
  - hooks 주입
  - `before_tool_execute`, `after_tool_execute` emit 연결
- `shacs_bot/channels/base.py`
  - ACL 통과 후 `message_received` emit 연결
  - `set_hooks()` 추가
- `shacs_bot/channels/manager.py`
  - hooks 주입
  - `before_outbound_send`, `after_outbound_send` emit 연결
  - outbound mutation은 `content`, `media`만 반영
- `shacs_bot/agent/approval.py`
  - hooks 주입
  - `approval_requested`, `approval_resolved` emit 연결
- `shacs_bot/heartbeat/service.py`
  - hooks 주입
  - `heartbeat_decided`, `background_job_completed` emit 연결
- `shacs_bot/agent/subagent.py`
  - shared hooks를 subagent `ToolRegistry`/`ApprovalGate`에 연결
  - `ApprovalGate` 호출 인자 불일치(`entity_name`/`entity_type`)를 `skill_name`으로 정리
- `shacs_bot/cli/commands.py`
  - gateway/agent 진입점에서 hooks registry 생성 및 주입

## 검증

- `uv run python` import smoke test 통과
- hook smoke test 통과
  - handler 순차 실행 확인
  - handler 예외가 삼켜지고 계속 진행되는 것 확인
  - `NoOpHookRegistry` no-op 확인
  - `Config().hooks.enabled is False` 기본값 확인

## 메모

- basedpyright 진단에는 기존 파일들에 이미 존재하던 경고/오류가 다수 남아 있음
- 이번 작업에서는 Lifecycle Hooks 구현과 직접 관련된 wiring 및 runtime smoke 검증에 집중함
