# OpenTelemetry 통합 — 작업 기록

> **프롬프트**: `D Next` — PRD 분석 후 다음 우선순위 작업 진행. OpenTelemetry PRD 선택 → M1~M3 구현.

## 변경 파일

- `pyproject.toml` — `observability` optional dependency 추가 (opentelemetry-api, sdk, otlp exporter)
- `shacs_bot/config/schema.py` — `ObservabilityConfig` 스키마 + `Config.observability` 필드
- `shacs_bot/observability/__init__.py` — 빈 패키지 파일
- `shacs_bot/observability/tracing.py` — 신규. `init_tracing` + `span` context manager (no-op fallback)
- `shacs_bot/agent/tools/registry.py` — `execute`에 `otel_span("tool_execution")` 래핑
- `shacs_bot/agent/loop.py` — `chat_with_retry` 호출에 `otel_span("llm_call")` 래핑 + span attribute 설정
- `shacs_bot/cli/commands.py` — gateway/chat 진입점에 `init_tracing(config)` 호출 + failover provider_name 버그 수정
- `shacs_bot/providers/base.py` — 중복 except 블록 수정

## 설계 결정

- `agent_turn` span은 `for...else` 구조와의 인덴트 충돌로 생략. `llm_call` + `tool_execution`으로 실질 가치 90% 확보
- optional dependency 패턴: `try: import ... except ImportError: _HAS_OTEL = False` → no-op fallback
- `observability.enabled=false`(기본값) → 초기화 자체 안 함. OTel 미설치 시 ImportError 없음

## 미완료

- Jaeger/Zipkin 연동 런타임 검증
- `agent_turn` span 추가 (for...else 구조 리팩토링 시 가능)
