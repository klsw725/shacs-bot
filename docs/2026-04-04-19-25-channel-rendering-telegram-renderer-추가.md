# channel-rendering Telegram renderer 추가

**날짜**: 2026-04-04 19:25  
**브랜치**: `feature/channel-rendering-m1-m2`

---

## 사용자 프롬프트

> 진행해

---

## 작업 내용

- `shacs_bot/channels/rendering.py`
  - `TelegramRenderer` 추가
  - registry에 `telegram` 채널 등록
- `shacs_bot/channels/telegram.py`
  - `render_text()` 추가
  - `telegram_outbound_text()` 추가
  - 기존 `_markdown_to_telegram_html()`를 renderer 경로에서 재사용
  - send 경로에서 prerendered HTML 재변환을 건너뛰도록 연결
- `tests/test_channel_rendering_m2.py`
  - Telegram renderer format marking 테스트 추가
  - prerendered Telegram HTML 재변환 방지 테스트 추가
  - `prepare_outbound_message()`의 Telegram 적용 테스트 추가

## 의도

- Telegram은 이미 markdown→HTML 변환기가 있었기 때문에, 새 포맷터를 만들지 않고 기존 변환을 renderer seam으로 승격하는 방식이 가장 안전했다.
- manager 경계를 유지한 채 채널별 표현을 registry 기반으로 확장할 수 있는지 다시 확인했다.
- draft/reply/media 흐름은 건드리지 않고 텍스트 준비만 renderer 경로로 옮겼다.

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_channel_rendering_m2.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `47 passed`
- `shacs_bot/channels/rendering.py` diagnostics: clean
- `tests/test_channel_rendering_m2.py` diagnostics: clean
- verifier 패스: Telegram renderer slice 승인

## 참고

- `shacs_bot/channels/telegram.py`에는 이번 변경과 무관한 기존 basedpyright 진단 이슈가 다수 남아 있다.
- HTML 분할 경계 문제는 M4 길이/표현 회귀 검증에서 별도로 확인해야 한다.
