from __future__ import annotations

from shacs_bot.bus.events import OutboundMessage, RenderHints
from shacs_bot.channels.manager import should_skip_outbound_progress


def test_outbound_message_defaults_render_hints() -> None:
    message = OutboundMessage(channel="cli", chat_id="direct", content="hello")

    assert message.render_hints.kind == "default"
    assert message.render_hints.footer_mode == "off"
    assert message.render_hints.split_policy == "auto"


def test_render_hints_tool_hint_respects_tool_hint_toggle() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="tool hint",
        render_hints=RenderHints(kind="tool_hint"),
    )

    assert should_skip_outbound_progress(
        message,
        send_progress=False,
        send_tool_hints=False,
        send_memory_hints=True,
    )
    assert not should_skip_outbound_progress(
        message,
        send_progress=False,
        send_tool_hints=True,
        send_memory_hints=True,
    )


def test_render_hints_memory_hint_respects_memory_hint_toggle() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="memory hint",
        render_hints=RenderHints(kind="memory_hint"),
    )

    assert should_skip_outbound_progress(
        message,
        send_progress=True,
        send_tool_hints=True,
        send_memory_hints=False,
    )
    assert not should_skip_outbound_progress(
        message,
        send_progress=False,
        send_tool_hints=False,
        send_memory_hints=True,
    )


def test_render_hints_progress_respects_progress_toggle() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="progress",
        render_hints=RenderHints(kind="progress"),
    )

    assert should_skip_outbound_progress(
        message,
        send_progress=False,
        send_tool_hints=True,
        send_memory_hints=True,
    )
    assert not should_skip_outbound_progress(
        message,
        send_progress=True,
        send_tool_hints=False,
        send_memory_hints=False,
    )


def test_metadata_progress_fallback_still_works() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="legacy tool hint",
        metadata={"_progress": True, "_tool_hint": True},
    )

    assert should_skip_outbound_progress(
        message,
        send_progress=True,
        send_tool_hints=False,
        send_memory_hints=True,
    )
    assert not should_skip_outbound_progress(
        message,
        send_progress=True,
        send_tool_hints=True,
        send_memory_hints=True,
    )


def test_skill_hint_keeps_existing_bypass_behavior() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="skill hint",
        metadata={"_progress": True, "_skill_hint": True},
        render_hints=RenderHints(kind="progress"),
    )

    assert not should_skip_outbound_progress(
        message,
        send_progress=False,
        send_tool_hints=False,
        send_memory_hints=False,
    )


def test_default_render_hints_never_trigger_progress_filter() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="final response",
        render_hints=RenderHints(kind="default"),
    )

    assert not should_skip_outbound_progress(
        message,
        send_progress=False,
        send_tool_hints=False,
        send_memory_hints=False,
    )
