# SPEC: Lifecycle Hooks

> **Prompt**: shacs-bot을 멀티채널 장시간 상주형 AI assistant로 운영하기 위해, 메시지/LLM/도구/전송/백그라운드 경계마다 내부 훅 시스템을 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`lifecycle-hooks.md`](./prds/lifecycle-hooks.md) | HookRegistry, emit 포인트, payload 규칙, 검증 단계를 구현 태스크로 분해 |

## TL;DR

> **목적**: core 코드에 매번 분기를 박지 않고도 정책, 감사, 후처리, 알림 자동화를 붙일 수 있는 확장 지점을 제공한다.
>
> **Deliverables**:
> - `shacs_bot/agent/hooks.py` — hook registry와 context 모델
> - `shacs_bot/agent/loop.py` — turn/LLM/tool emit 포인트
> - `shacs_bot/channels/base.py`, `shacs_bot/channels/manager.py` — inbound/outbound emit 포인트
> - `shacs_bot/agent/approval.py`, `shacs_bot/heartbeat/service.py` — approval/heartbeat emit 포인트
> - `docs/specs/lifecycle-hooks/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Medium (4-6시간)

## User Scenarios & Testing

### Scenario 1 - 운영자는 응답 전후 이벤트를 관측할 수 있다

운영자는 assistant가 메시지를 받고 응답을 보내는 핵심 단계마다 구조화된 이벤트를 볼 수 있어야 한다.

**테스트**: 메시지 수신, 도구 실행, 응답 전송이 순서대로 기록되는지 확인한다.

### Scenario 2 - 정책 훅이 assistant 응답을 깨뜨리지 않는다

정책/감사용 훅이 실패하더라도 사용자 응답은 계속 전달되어야 한다.

**테스트**: 예외를 던지는 hook handler를 등록해도 최종 응답이 정상 전송되는지 확인한다.

### Scenario 3 - 특정 채널 후처리를 hook으로 붙일 수 있다

운영자는 core 코드를 직접 수정하지 않고도 특정 채널의 footer, 감사 메타데이터, 알림 동작을 추가할 수 있어야 한다.

**테스트**: outbound 직전 hook이 특정 채널 payload만 수정하는지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 inbound, LLM 호출, tool 실행, outbound 전송, approval, heartbeat 완료 시점에 hook 이벤트를 발생시켜야 한다.
- **FR-002**: hook 미등록 상태에서는 기존 assistant 동작이 변경되지 않아야 한다.
- **FR-003**: hook handler 실패는 기록되어야 하지만 사용자 응답 경로를 중단시키면 안 된다.
- **FR-004**: hook payload는 session, channel, event, 관련 메타데이터를 구조화된 형태로 제공해야 한다.
- **FR-005**: 최소 하나의 이벤트는 payload 후처리(outbound 수정)를 지원해야 한다.

## Key Entities

- **Hook Event**: 특정 assistant lifecycle 경계에서 발생하는 이름 있는 이벤트
- **Hook Context**: event, session, channel, payload를 담는 구조화된 전달 객체
- **Hook Handler**: 이벤트를 수신해 감사/정책/후처리를 수행하는 실행 단위

## Success Criteria

- 100%의 핵심 실행 경계(inbound, llm, tool, outbound, approval, heartbeat)에 대해 최소 1개의 emit 포인트가 정의된다.
- hook 비활성화 상태에서 기존 주요 흐름이 회귀 없이 유지된다.
- hook handler 실패가 발생해도 사용자 응답 성공률이 저하되지 않는다.
- 운영자는 코드 수정 없이 감사/후처리용 hook을 추가할 수 있다.

## Assumptions

- 1단계 범위는 in-process Python hook만 다룬다.
- 외부 webhook, shell hook, 우선순위 시스템은 이번 범위 밖이다.
- payload 민감정보 최소화는 hook 설계의 기본 원칙으로 적용한다.

## 현재 상태 분석

- `channels/base.py`는 `_handle_message()`에서 권한 확인 후 바로 `MessageBus`로 전달한다.
- `agent/loop.py`는 세션/LLM/도구/메모리/응답을 직접 연결하지만, 외부 확장 지점은 거의 없다.
- `channels/manager.py`는 outbound dispatch 실패를 로그만 남긴다.
- `heartbeat/service.py`는 `skip/run` 결정과 notify 콜백을 갖고 있지만 이벤트 계층은 없다.

현재는 정책/감사/운영 자동화를 붙이려면 core 흐름에 직접 분기를 추가해야 한다. assistant가 장시간 운영될수록 이 방식은 유지보수 비용을 키운다.

## 설계

### 설계 원칙

1. **기본 동작 no-op** — 훅이 없어도 기존 동작 완전 유지
2. **내부 이벤트 우선** — 외부 인프라 없이 현재 앱 내부에서 동작
3. **실패 격리** — hook 예외가 사용자 응답 경로를 끊지 않음
4. **구조화된 context** — dict 덤프가 아니라 명시적 필드 제공

### 이벤트 목록

```text
message_received
session_loaded
before_context_build
before_llm_call
after_llm_call
before_tool_execute
after_tool_execute
before_outbound_send
after_outbound_send
approval_requested
approval_resolved
heartbeat_decided
background_job_completed
```

### 범위

- Python callback 기반 registry
- config 기반 on/off
- outbound 수정은 명시적으로 허용된 이벤트에 한해 지원
- audit/logging/policy/formatting 용도 우선

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/agent/hooks.py` | 신규 | HookRegistry, HookContext, emit helper |
| `shacs_bot/agent/loop.py` | 수정 | turn/LLM/tool emit 포인트 추가 |
| `shacs_bot/channels/base.py` | 수정 | inbound emit |
| `shacs_bot/channels/manager.py` | 수정 | outbound 전/후 emit |
| `shacs_bot/agent/approval.py` | 수정 | approval 요청/해결 emit |
| `shacs_bot/heartbeat/service.py` | 수정 | decision/execute/notify emit |
| `shacs_bot/config/schema.py` | 수정 | hooks 관련 설정 추가 |

## 검증 기준

- [x] hooks 비활성화 시 기존 gateway/chat 동작 무변경
- [x] inbound → llm → tool → outbound 이벤트가 중복 없이 발생
- [x] 훅 핸들러 예외가 메인 응답을 중단시키지 않음
- [x] approval / heartbeat 관련 이벤트가 관측 가능

## Must NOT

- 훅 시스템 때문에 응답 경로가 외부 인프라에 hard dependency를 갖지 않는다.
- hooks가 없다는 이유로 기존 기능이 degraded 되지 않는다.
- hook payload에 원문 민감 데이터를 무분별하게 복사하지 않는다.
