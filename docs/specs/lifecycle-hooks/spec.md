# SPEC: Lifecycle Hooks

> **Prompt**: shacs-bot을 멀티채널 장시간 상주형 AI assistant로 운영하기 위해, 메시지/LLM/도구/전송/백그라운드 경계마다 내부 훅 시스템을 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`lifecycle-hooks.md`](./prds/lifecycle-hooks.md) | no-op 기본값을 유지하는 HookRegistry + 주요 emit 포인트 추가 |

## TL;DR

> **목적**: core 로직을 계속 분기시키지 않고도 정책, 감사, 알림, 채널 후처리, 운영 자동화를 붙일 수 있는 내부 이벤트/훅 레이어를 만든다.
>
> **Deliverables**:
> - `agent/hooks.py` — HookRegistry, HookContext, no-op dispatcher
> - `agent/loop.py` — before/after LLM, tool, turn emit 포인트
> - `channels/base.py`, `channels/manager.py` — inbound/outbound emit 포인트
> - `heartbeat/service.py` — heartbeat decision/execute/notify emit 포인트
> - `config/schema.py` — 훅 on/off 및 기본 동작 설정
>
> **Estimated Effort**: Medium (4-6시간)

## 현재 상태 분석

- `channels/base.py`는 `_handle_message()`에서 권한 확인 후 바로 `MessageBus`로 전달한다.
- `agent/loop.py`는 세션/LLM/도구/메모리/응답을 직접 연결하지만, 외부 확장 지점은 거의 없다.
- `channels/manager.py`는 outbound dispatch 실패를 로그만 남긴다.
- `heartbeat/service.py`는 `skip/run` 결정과 notify 콜백을 갖고 있지만 이벤트 계층은 없다.

즉, 현재는 기능을 추가하려면 각 지점에 if/else를 박아 넣어야 한다. assistant 제품으로 갈수록 필요한 기능은 다음과 같이 늘어난다:

1. 응답 전 민감 정보 마스킹
2. tool 실행 후 감사 로그
3. 특정 채널에서만 footer/format 후처리
4. background job 완료 시 운영 알림
5. approval 요청/승인 완료에 대한 관측

이 요구는 모두 "기존 흐름에 끼어드는 얇은 확장점"이 있으면 훨씬 단순해진다.

## 설계

### 설계 원칙

1. **기본 동작 no-op** — 훅이 없어도 기존 동작 완전 유지
2. **내부 이벤트 우선** — 외부 웹훅/쉘 연동은 후순위, 먼저 Python 내부 확장점 제공
3. **blocking 최소화** — 실패한 훅이 메인 응답을 깨뜨리지 않도록 기본 격리
4. **구조화된 context** — dict 덤프 대신 타입화된 context 제공

### Hook 이벤트 목록

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

### 핵심 구조

```python
@dataclass
class HookContext:
    event: str
    session_key: str | None = None
    channel: str | None = None
    chat_id: str | None = None
    payload: dict[str, Any] = field(default_factory=dict)


class HookRegistry:
    def register(self, event: str, handler: HookHandler) -> None: ...
    async def emit(self, event: str, ctx: HookContext) -> None: ...
```

- 기본 구현은 in-process only
- handler 실패는 `loguru`로 기록하고 메인 흐름은 유지
- 특정 이벤트(`before_outbound_send`)만 선택적으로 payload 수정 허용

### 1단계 범위

- Python callback 기반 registry
- config로 전체 on/off 가능
- 메인 emit 포인트만 연결
- side effect 없는 읽기/감사성 훅 우선

### 2단계 범위(이번 spec 범위 밖)

- 외부 webhook sink
- hook priority / ordering
- hook timeout 개별 설정
- operator-defined custom scripts

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/agent/hooks.py` | 신규 | HookRegistry, HookContext, no-op dispatcher |
| `shacs_bot/agent/loop.py` | 수정 | LLM/tool/turn 경계 emit |
| `shacs_bot/channels/base.py` | 수정 | inbound 수신 emit |
| `shacs_bot/channels/manager.py` | 수정 | outbound 전/후 emit |
| `shacs_bot/agent/approval.py` | 수정 | approval 요청/해결 emit |
| `shacs_bot/heartbeat/service.py` | 수정 | decision/execute/notify emit |
| `shacs_bot/config/schema.py` | 수정 | HooksConfig 추가 |

## 검증 기준

- [ ] hooks 비활성화 시 기존 gateway/chat 동작 무변경
- [ ] inbound → llm → tool → outbound 이벤트가 중복 없이 발생
- [ ] 훅 핸들러 예외가 메인 응답을 중단시키지 않음
- [ ] approval / heartbeat 관련 이벤트가 관측 가능

## Must NOT

- 훅 시스템 때문에 응답 경로가 외부 인프라에 hard dependency를 갖지 않는다.
- hooks가 없다는 이유로 기존 기능이 degraded 되지 않는다.
- hook payload에 원문 민감 데이터를 무분별하게 복사하지 않는다.
