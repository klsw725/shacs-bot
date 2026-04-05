# SPEC: Channel-aware Rendering Layer

> **Prompt**: 같은 assistant 응답을 채널마다 무작정 재사용하지 않고, canonical response를 채널별로 렌더링하는 계층을 도입한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`channel-rendering.md`](./prds/channel-rendering.md) | canonical response, renderer 계층, 채널별 적용 순서를 구현 태스크로 분해 |

## TL;DR

> **목적**: 멀티채널 assistant가 같은 의미의 응답을 채널별로 자연스럽게 표현하도록 한다.
>
> **Deliverables**:
> - `shacs_bot/bus/events.py` — canonical render metadata 확장
> - `shacs_bot/channels/rendering.py` — renderer 인터페이스와 fallback 규칙
> - `shacs_bot/channels/manager.py` — dispatch 직전 renderer 적용
> - `shacs_bot/channels/slack.py`, `discord.py`, `telegram.py`, `email.py` — 우선 적용 채널 개선
> - `docs/specs/channel-rendering/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Medium (4-6시간)

## User Scenarios & Testing

### Scenario 1 - 같은 응답 의도를 채널별로 다르게 표현한다

사용자는 같은 assistant 응답이라도 Slack, Discord, Telegram, Email에서 각 채널에 맞는 형식으로 받아야 한다.

**테스트**: 동일한 canonical response를 여러 채널에 보냈을 때 thread/footer/길이 제한이 각 채널 규칙에 맞게 반영되는지 확인한다.

### Scenario 2 - 채널이 추가되어도 core 로직은 유지된다

운영자는 새로운 채널을 붙일 때 AgentLoop를 수정하지 않고 renderer만 추가해야 한다.

**테스트**: renderer가 없는 채널도 plain-text fallback으로 동작하는지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 assistant 응답의 의미 구조와 채널별 표현 방식을 분리해야 한다.
- **FR-002**: renderer는 최소 Slack, Discord, Telegram, Email에 대해 채널별 표현 차이를 적용해야 한다.
- **FR-003**: renderer가 정의되지 않은 채널은 기본 표현으로 안전하게 fallback 해야 한다.
- **FR-004**: 진행 메시지, tool 힌트, memory 힌트, usage footer는 채널별 규칙에 맞게 표현되어야 한다.
- **FR-005**: 채널 표현 차이 때문에 AgentLoop가 채널별 분기로 오염되면 안 된다.

## Key Entities

- **Canonical Response**: 채널에 독립적인 assistant 응답 의미 구조
- **Render Hints**: thread, sections, priority, footer 같은 표현 힌트
- **Channel Renderer**: canonical response를 채널 표현으로 변환하는 단위

## Success Criteria

- 우선 적용 채널 4종에서 같은 응답 의도가 채널 규칙에 맞게 자연스럽게 보인다.
- renderer가 없는 채널에서도 전송 실패 없이 fallback 응답이 전달된다.
- 채널별 형식 차이를 위해 AgentLoop에 새로운 채널 분기가 추가되지 않는다.
- 긴 응답과 힌트 메시지가 채널 길이 제한을 넘기지 않는다.

## Assumptions

- 1단계는 rich UI를 전면 도입하지 않고 기존 문자열 전송 경로를 유지한다.
- 채널별 persona 분화는 범위 밖이다.
- 사용량 footer와 진행 힌트는 renderer가 표현만 다루고 생성 책임은 기존 로직을 유지한다.

## 현재 상태 분석

- `OutboundMessage`는 이제 `render_hints`를 포함해 channel/chat_id/content/media/metadata/render_hints 구조로 확장되었다.
- `channels/manager.py`는 대상 채널을 찾은 뒤 dispatch 직전에 renderer를 적용하고 `channel.send()`를 호출한다.
- Slack/Discord/Telegram/Email은 각 채널 renderer를 통해 prerender를 소비하고, renderer가 없는 채널은 plain-text fallback을 유지한다.

현재 구조는 canonical response와 channel renderer를 분리했고, 긴 응답 분할/힌트 필터/채널별 표현 차이를 테스트로 검증하는 단계까지 도달했다.

## 설계

### 설계 원칙

1. **normalize early, render late**
2. **점진적 도입** — 기존 `content: str` 경로 유지
3. **채널별 표현만 분리** — 응답 의미 생성은 기존 흐름 유지

### 범위

- canonical render metadata 추가
- renderer 인터페이스 추가
- 우선 채널 4종 적용
- plain-text fallback 제공

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

- [x] 기존 plain text 응답이 regression 없이 전송됨
- [x] 동일한 canonical response가 Slack/Discord/Telegram/Email에서 각기 자연스럽게 표현됨
- [x] progress/tool/memory hint 표시 규칙이 채널별로 일관됨
- [x] 길이 초과 메시지가 채널 규칙에 맞게 분할됨

## Must NOT

- 채널별 renderer 때문에 AgentLoop가 채널별 분기로 오염되지 않는다.
- 기존 채널 `send()` 구현을 대규모로 뒤엎지 않는다.
