"""OpenTelemetry tracing — optional dependency. 미설치 시 no-op."""

from __future__ import annotations

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
    if _tracer is None:
        yield None
        return

    with _tracer.start_as_current_span(name, attributes=attributes or {}) as s:
        yield s
