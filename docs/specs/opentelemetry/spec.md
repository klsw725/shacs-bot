# SPEC: OpenTelemetry 통합

> **Prompt**: HKUDS/nanobot, OpenClaw 분석 후 shacs-bot에 추가할 기능 — OpenTelemetry 통합

## PRDs

| PRD | 설명 |
|---|---|
| [`opentelemetry.md`](./prds/opentelemetry.md) | OTel 모듈 + 3개 계측 포인트 + optional dependency |

## TL;DR

> **목적**: LLM 호출, 도구 실행, 메모리 통합 등 핵심 경로에 분산 트레이싱을 도입하여 프로덕션 관측성(observability)을 확보한다.
>
> **Deliverables**:
> - `observability/tracing.py` — OTel 초기화 + span 헬퍼
> - `providers/base.py` — LLM 호출 span
> - `agent/tools/registry.py` — 도구 실행 span
> - `agent/loop.py` — 에이전트 턴 span
> - `config/schema.py` — `ObservabilityConfig` 추가
>
> **Estimated Effort**: Short (2-3시간)

## 현재 상태 분석

- 로깅: `loguru`만 사용. 구조화된 메트릭/트레이싱 없음
- 채널별 에러는 로그로만 추적. 호출 체인의 병목 파악 불가
- LLM 호출 시간, 도구 실행 시간 측정 수단 없음

## 설계

### 계측(Instrumentation) 포인트

```
[Trace: agent_turn]
  ├─ [Span: llm_call] model, tokens, latency, cache_hit
  │     ├─ prompt_tokens, completion_tokens
  │     └─ finish_reason, provider
  ├─ [Span: tool_execution] tool_name, duration, success
  │     └─ error (if failed)
  ├─ [Span: tool_execution] ...
  ├─ [Span: llm_call] ... (multi-turn)
  └─ [Span: memory_consolidation] messages_count, duration
```

### 의존성

```toml
# pyproject.toml [project.optional-dependencies]
observability = [
    "opentelemetry-api>=1.20.0",
    "opentelemetry-sdk>=1.20.0",
    "opentelemetry-exporter-otlp-proto-grpc>=1.20.0",
]
```

optional dependency로 추가. 미설치 시 no-op 동작.

### 변경 사항

#### 1. Config 추가 (`config/schema.py`)

```python
class ObservabilityConfig(Base):
    """관측성 설정."""
    enabled: bool = False
    otlp_endpoint: str = "http://localhost:4317"  # gRPC endpoint
    service_name: str = "shacs-bot"
    sample_rate: float = 1.0  # 0.0 ~ 1.0
```

#### 2. OTel 초기화 (`observability/tracing.py`)

```python
"""OpenTelemetry 트레이싱 초기화 — optional dependency."""

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
    """OTel TracerProvider를 구성한다. config.observability.enabled가 False이면 no-op."""
    global _tracer
    if not _HAS_OTEL or not config.observability.enabled:
        return

    resource = Resource.create({"service.name": config.observability.service_name})
    provider = TracerProvider(resource=resource)
    exporter = OTLPSpanExporter(endpoint=config.observability.otlp_endpoint)
    provider.add_span_processor(BatchSpanProcessor(exporter))
    trace.set_tracer_provider(provider)
    _tracer = trace.get_tracer("shacs-bot")


def get_tracer():
    """현재 tracer 반환. 미초기화 시 None."""
    return _tracer


@contextmanager
def span(name: str, attributes: dict[str, Any] | None = None) -> Generator:
    """tracer가 있으면 span을 생성하고, 없으면 no-op context."""
    if _tracer is None:
        yield None
        return

    with _tracer.start_as_current_span(name, attributes=attributes or {}) as s:
        yield s
```

#### 3. LLM 호출 계측 (`providers/base.py`)

```python
from shacs_bot.observability.tracing import span

async def chat_with_retry(self, ...):
    with span("llm_call", {"model": model, "provider": self.__class__.__name__}) as s:
        # ... 기존 로직 ...
        if s and response:
            s.set_attribute("tokens.prompt", response.usage.get("prompt_tokens", 0))
            s.set_attribute("tokens.completion", response.usage.get("completion_tokens", 0))
            s.set_attribute("finish_reason", response.finish_reason)
            s.set_attribute("cache.read_tokens", response.usage.get("cache_read_input_tokens", 0))
        return response
```

#### 4. 도구 실행 계측 (`agent/tools/registry.py`)

```python
from shacs_bot.observability.tracing import span

async def execute(self, name: str, params: dict[str, Any]) -> str:
    with span("tool_execution", {"tool.name": name}) as s:
        result = await tool.execute(**params)
        if s:
            s.set_attribute("tool.success", not result.startswith("Error"))
        return result
```

#### 5. 에이전트 턴 계측 (`agent/loop.py`)

```python
from shacs_bot.observability.tracing import span

async def _run_agent_loop(self, ...):
    with span("agent_turn", {"session": session_key, "channel": channel}):
        # ... 기존 전체 루프 ...
```

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `observability/__init__.py` | 신규 | 패키지 초기화 |
| `observability/tracing.py` | 신규 | OTel 초기화, span 헬퍼, no-op fallback |
| `config/schema.py` | 수정 | `ObservabilityConfig` 추가, `Config`에 `observability` 필드 |
| `providers/base.py` | 수정 | `chat_with_retry`에 span 래핑 |
| `agent/tools/registry.py` | 수정 | `execute`에 span 래핑 |
| `agent/loop.py` | 수정 | `_run_agent_loop`에 span 래핑 |
| `pyproject.toml` | 수정 | `[project.optional-dependencies]`에 `observability` 그룹 |

## 검증 기준

- [ ] `uv sync` (observability 없이) 시 기존 동작 무변경 확인
- [ ] `uv sync --extra observability` 시 OTel 의존성 설치 확인
- [ ] Jaeger/Zipkin에서 `agent_turn` → `llm_call` → `tool_execution` span 계층 확인
- [ ] OTel 미설치 상태에서 `ImportError` 없이 no-op 동작 확인
