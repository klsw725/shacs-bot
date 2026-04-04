# channel-rendering M2 renderer interface 추가

**날짜**: 2026-04-04 18:05  
**브랜치**: `feature/channel-rendering-m1-m2`

---

## 사용자 프롬프트

> 아 지금 작업들을 브랜치하나 만들어서 커밋하고 M2 진행해

---

## 작업 내용

- `feature/channel-rendering-m1-m2` 브랜치 생성
- M1 render_hints 기반 작업과 operator-console 문서 동기화를 먼저 커밋
  - 커밋: `a4fcb7c feat(channels): add render hints foundation`
- `shacs_bot/channels/rendering.py` 신규 추가
  - `ChannelRenderer` 프로토콜
  - `PlainTextRenderer` 기본 구현
  - renderer registry/helper (`register_channel_renderer`, `get_channel_renderer`, `render_outbound_message`)
  - 공통 분할 helper (`split_rendered_content`)
- `tests/test_channel_rendering_m2.py` 신규 추가
  - plain-text fallback
  - renderer override
  - split helper

## 의도

- M2는 renderer 계층의 인터페이스와 fallback만 도입하고, 기존 채널 send 경로는 그대로 유지한다.
- `manager.py` 통합은 M3 범위로 남겨 두고, 지금 단계에서는 renderer 레이어를 안전하게 올릴 수 있는 최소 seam만 마련한다.
- 이후 Slack/Telegram/Discord/Email 채널별 표현 차이를 renderer로 이동할 수 있는 기반을 준비한다.

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_channel_rendering_m2.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `38 passed`
- `shacs_bot/channels/rendering.py` diagnostics: clean
- `tests/test_channel_rendering_m2.py` diagnostics: clean

## 정리

- M1은 `OutboundMessage`에 render intent를 싣는 단계였고, M2는 그 intent를 해석할 renderer 계층의 인터페이스를 추가한 단계다.
- 아직 기존 `ChannelManager`와 각 채널의 `send()`는 건드리지 않았기 때문에 회귀 위험이 낮고, M3에서 manager dispatch 직전에 renderer를 꽂으면 된다.
