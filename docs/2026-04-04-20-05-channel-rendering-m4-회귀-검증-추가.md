# channel-rendering M4 회귀 검증 추가

**날짜**: 2026-04-04 20:05  
**브랜치**: `feature/channel-rendering-m1-m2`

---

## 사용자 프롬프트

> 그래그래

---

## 작업 내용

- `tests/test_channel_rendering_m4.py` 신규 추가
  - manager dispatch에서 progress/tool/memory hint 필터가 실제로 적용되는지 검증
  - `split_rendered_content()`가 code block을 깨뜨리지 않는지 검증
  - Slack/Discord/Telegram의 long message split 경로를 실제 `send()` 기준으로 검증
  - tool/memory hint negative case와 raw `split_message()` codeblock reopen 경로를 추가 검증
- `shacs_bot/channels/slack.py`
  - Slack outbound text도 길이 제한(`MAX_MESSAGE_LEN = 40000`) 기준으로 분할 전송하도록 보완
- `shacs_bot/utils/helpers.py`
  - code fence 바로 뒤의 아주 이른 줄바꿈 때문에 `split_message()`가 사실상 무한 분할에 빠질 수 있던 문제를 수정
  - code block을 chunk 경계에서 닫고 다시 여는 경로가 마지막 chunk에서 깨지지 않도록 보정

## 의도

- M4는 renderer를 더 늘리는 단계가 아니라, 지금까지 만든 seam이 실제 dispatch/send 경로에서도 안전한지 확인하는 단계다.
- 특히 hint filtering과 채널 길이 제한은 단위 helper 수준이 아니라 실제 채널 send 경로로 검증해야 의미가 있다.
- Slack이 아직 단건 전송만 하던 상태였기 때문에, M4에서 드러난 split gap을 같이 메워서 검증을 통과시켰다.

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_channel_rendering_m2.py tests/test_channel_rendering_m4.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `62 passed`
- `tests/test_channel_rendering_m4.py` error diagnostics: 없음
- `shacs_bot/channels/slack.py`에는 이번 변경과 무관한 기존 basedpyright 이슈가 남아 있음

## 참고

- 이번 변경으로 PRD의 M4(힌트/분할 회귀 검증)에 해당하는 핵심 테스트가 추가되었다.
- split policy의 richer differentiation(`auto` vs `preserve_blocks`의 의미적 차이 확대)는 이후 개선 여지로 남아 있지만, 현재 helper 기준의 안전성 검증은 확보했다.
