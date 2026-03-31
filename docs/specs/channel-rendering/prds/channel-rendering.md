# PRD: Channel-aware Rendering Layer

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 assistant 응답은 채널별 형식 차이를 거의 고려하지 않고 문자열 중심으로 전달된다.

문제점:

1. thread/reply/footer/hint 표현 규칙이 채널 구현 내부에 흩어진다.
2. 같은 응답 의도라도 Slack/Discord/Telegram/Email에서 자연스러운 표현이 다르다.
3. 새 채널을 붙일 때마다 core 로직이나 개별 채널 구현에 중복 처리가 생긴다.

## 해결책

canonical response와 channel renderer를 분리한다.

- outbound 이벤트에 render hints 추가
- 채널별 renderer가 최종 표현 담당
- renderer 미존재 시 plain-text fallback

## 사용자 영향

| Before | After |
|---|---|
| 채널마다 품질과 표현이 들쭉날쭉 | 채널별 규칙에 맞는 일관된 응답 |
| footer/hint/thread 규칙이 구현마다 다름 | renderer 계층에서 통일 관리 |
| 새 채널 추가 시 중복 로직 증가 | fallback + renderer 추가로 확장 |

## 기술적 범위

- **변경 파일**: 5개 수정 + 1개 신규
- **변경 유형**: outbound 모델 확장 + 렌더링 계층 추가
- **의존성**: 없음
- **하위 호환성**: renderer 미지정 시 기존 plain text 전송 유지

### 변경 1: canonical render metadata (`shacs_bot/bus/events.py`)

- `OutboundMessage`에 render hints 필드 추가
- thread 선호, section 정보, footer 제어, 우선순위 힌트 포함

### 변경 2: renderer 인터페이스 추가 (`shacs_bot/channels/rendering.py`)

- `ChannelRenderer` 프로토콜
- `PlainTextRenderer` 기본 구현
- 공통 분할/후처리 helper

### 변경 3: ChannelManager 통합 (`shacs_bot/channels/manager.py`)

- dispatch 직전 renderer 선택
- 채널별 fallback 경로 보장

### 변경 4: 우선 채널 적용 (`shacs_bot/channels/slack.py`, `discord.py`, `telegram.py`, `email.py`)

- Slack: thread/section/footer 표현
- Discord: reply/thread/길이 분할 표현
- Telegram: markdown-safe 표현
- Email: subject/body 분리 표현

## 성공 기준

1. 같은 canonical response가 주요 채널 4종에서 자연스럽게 표현된다.
2. renderer가 없는 채널도 안전하게 fallback 된다.
3. progress/tool/memory hint 표현 규칙이 채널별로 일관된다.
4. AgentLoop는 채널별 분기로 오염되지 않는다.

---

## 마일스톤

- [ ] **M1: canonical response 메타데이터 정의**
  `bus/events.py`에 render hints 필드 추가.

- [ ] **M2: renderer 인터페이스 및 fallback 구현**
  `channels/rendering.py`에 기본 renderer와 helper 추가.

- [ ] **M3: manager 통합 및 우선 채널 적용**
  `channels/manager.py`에 renderer 선택을 넣고 Slack/Discord/Telegram/Email 순으로 적용.

- [ ] **M4: 힌트/분할 회귀 검증**
  progress/tool/memory hint와 길이 제한 시나리오 검증.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| renderer와 채널 send 책임 경계가 모호해짐 | 중간 | 중간 | renderer는 표현만, send는 전송만 담당하도록 역할 분리 |
| 채널별 분기 로직이 다시 manager로 유입 | 중간 | 높음 | manager는 renderer 선택만 하고 세부 표현은 renderer로 이동 |
| fallback 누락으로 일부 채널 전송 실패 | 낮음 | 높음 | PlainTextRenderer 기본값 유지 |

## Acceptance Criteria

- [ ] Slack/Discord/Telegram/Email의 표현 차이가 renderer로 분리된다.
- [ ] renderer 미존재 채널에서도 기존 응답이 그대로 전송된다.
- [ ] hint/footer/thread 규칙이 채널별로 자연스럽게 적용된다.
- [ ] 길이 초과 응답이 각 채널 규칙에 맞게 분할된다.
