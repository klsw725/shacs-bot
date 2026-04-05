# channel-rendering M1 render_hints 추가

**날짜**: 2026-04-04 16:40  
**브랜치**: `main`

---

## 사용자 프롬프트

> 구현해

---

## 작업 내용

- `shacs_bot/bus/events.py`
  - `RenderHints` dataclass 추가
  - `OutboundMessage`에 `render_hints` 필드 추가 (`default_factory` 사용)
- `shacs_bot/agent/loop.py`
  - `_render_hints()` 헬퍼 추가
  - 일반 최종 응답에 `footer_mode` 힌트 연결
  - progress / tool hint / memory hint outbound에 `render_hints.kind` 연결
  - 기존 metadata 플래그(`_progress`, `_tool_hint`, `_memory_hint`, `_skill_hint`)는 유지
- `shacs_bot/channels/manager.py`
  - `render_hints.kind`를 우선 읽고 legacy metadata를 fallback으로 사용하는 progress 필터 helper 추가
- `tests/test_channel_rendering_m1.py`
  - 기본값, tool hint, memory hint, progress, default, legacy metadata fallback, skill hint bypass 테스트 추가

## 의도

- 메시지의 transport metadata와 채널 독립적 표현 의도를 분리하는 M1 기반을 추가
- 기존 채널 `send()` 구현은 건드리지 않고, 이후 renderer 계층을 올릴 수 있는 최소 seam만 마련
- 기존 progress/tool/memory filtering 동작은 유지하면서 새 `render_hints` 경로를 병행 도입

## 검증

```bash
uv run pytest tests/test_channel_rendering_m1.py tests/test_llm_planner_fallback.py tests/test_e2e_planner_to_workflow.py
```

- 결과: `33 passed`
- `tests/test_channel_rendering_m1.py` diagnostics: clean
- `oh-my-claudecode:verifier` 검증 패스: 승인(핵심 요구사항 충족)

## 참고

- `shacs_bot/agent/loop.py`, `shacs_bot/channels/manager.py`에는 이번 변경과 무관한 기존 basedpyright 진단 이슈가 이미 남아 있다.
- 이번 변경은 그 위에 M1 범위만 외과적으로 추가했고, 새 테스트와 verifier 점검 기준에서는 문제 없음을 확인했다.
