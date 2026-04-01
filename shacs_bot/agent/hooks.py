"""Lifecycle hook registry for shacs-bot."""

from dataclasses import dataclass, field
from typing import Any, Callable, Awaitable

from loguru import logger

# ── Event names ──────────────────────────────────────────────────────────────
MESSAGE_RECEIVED = "message_received"
BEFORE_CONTEXT_BUILD = "before_context_build"
BEFORE_LLM_CALL = "before_llm_call"
AFTER_LLM_CALL = "after_llm_call"
BEFORE_TOOL_EXECUTE = "before_tool_execute"
AFTER_TOOL_EXECUTE = "after_tool_execute"
BEFORE_OUTBOUND_SEND = "before_outbound_send"
AFTER_OUTBOUND_SEND = "after_outbound_send"
APPROVAL_REQUESTED = "approval_requested"
APPROVAL_RESOLVED = "approval_resolved"
SESSION_LOADED = "session_loaded"
HEARTBEAT_DECIDED = "heartbeat_decided"
BACKGROUND_JOB_COMPLETED = "background_job_completed"

HookHandler = Callable[["HookContext"], Awaitable[None]]


@dataclass
class HookContext:
    """Structured delivery object passed to every hook handler.

    ``payload`` fields for ``before_outbound_send`` are the only mutable surface:
    handlers may overwrite ``payload["content"]`` (str) and ``payload["media"]`` (list[str]).
    All other events are observer-only; payload mutations are ignored by the runtime.
    """

    event: str
    session_key: str | None = None
    channel: str | None = None
    payload: dict[str, Any] = field(default_factory=dict)


class HookRegistry:
    """In-process lifecycle hook registry.

    Handlers are async-only and run sequentially in registration order.
    Handler failures are caught, logged, and swallowed so they never interrupt
    the main response path.
    """

    def __init__(self) -> None:
        self._handlers: dict[str, list[HookHandler]] = {}

    def register(self, event: str, handler: HookHandler) -> None:
        """Register *handler* for *event*."""
        self._handlers.setdefault(event, []).append(handler)

    async def emit(self, ctx: HookContext) -> None:
        """Fire all handlers registered for *ctx.event* sequentially."""
        for handler in self._handlers.get(ctx.event, []):
            try:
                await handler(ctx)
            except Exception as e:
                logger.warning(
                    "Hook handler 실패 (event={}, handler={}): {}",
                    ctx.event,
                    getattr(handler, "__qualname__", repr(handler)),
                    e,
                )


class NoOpHookRegistry(HookRegistry):
    """Zero-overhead no-op registry used when hooks are disabled."""

    async def emit(self, ctx: HookContext) -> None:
        pass


def register_example_hooks(registry: HookRegistry, redact_payloads: bool = True) -> None:
    for event in (
        SESSION_LOADED,
        AFTER_TOOL_EXECUTE,
        APPROVAL_RESOLVED,
        AFTER_OUTBOUND_SEND,
        HEARTBEAT_DECIDED,
        BACKGROUND_JOB_COMPLETED,
    ):
        registry.register(event, _example_logging_hook(redact_payloads=redact_payloads))


def _example_logging_hook(redact_payloads: bool) -> HookHandler:
    async def handler(ctx: HookContext) -> None:
        payload: dict[str, Any] = _build_example_payload(ctx, redact_payloads=redact_payloads)
        logger.info(
            "Lifecycle hook example: event={} session_key={} channel={} payload={}",
            ctx.event,
            ctx.session_key,
            ctx.channel,
            payload,
        )

    return handler


def _build_example_payload(ctx: HookContext, redact_payloads: bool) -> dict[str, Any]:
    if not ctx.payload:
        return {}
    if not redact_payloads:
        return dict(ctx.payload)

    allowed_keys: tuple[str, ...] = (
        "tool",
        "tier",
        "denied",
        "reason",
        "chat_id",
        "action",
        "has_tool_calls",
        "result_length",
        "is_error",
    )
    return {key: ctx.payload[key] for key in allowed_keys if key in ctx.payload}
