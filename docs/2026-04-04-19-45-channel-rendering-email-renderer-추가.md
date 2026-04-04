# channel-rendering Email renderer 추가

**날짜**: 2026-04-04 19:45  
**브랜치**: `feature/channel-rendering-m1-m2`

---

## 사용자 프롬프트

> 가자

---

## 작업 내용

- `shacs_bot/channels/rendering.py`
  - `EmailRenderer` 추가
  - registry에 `email` 채널 등록
- `shacs_bot/channels/email.py`
  - `render_subject()` 추가
  - `render_text()` 추가
  - 첫 markdown heading을 제목 후보로 추출하고, 제목으로 채택된 경우 본문에서 제거
  - markdown table을 plain key/value 라인으로 변환
  - 제목에 포함된 inline markdown/link 문법 정리
- `tests/test_channel_rendering_m2.py`
  - Email renderer subject/body 분리 테스트 추가
  - 기존 subject override 보존 테스트 추가
  - heading 없는 본문 fallback 테스트 추가
  - markdown subject 정리 테스트 추가
  - `prepare_outbound_message()`의 Email 적용 테스트 추가

## 의도

- Email은 HTML 메일 재설계까지 가지 않고, 현재 plain-text SMTP 흐름을 유지한 채 제목/본문 분리만 renderer 책임으로 옮기는 최소 확장을 선택했다.
- 기존 `metadata["subject"]` override 규칙을 유지하면서, heading 기반 subject 추론만 renderer가 보조하도록 했다.
- reply prefix/attachment/SMTP 전송 경로는 건드리지 않았다.

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_channel_rendering_m2.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `52 passed`
- `shacs_bot/channels/rendering.py` diagnostics: clean
- `tests/test_channel_rendering_m2.py` diagnostics: clean
- verifier 패스: Email renderer slice 승인

## 참고

- `shacs_bot/channels/email.py`에는 이번 변경과 무관한 기존 basedpyright 경고가 다수 남아 있다.
- 이번 변경은 그 위에 Email 전용 subject/body normalization만 추가했다.
