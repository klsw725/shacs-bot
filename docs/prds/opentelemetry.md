# PRD: OpenTelemetry 통합

> **Spec**: [`docs/specs/2026-03-16-opentelemetry.md`](../specs/2026-03-16-opentelemetry.md)

---

## 문제

현재 shacs-bot은 `loguru`만 사용한다. 구조화된 메트릭이나 분산 트레이싱이 없어서:

1. LLM 호출 지연 시 어느 프로바이더가 느린지 파악 불가
2. 도구 실행 실패 시 전체 호출 체인에서 어디서 깨졌는지 로그를 수동 추적
3. 멀티채널 운영 시 채널별/세션별 성능 비교 불가
4. prompt caching이 실제로 얼마나 비용을 절감하는지 정량 측정 불가

## 해결책

OpenTelemetry를 **optional dependency**로 추가하고, 3개 핵심 경로에 span을 삽입한다. 미설치 시 완전 no-op — 기존 동작에 영향 없음.

## 사용자 영향

| Before | After |
|---|---|
| 로그만으로 디버깅 | Jaeger/Zipkin에서 호출 체인 시각화 |
| LLM 응답 시간 측정 불가 | span에 latency, token count 기록 |
| cache hit 비율 확인 불가 | span attribute로 cache 통계 확인 |
| 설치 필수 의존성 증가 없음 | optional — `uv sync --extra observability`로 선택 설치 |

## 기술적 범위

- **변경 파일**: 6개 수정 + 2개 신규
- **변경 유형**: optional 모듈 추가 + 기존 메서드에 context manager 래핑
- **의존성**: `opentelemetry-api`, `opentelemetry-sdk`, `opentelemetry-exporter-otlp-proto-grpc` (optional)
- **하위 호환성**: optional dep 미설치 시 `ImportError` 없이 no-op. `observability.enabled=false`(기본값)이면 초기화 안 함

### 변경 1: 의존성 추가 (`pyproject.toml`)

`[project.optional-dependencies]` 섹션에 추가:

```toml
observability = [
    "opentelemetry-api>=1.20.0",
    "opentelemetry-sdk>=1.20.0",
    "opentelemetry-exporter-otlp-proto-grpc>=1.20.0",
]
```

### 변경 2: Config 추가 (`config/schema.py`)

`GatewayConfig` 아래 (line 278 부근)에 추가:

```python
class ObservabilityConfig(Base):
    """관측성 설정."""
    enabled: bool = False
    otlp_endpoint: str = "http://localhost:4317"
    service_name: str = "shacs-bot"
    sample_rate: float = 1.0
```

`Config` 클래스 (line 336)에 필드 추가:

```python
observability: ObservabilityConfig = Field(default_factory=ObservabilityConfig)
```

### 변경 3: tracing 모듈 (`shacs_bot/observability/__init__.py`, `shacs_bot/observability/tracing.py` 신규)

**`__init__.py`**: 빈 파일

**`tracing.py`**:

```python
"""OpenTelemetry 트레이싱 — optional dependency. 미설치 시 no-op."""

from contextlib import contextmanager
from typing import Any, Generator

try:
    from opentelemetry import trace
    from opentelemetry.sdk.trace import TracerProvider
    from opentelemetry.sdk.trace.export import BatchSpanProcessor
    from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
    from opentelemetry.sdk.resources import Resource
    _HAS_OTEL = True
except ImportError:
    _HAS_OTEL = False

_tracer = None


def init_tracing(config) -> None:
    """config.observability.enabled=True이고 OTel 설치 시 TracerProvider를 구성한다."""
    global _tracer
    if not _HAS_OTEL or not config.observability.enabled:
        return

    resource = Resource.create({"service.name": config.observability.service_name})
    provider = TracerProvider(resource=resource)
    exporter = OTLPSpanExporter(endpoint=config.observability.otlp_endpoint)
    provider.add_span_processor(BatchSpanProcessor(exporter))
    trace.set_tracer_provider(provider)
    _tracer = trace.get_tracer("shacs-bot")


@contextmanager
def span(name: str, attributes: dict[str, Any] | None = None) -> Generator:
    """tracer가 있으면 span을 생성하고, 없으면 no-op context."""
    if _tracer is None:
        yield None
        return

    with _tracer.start_as_current_span(name, attributes=attributes or {}) as s:
        yield s
```

