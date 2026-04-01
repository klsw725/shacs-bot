# Lifecycle Hooks 예제 핸들러 추가

## 사용자 프롬프트

```text
1
```

## 변경 내용

- `shacs_bot/agent/hooks.py`
  - `register_example_hooks()` 추가
  - built-in example logging hook 추가
  - 기본 예제는 observer-only이며 다음 이벤트에 등록됨
    - `session_loaded`
    - `after_tool_execute`
    - `approval_resolved`
    - `after_outbound_send`
    - `heartbeat_decided`
    - `background_job_completed`
  - `redact_payloads=True`일 때 허용된 소수 필드만 로그에 남기도록 구성
- `shacs_bot/cli/commands.py`
  - gateway / agent 진입점에서 `hooks.enabled`가 켜져 있으면 example hooks 자동 등록

## 설계 의도

- 새 plugin/discovery 시스템 없이 기존 `hooks.enabled`를 그대로 gate로 사용
- mutation 예제가 아니라 운영 관측용 observer 예제로 제한
- payload는 기존 `redact_payloads` 설정을 존중

## 검증

- `uv run python` 스모크 테스트 통과
  - example hooks 등록 확인
  - `session_loaded`, `approval_resolved` 로그 발생 확인
  - `redact_payloads=True`일 때 raw 필드가 로그에 노출되지 않는 것 확인

## 메모

- `commands.py`의 basedpyright 오류는 기존 파일에 이미 존재하던 것들이다.
- 이번 변경과 직접 관련된 example hook registration/runtime smoke는 통과했다.
