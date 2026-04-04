from __future__ import annotations

from dataclasses import replace
from typing import Protocol

from shacs_bot.bus.events import OutboundMessage
from shacs_bot.utils.helpers import split_message


class ChannelRenderer(Protocol):
    def render(self, msg: OutboundMessage) -> OutboundMessage: ...


class PlainTextRenderer:
    def render(self, msg: OutboundMessage) -> OutboundMessage:
        return replace(msg, content=msg.content or "")


class SlackRenderer:
    def render(self, msg: OutboundMessage) -> OutboundMessage:
        try:
            from shacs_bot.channels.slack import SlackChannel

            metadata = dict(msg.metadata or {})
            metadata["_rendered_format"] = "slack_mrkdwn"
            return replace(
                msg, content=SlackChannel.render_text(msg.content or ""), metadata=metadata
            )
        except Exception:
            return replace(msg, content=msg.content or "")


class DiscordRenderer:
    def render(self, msg: OutboundMessage) -> OutboundMessage:
        try:
            from shacs_bot.channels.discord import DiscordChannel

            metadata = dict(msg.metadata or {})
            metadata["_rendered_format"] = "discord_markdown"
            return replace(
                msg, content=DiscordChannel.render_text(msg.content or ""), metadata=metadata
            )
        except Exception:
            return replace(msg, content=msg.content or "")


class TelegramRenderer:
    def render(self, msg: OutboundMessage) -> OutboundMessage:
        try:
            from shacs_bot.channels.telegram import TelegramChannel

            metadata = dict(msg.metadata or {})
            metadata["_rendered_format"] = "telegram_html"
            return replace(
                msg, content=TelegramChannel.render_text(msg.content or ""), metadata=metadata
            )
        except Exception:
            return replace(msg, content=msg.content or "")


_PLAIN_TEXT_RENDERER = PlainTextRenderer()
_CHANNEL_RENDERERS: dict[str, ChannelRenderer] = {}


def register_channel_renderer(channel: str, renderer: ChannelRenderer) -> None:
    _CHANNEL_RENDERERS[channel] = renderer


def get_channel_renderer(channel: str) -> ChannelRenderer:
    return _CHANNEL_RENDERERS.get(channel, _PLAIN_TEXT_RENDERER)


def render_outbound_message(msg: OutboundMessage) -> OutboundMessage:
    renderer = get_channel_renderer(msg.channel)
    return renderer.render(msg)


def split_rendered_content(msg: OutboundMessage, *, max_len: int) -> list[str]:
    content = msg.content or ""
    if not content:
        return []
    if msg.render_hints.split_policy == "preserve_blocks":
        return split_message(content, max_len=max_len)
    return split_message(content, max_len=max_len)


register_channel_renderer("slack", SlackRenderer())
register_channel_renderer("discord", DiscordRenderer())
register_channel_renderer("telegram", TelegramRenderer())
