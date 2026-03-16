# Prompt Caching 강화 — 작업 기록

> **프롬프트**: `D Next` — PRD 분석 후 다음 우선순위 작업 진행. Prompt Caching 강화 PRD 선택 → M1, M2 구현.

## 변경 파일

- `shacs_bot/providers/litellm.py`

## 변경 내용

### M1: `_apply_cache_control` 확장

- `_CACHE_MIN_CHARS = 4000` 클래스 상수 추가
- `_apply_cache_control` 메서드를 2-pass 로직으로 확장:
  - 1차 패스: `last_user_idx` (마지막 user 턴), `last_large_tool_idx` (4000자 이상 tool result) 탐색
  - 2차 패스: system + user + tool result + tool definition = 4개 cache breakpoint 삽입
- 기존 system 메시지 + tool definition 캐싱 동작 유지

### M2: cache 통계 로깅

- `_parse_response`에서 `cache_read_input_tokens`, `cache_creation_input_tokens` 파싱 (`getattr` 안전 처리)
- usage dict에 cache 통계 필드 추가
- cache hit 시 `logger.debug("Prompt cache hit: {} tokens cached, {} tokens created", ...)` 출력

## 미완료

- M3: Anthropic 모델로 멀티턴 대화 실제 테스트 (runtime 검증 필요)
