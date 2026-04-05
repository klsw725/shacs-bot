# channel-rendering Discord renderer 추가

**날짜**: 2026-04-04 19:05  
**브랜치**: `feature/channel-rendering-m1-m2`

---

## 사용자 프롬프트

> 진행해

---

## 작업 내용

- 현재 M3 Slack slice를 체크포인트 커밋
  - 커밋: `6051dfa feat(channels): integrate slack renderer dispatch`
- `shacs_bot/channels/rendering.py`
  - `DiscordRenderer` 추가
  - registry에 `discord` 채널 등록
- `shacs_bot/channels/discord.py`
  - `discord_outbound_text()` 추가
  - `render_text()` 추가
  - Markdown table → Discord 친화적 key/value 라인 변환
  - Markdown heading → bold 변환
  - 기존 send의 split/reply/thread/REST 전송 로직은 유지하고 content 준비만 renderer 경로로 연결
- `tests/test_channel_rendering_m2.py`
  - Discord renderer format marking 테스트 추가
  - `prepare_outbound_message()`의 Discord 적용 테스트 추가
  - prerendered Discord 텍스트 재변환 방지 테스트 추가

## 의도

- Slack 다음 채널로 Discord를 먼저 확장해 renderer seam이 단일 채널 특화가 아니라 반복 가능한 패턴인지 확인한다.
- Discord는 기존에 plain text only 경로였기 때문에, 적은 변경으로도 사용자 가시 효과가 크다.
- manager 분기 추가 없이 renderer registry와 채널 helper만으로 확장 가능한지 검증했다.

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_channel_rendering_m2.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `44 passed`
- `shacs_bot/channels/rendering.py` diagnostics: clean
- `tests/test_channel_rendering_m2.py` diagnostics: clean
- verifier 패스: Discord renderer slice 승인

## 참고

- `shacs_bot/channels/discord.py`에는 이번 변경과 무관한 기존 basedpyright 진단 이슈가 다수 남아 있다.
- 이번 변경은 그 위에 Discord 전용 text normalization과 renderer registration만 추가했다.
