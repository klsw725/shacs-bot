# channel-rendering M3 manager/slack 적용

**날짜**: 2026-04-04 18:35  
**브랜치**: `feature/channel-rendering-m1-m2`

---

## 사용자 프롬프트

> 진행

---

## 작업 내용

- `shacs_bot/channels/manager.py`
  - `prepare_outbound_message()` 추가
  - dispatch 직전 `render_outbound_message()`를 호출하도록 통합
- `shacs_bot/channels/rendering.py`
  - Slack 전용 `SlackRenderer` 등록
  - fallback registry는 그대로 유지
- `shacs_bot/channels/slack.py`
  - manager에서 이미 렌더된 mrkdwn을 다시 변환하지 않도록 `_rendered_format` 분기 추가
  - `slack_outbound_text()` helper로 send 경로와 테스트 경로를 동일화
- `tests/test_channel_rendering_m2.py`
  - manager 통합점 테스트 추가
  - prerendered Slack 텍스트가 재변환되지 않는 경로 테스트 추가

## 의도

- M3의 핵심은 renderer 레이어를 실제 dispatch 경로에 연결하는 것이다.
- 우선 채널은 Slack만 적용하고, Discord/Telegram/Email은 plain-text fallback으로 유지해 범위를 작게 잡았다.
- 기존 채널 `send()` 구현을 대규모로 바꾸지 않고, manager pre-send 단계에서 renderer를 꽂는 방향을 확인했다.

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_channel_rendering_m2.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `41 passed`
- `shacs_bot/channels/rendering.py` diagnostics: clean
- `tests/test_channel_rendering_m2.py` diagnostics: clean
- verifier 패스: manager pre-send 렌더링, Slack 이중변환 방지, non-Slack fallback 확인

## 참고

- `shacs_bot/channels/manager.py`, `shacs_bot/channels/slack.py`에는 이번 변경과 무관한 기존 basedpyright 오류/경고가 남아 있다.
- 이번 M3 변경은 그 위에 pre-send renderer 통합과 Slack 우선 적용만 추가했다.
