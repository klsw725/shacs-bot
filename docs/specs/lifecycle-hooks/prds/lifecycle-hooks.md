# PRD: Lifecycle Hooks

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 shacs-bot은 메시지 수신, LLM 호출, 도구 실행, 응답 전송, 승인, heartbeat 같은 핵심 경계에서 **공통 확장 지점이 없다**.

이로 인해 다음 문제가 생긴다.

1. 감사/알림/후처리를 추가하려면 core 흐름에 if/else를 삽입해야 한다.
2. 특정 채널 전송 직전 후처리나 승인 이벤트 관측이 흩어진 로직으로 구현된다.
3. 운영성 기능이 누적될수록 AgentLoop와 ChannelManager가 비대해진다.

## 해결책

HookRegistry + HookContext를 도입해, assistant lifecycle 경계마다 명시적인 emit 포인트를 만든다.

- in-process Python hook
- no-op 기본 동작
- 구조화된 context 전달
- 실패 격리

## 사용자 영향

| Before | After |
|---|---|
| 정책/감사 로직을 core 코드에 직접 삽입 | hook 등록으로 lifecycle에 부착 |
| outbound 후처리 규칙이 채널 구현에 분산 | before/after outbound hook으로 일관 적용 |
| approval/heartbeat 상태를 별도 관측하기 어려움 | 공통 이벤트로 감시 가능 |

## 기술적 범위

- **변경 파일**: 6개 수정 + 1개 신규
- **변경 유형**: 내부 이벤트 레이어 추가
- **의존성**: 없음
- **하위 호환성**: hook 미등록/비활성화 시 기존 동작 유지

### 변경 1: HookRegistry 추가 (`shacs_bot/agent/hooks.py`)

- `HookContext` dataclass 정의
- `HookHandler` 프로토콜 또는 callable 타입 정의
- 이벤트별 handler 등록/조회/emit 구현
- handler 예외는 로깅 후 삼킴

### 변경 2: AgentLoop emit 포인트 연결 (`shacs_bot/agent/loop.py`)

- `before_context_build`
- `before_llm_call`, `after_llm_call`
- `before_tool_execute`, `after_tool_execute`
- turn 단위 context 보강

### 변경 3: 채널 이벤트 연결 (`shacs_bot/channels/base.py`, `shacs_bot/channels/manager.py`)

- inbound 수신 시 `message_received`
- outbound 직전/직후 `before_outbound_send`, `after_outbound_send`
- outbound 수정 허용 이벤트 범위 제한

### 변경 4: approval/heartbeat emit (`shacs_bot/agent/approval.py`, `shacs_bot/heartbeat/service.py`)

- approval 생성/해결 이벤트 발행
- heartbeat decision/run/notify 결과 이벤트 발행

### 변경 5: config 추가 (`shacs_bot/config/schema.py`)

- hooks enable/disable
- payload redaction 기본 옵션
- outbound mutation 허용 여부 기본값

## 성공 기준

1. 핵심 경계별 이벤트가 중복 없이 emit 된다.
2. hook 비활성화 시 gateway/chat 동작이 회귀하지 않는다.
3. hook handler 실패가 사용자 응답을 중단시키지 않는다.
4. 운영자는 approval/heartbeat/outbound 관련 감시 로직을 hook으로 붙일 수 있다.

---

## 마일스톤

- [ ] **M1: HookRegistry와 context 모델 추가**
  `agent/hooks.py`에 registry, context, safe emit 구현.

- [ ] **M2: AgentLoop/채널 emit 포인트 연결**
  `agent/loop.py`, `channels/base.py`, `channels/manager.py`에 핵심 이벤트 연결.

- [ ] **M3: approval/heartbeat 통합 및 설정 추가**
  `agent/approval.py`, `heartbeat/service.py`, `config/schema.py` 반영.

- [ ] **M4: 회귀 및 예외 격리 검증**
  hooks 비활성화, handler 예외, outbound 수정 시나리오 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 이벤트 중복 발행 | 중간 | 중간 | 이벤트별 emit 지점을 한 번만 연결하고 검증 시나리오 추가 |
| hook payload에 민감정보 과다 포함 | 중간 | 높음 | context 필드 최소화 + redaction 옵션 추가 |
| hook 실패가 응답 흐름에 영향 | 낮음 | 높음 | safe emit와 예외 격리 기본값 유지 |

## Acceptance Criteria

- [ ] inbound, llm, tool, outbound, approval, heartbeat 이벤트가 정의되고 emit 된다.
- [ ] hook 미등록 상태에서 기존 기능이 동일하게 동작한다.
- [ ] 실패하는 hook handler가 있어도 사용자 응답이 전송된다.
- [ ] approval/heartbeat 이벤트를 기반으로 운영 로그를 남길 수 있다.
