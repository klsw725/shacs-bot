# Lifecycle Hooks 검증 완료

## 사용자 프롬프트

```text
3
```

## 작업 내용

Lifecycle Hooks 구현 이후 남아 있던 PRD 검증 시나리오를 실제 런타임 스모크 체크로 마무리했다.

추가 반영한 보완점:

- `session_loaded` 이벤트 추가 및 `AgentLoop._process_message()` 양 경로에서 emit
- tool hook emit에 `session_key`, `channel` 포함
- auto approval 경로도 `approval_resolved`로 관측 가능하도록 보강
- `HooksConfig`에 다음 설정 추가
  - `outbound_mutation_enabled`
  - `redact_payloads`
- `ChannelManager`에서 outbound mutation 적용을 `outbound_mutation_enabled`로 가드

## 검증 결과

다음 시나리오를 `uv run python` 스모크 스크립트로 검증했다.

- hooks 비활성화 기본값 확인
  - `hooks.enabled == False`
  - `hooks.outbound_mutation_enabled == False`
  - `hooks.redact_payloads == True`
  - `NoOpHookRegistry.emit()` no-op 확인
- inbound `message_received` 이벤트 발생 확인
- `session_loaded`, `before_context_build`, `before/after_llm_call`, `before/after_tool_execute` 발생 확인
- tool hook context에 `session_key`, `channel` 전달 확인
- outbound mutation 비활성화 시 변경 미적용 확인
- outbound mutation 활성화 시 `content`, `media`만 반영되고 `chat_id`, `metadata`는 유지됨 확인
- failing hook handler가 있어도 outbound send가 계속 진행됨 확인
- manual approval의 `approval_requested` / `approval_resolved` 확인
- auto approval의 `approval_resolved` 확인
- heartbeat의 `heartbeat_decided` / `background_job_completed` 확인

## 문서 반영

- `docs/specs/lifecycle-hooks/spec.md` 검증 기준 체크 완료
- `docs/specs/lifecycle-hooks/prds/lifecycle-hooks.md` 마일스톤/Acceptance Criteria 체크 완료

## 메모

- basedpyright에는 기존 코드베이스 전반의 사전 존재 경고/오류가 남아 있다.
- 이번 검증에서는 Lifecycle Hooks 관련 런타임 동작과 새로 추가한 emit/config 경로를 중심으로 확인했다.
