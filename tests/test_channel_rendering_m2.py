from __future__ import annotations

import importlib
from dataclasses import replace
from typing import Callable, cast

from shacs_bot.bus.events import OutboundMessage, RenderHints
from shacs_bot.channels.manager import prepare_outbound_message
from shacs_bot.channels.rendering import (
    PlainTextRenderer,
    get_channel_renderer,
    register_channel_renderer,
    render_outbound_message,
    split_rendered_content,
)


def test_plain_text_renderer_preserves_message_shape() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="hello",
        metadata={"message_id": "1"},
        render_hints=RenderHints(kind="default", footer_mode="full"),
    )

    rendered = PlainTextRenderer().render(message)

    assert rendered.content == "hello"
    assert rendered.metadata == {"message_id": "1"}
    assert rendered.render_hints.footer_mode == "full"


def test_unknown_channel_uses_plain_text_fallback() -> None:
    message = OutboundMessage(channel="unknown", chat_id="direct", content="fallback")

    rendered = render_outbound_message(message)

    assert rendered.content == "fallback"
    assert rendered.channel == "unknown"


def test_registered_renderer_overrides_default() -> None:
    class UpperRenderer:
        def render(self, msg: OutboundMessage) -> OutboundMessage:
            return replace(msg, content=msg.content.upper())

    register_channel_renderer("cli-test", UpperRenderer())

    renderer = get_channel_renderer("cli-test")
    rendered = renderer.render(OutboundMessage(channel="cli-test", chat_id="d", content="hello"))

    assert rendered.content == "HELLO"


def test_slack_renderer_marks_rendered_format() -> None:
    rendered = render_outbound_message(
        OutboundMessage(channel="slack", chat_id="room", content="**hello**")
    )

    assert rendered.metadata["_rendered_format"] == "slack_mrkdwn"
    assert rendered.content


def test_discord_renderer_marks_rendered_format_and_converts_tables() -> None:
    rendered = render_outbound_message(
        OutboundMessage(
            channel="discord",
            chat_id="room",
            content="| Name | Value |\n| --- | --- |\n| Foo | Bar |",
        )
    )

    assert rendered.metadata["_rendered_format"] == "discord_markdown"
    assert "**Name**: Foo" in rendered.content


def test_split_rendered_content_uses_helper() -> None:
    message = OutboundMessage(
        channel="cli",
        chat_id="direct",
        content="line1\nline2\nline3",
        render_hints=RenderHints(split_policy="preserve_blocks"),
    )

    chunks = split_rendered_content(message, max_len=8)

    assert chunks
    assert all(len(chunk) <= 8 or chunk.endswith("```") for chunk in chunks)


def test_split_rendered_content_returns_empty_for_empty_content() -> None:
    message = OutboundMessage(channel="cli", chat_id="direct", content="")

    assert split_rendered_content(message, max_len=10) == []


def test_prepare_outbound_message_applies_registered_renderer() -> None:
    rendered = prepare_outbound_message(
        OutboundMessage(channel="slack", chat_id="room", content="**hello**")
    )

    assert rendered.metadata["_rendered_format"] == "slack_mrkdwn"
    assert rendered.content


def test_slack_outbound_text_uses_prerendered_content_without_reconversion() -> None:
    slack_module = importlib.import_module("shacs_bot.channels.slack")
    slack_outbound_text = cast(
        Callable[[OutboundMessage], str], getattr(slack_module, "slack_outbound_text")
    )

    assert (
        slack_outbound_text(
            OutboundMessage(
                channel="slack",
                chat_id="room",
                content="**already-rendered**",
                metadata={"_rendered_format": "slack_mrkdwn"},
            )
        )
        == "**already-rendered**"
    )


def test_discord_outbound_text_uses_prerendered_content_without_reconversion() -> None:
    discord_module = importlib.import_module("shacs_bot.channels.discord")
    discord_outbound_text = cast(
        Callable[[OutboundMessage], str], getattr(discord_module, "discord_outbound_text")
    )

    assert (
        discord_outbound_text(
            OutboundMessage(
                channel="discord",
                chat_id="room",
                content="**already-rendered**",
                metadata={"_rendered_format": "discord_markdown"},
            )
        )
        == "**already-rendered**"
    )


def test_prepare_outbound_message_applies_discord_renderer() -> None:
    rendered = prepare_outbound_message(
        OutboundMessage(
            channel="discord",
            chat_id="room",
            content="# Heading\n\n| Name | Value |\n| --- | --- |\n| Foo | Bar |",
        )
    )

    assert rendered.metadata["_rendered_format"] == "discord_markdown"
    assert "**Heading**" in rendered.content
    assert "**Name**: Foo" in rendered.content
