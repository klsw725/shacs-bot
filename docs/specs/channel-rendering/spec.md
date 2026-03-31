# SPEC: Channel-aware Rendering Layer

> **Prompt**: 같은 assistant 응답을 채널마다 무작정 재사용하지 않고, canonical response를 채널별로 렌더링하는 계층을 도입한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`channel-rendering.md`](./prds/channel-rendering.md) | canonical outbound model + per-channel renderer + fallback 규칙 |

## TL;DR

> **목적**: 멀티채널 assistant의 품질을 높이기 위해 "normalize early, render late" 구조를 도입한다.
>
> **Deliverables**:
> - `bus/events.py` — canonical assistant response metadata 확장
> - `channels/rendering.py` — Renderer 인터페이스 + 공통 fallback
> - `channels/*.py` — 채널별 renderer 적용
> - `channels/manager.py` — dispatch 직전 render 호출
>
> **Estimated Effort**: Medium (4-6시간)

## 현재 상태 분석

- `OutboundMessage`는 channel/chat_id/content/media/metadata 중심의 얇은 구조다.
- `channels/manager.py`는 대상 채널을 찾아 `channel.send()`를 호출한다.
- 채널별 config에는 `reply_in_thread`, `group_policy`, `send_progress`, `send_memory_hints` 같은 flag만 있고, 응답 의미 구조는 없다.

문제는 assistant 응답이 채널에 따라 요구 형식이 다르다는 점이다.

- Slack: thread, block-like section, 긴 후속 액션 메시지
- Discord: thread/reply, 제한된 포맷, 길이 분할
- Telegram: markdown/html 제약
- Email: 제목/본문 분리, 긴 형식 허용

현재 구조에서는 각 채널이 문자열을 자기 방식으로 해석할 뿐이어서, 멀티채널 품질이 채널 구현 내부에 흩어진다.

## 설계

### 설계 원칙

1. **입력 의미는 하나** — assistant는 먼저 canonical response를 만든다.
2. **출력 표현은 채널이 결정** — 최종 포맷은 renderer가 담당한다.
3. **점진적 도입** — 기존 `content: str` 경로를 유지하면서 metadata를 확장한다.

### Canonical response

```python
class OutboundMessage:
    channel: str
    chat_id: str
    content: str
    media: list[str]
    metadata: dict[str, Any]
    render: dict[str, Any] | None = None
```

`render` 예시:

```json
{
  "tone": "default",
  "sections": ["요약", "다음 액션"],
  "thread_preferred": true,
  "priority": "normal",
  "suppress_footer": false
}
```

### Renderer 구조

```python
class ChannelRenderer(Protocol):
    def render(self, msg: OutboundMessage) -> OutboundMessage: ...
```

- `SlackRenderer`
- `DiscordRenderer`
- `TelegramRenderer`
- `EmailRenderer`
- 기본 `PlainTextRenderer`

### 적용 범위

1. thread / reply 정책 반영
2. 긴 메시지 분할 기준 표준화
3. progress/tool/memory hint 표현 통일
4. footer/usage를 채널별로 자연스럽게 배치

### 비목표

- rich UI 전용 block builder 전면 도입
- 채널별 완전 다른 assistant persona
- 템플릿 엔진 대형화

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/bus/events.py` | 수정 | render hints 메타데이터 확장 |
| `shacs_bot/channels/rendering.py` | 신규 | renderer 인터페이스 및 공통 fallback |
| `shacs_bot/channels/manager.py` | 수정 | dispatch 직전 renderer 호출 |
| `shacs_bot/channels/slack.py` | 수정 | thread/section 렌더링 적용 |
| `shacs_bot/channels/discord.py` | 수정 | thread/reply 렌더링 적용 |
| `shacs_bot/channels/telegram.py` | 수정 | markdown-safe 렌더링 적용 |
| `shacs_bot/channels/email.py` | 수정 | subject/body 렌더링 적용 |

## 검증 기준

- [ ] 기존 plain text 응답이 regression 없이 전송됨
- [ ] 동일한 canonical response가 Slack/Discord/Telegram에서 각기 자연스럽게 표현됨
- [ ] progress/tool/memory hint 표시 규칙이 채널별로 일관됨
- [ ] 길이 초과 메시지가 채널 규칙에 맞게 분할됨

## Must NOT

- 채널별 renderer 때문에 AgentLoop가 채널별 분기로 오염되지 않는다.
- 기존 채널 `send()` 구현을 대규모로 뒤엎지 않는다.