### 변경 4: LLM 호출 계측 (`providers/base.py`)

**base.py** (line 202, `chat_with_retry` 메서드):

메서드 전체를 `span`으로 래핑:

```python
from shacs_bot.observability.tracing import span as otel_span

async def chat_with_retry(self, ...):
    with otel_span("llm_call", {"model": model or "", "provider": type(self).__name__}) as s:
        # ... 기존 전체 로직 ...

        # 성공 응답 반환 직전에 attribute 기록
        if s and response and response.finish_reason != "error":
            s.set_attribute("tokens.prompt", response.usage.get("prompt_tokens", 0))
            s.set_attribute("tokens.completion", response.usage.get("completion_tokens", 0))
            s.set_attribute("finish_reason", response.finish_reason)
            s.set_attribute("cache.read_tokens", response.usage.get("cache_read_input_tokens", 0))

        return response
```

### 변경 5: 도구 실행 계측 (`agent/tools/registry.py`)

**registry.py** (line 36, `execute` 메서드):

```python
from shacs_bot.observability.tracing import span as otel_span

async def execute(self, name: str, params: dict[str, Any]) -> str:
    with otel_span("tool_execution", {"tool.name": name}) as s:
        # ... 기존 로직 ...
        result = await tool.execute(**params)
        if s:
            s.set_attribute("tool.success", not isinstance(result, str) or not result.startswith("Error"))
            s.set_attribute("tool.result_length", len(result) if isinstance(result, str) else 0)
        return result
```

### 변경 6: 에이전트 턴 계측 (`agent/loop.py`)

**loop.py** (line 425, `_run_agent_loop` 메서드):

```python
from shacs_bot.observability.tracing import span as otel_span

async def _run_agent_loop(self, init_messages, on_progress=None):
    with otel_span("agent_turn", {"model": self._model}):
        # ... 기존 전체 루프 ...
```

### 변경 7: 초기화 호출

게이트웨이 시작 시 (`cli/commands.py`의 gateway 커맨드 또는 `__main__.py`) `init_tracing(config)`을 호출:

```python
from shacs_bot.observability.tracing import init_tracing
init_tracing(config)
```

## 성공 기준

1. `uv sync` (observability 없이) 시 기존 동작 무변경, `ImportError` 없음
2. `uv sync --extra observability` 후 `observability.enabled=true` 설정 시 OTel 초기화
3. Jaeger에서 `agent_turn` → `llm_call` → `tool_execution` span 계층 확인
4. span에 `tokens.prompt`, `tokens.completion`, `tool.name`, `cache.read_tokens` attribute 포함
5. `observability.enabled=false`(기본값)이면 span 생성 안 함 (no-op)

---

## 마일스톤

- [x] **M1: 모듈 + Config 추가**
  `observability/` 패키지 생성. `tracing.py` 구현 (no-op fallback). `ObservabilityConfig` 스키마 추가. `pyproject.toml`에 optional dep.

- [x] **M2: 2개 계측 포인트 삽입**
  `registry.py` (`tool_execution`), `loop.py` (`llm_call`) — `otel_span` context manager로 래핑. `agent_turn`은 `for...else` 구조 호환 문제로 생략.

- [x] **M3: 초기화 + 검증**
  gateway/chat 시작 시 `init_tracing` 호출. optional dep 미설치 시 no-op 확인. Jaeger 연동은 runtime 검증 필요.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| OTel 의존성이 무거움 (~50MB) | 중간 | 낮음 | optional dep — 필요한 사용자만 설치 |
| span 생성 오버헤드 | 낮음 | 낮음 | LLM 호출 자체가 수초. span 생성은 마이크로초 |
| OTLP exporter 연결 실패 시 | 낮음 | 낮음 | `BatchSpanProcessor`가 비동기 — 연결 실패해도 에이전트에 영향 없음 |
| `async with` 대신 `with`로 span 사용 | 낮음 | 낮음 | OTel span context manager는 sync/async 호환 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | M1~M3 구현 완료 — tracing 모듈, 계측 포인트 2개(llm_call, tool_execution), init_tracing 호출 |
