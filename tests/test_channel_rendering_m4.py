from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from typing import Any

from shacs_bot.bus.events import OutboundMessage, RenderHints
from shacs_bot.bus.networks import MessageBus
from shacs_bot.channels.base import BaseChannel
from shacs_bot.channels.discord import DiscordChannel, MAX_MESSAGE_LEN as DISCORD_MAX_MESSAGE_LEN
from shacs_bot.channels.manager import ChannelManager
from shacs_bot.channels.rendering import split_rendered_content
from shacs_bot.channels.slack import MAX_MESSAGE_LEN as SLACK_MAX_MESSAGE_LEN, SlackChannel
from shacs_bot.channels.telegram import TelegramChannel
from shacs_bot.config.schema import Base, Config, DiscordConfig, SlackConfig, TelegramConfig
from shacs_bot.utils.helpers import split_message


class _RecordingChannel(BaseChannel):
    name: str = "recording"

    def __init__(self, bus: MessageBus) -> None:
        super().__init__(Base(), bus)
        self.sent: list[OutboundMessage] = []

    async def start(self) -> None:
        return None

    async def stop(self) -> None:
        return None

    async def send(self, msg: OutboundMessage) -> None:
        self.sent.append(msg)


class _TestChannelManager(ChannelManager):
    def set_test_channel(self, name: str, channel: BaseChannel) -> None:
        self._channels = {name: channel}

    async def run_dispatch_outbound(self) -> None:
        await self._dispatch_outbound()


class _TestSlackChannel(SlackChannel):
    def set_web_client(self, client: Any) -> None:
        self._web_client = client


class _TestDiscordChannel(DiscordChannel):
    def __init__(self, config: DiscordConfig, bus: MessageBus) -> None:
        super().__init__(config, bus)
        self.payloads: list[str] = []

    def set_http_client(self, client: Any) -> None:
        self._http = client

    async def _send_payload(
        self, url: str, headers: dict[str, str], payload: dict[str, Any]
    ) -> bool:
        self.payloads.append(str(payload["content"]))
        return True


class _TestTelegramChannel(TelegramChannel):
    def set_app(self, app: Any) -> None:
        self._app = app


async def _dispatch_once(
    msg: OutboundMessage,
    *,
    send_progress: bool,
    send_tool_hints: bool,
    send_memory_hints: bool,
) -> list[OutboundMessage]:
    bus = MessageBus()
    config = Config()
    config.channels.send_progress = send_progress
    config.channels.send_tool_hints = send_tool_hints
    config.channels.send_memory_hints = send_memory_hints
    manager = _TestChannelManager(config=config, bus=bus)
    recorder = _RecordingChannel(bus)
    manager.set_test_channel(msg.channel, recorder)

    task = asyncio.create_task(manager.run_dispatch_outbound())
    await bus.publish_outbound(msg)
    await asyncio.sleep(0.05)
    _ = task.cancel()
    try:
        await task
    except asyncio.CancelledError:
        pass
    return recorder.sent


def test_manager_dispatch_skips_progress_when_disabled() -> None:
    sent = asyncio.run(
        _dispatch_once(
            OutboundMessage(
                channel="cli",
                chat_id="direct",
                content="progress",
                render_hints=RenderHints(kind="progress"),
            ),
            send_progress=False,
            send_tool_hints=False,
            send_memory_hints=True,
        )
    )

    assert sent == []


def test_manager_dispatch_allows_tool_hint_when_enabled() -> None:
    sent = asyncio.run(
        _dispatch_once(
            OutboundMessage(
                channel="cli",
                chat_id="direct",
                content="tool",
                render_hints=RenderHints(kind="tool_hint"),
            ),
            send_progress=False,
            send_tool_hints=True,
            send_memory_hints=False,
        )
    )

    assert len(sent) == 1
    assert sent[0].content == "tool"


def test_manager_dispatch_allows_memory_hint_when_enabled() -> None:
    sent = asyncio.run(
        _dispatch_once(
            OutboundMessage(
                channel="cli",
                chat_id="direct",
                content="memory",
                render_hints=RenderHints(kind="memory_hint"),
            ),
            send_progress=False,
            send_tool_hints=False,
            send_memory_hints=True,
        )
    )

    assert len(sent) == 1
    assert sent[0].content == "memory"


def test_manager_dispatch_skips_tool_hint_when_disabled() -> None:
    sent = asyncio.run(
        _dispatch_once(
            OutboundMessage(
                channel="cli",
                chat_id="direct",
                content="tool",
                render_hints=RenderHints(kind="tool_hint"),
            ),
            send_progress=True,
            send_tool_hints=False,
            send_memory_hints=True,
        )
    )

    assert sent == []


def test_manager_dispatch_skips_memory_hint_when_disabled() -> None:
    sent = asyncio.run(
        _dispatch_once(
            OutboundMessage(
                channel="cli",
                chat_id="direct",
                content="memory",
                render_hints=RenderHints(kind="memory_hint"),
            ),
            send_progress=True,
            send_tool_hints=True,
            send_memory_hints=False,
        )
    )

    assert sent == []


def test_split_rendered_content_preserves_code_blocks() -> None:
    chunks = split_rendered_content(
        OutboundMessage(
            channel="cli",
            chat_id="direct",
            content="```\n" + ("x" * 60) + "\n```",
            render_hints=RenderHints(split_policy="preserve_blocks"),
        ),
        max_len=30,
    )

    assert len(chunks) > 1
    assert all(chunk.count("```") % 2 == 0 for chunk in chunks)


def test_split_message_closes_and_reopens_code_blocks_cleanly() -> None:
    chunks = split_message("```\n" + ("x" * 60) + "\n```\nafter block", max_len=30)

    assert len(chunks) > 1
    assert all(chunk.count("```") % 2 == 0 for chunk in chunks)
    assert chunks[-1].endswith("after block")


def test_slack_send_splits_long_messages() -> None:
    class _FakeWebClient:
        def __init__(self) -> None:
            self.texts: list[str] = []

        async def chat_postMessage(
            self, *, channel: str, text: str, thread_ts: str | None = None
        ) -> dict[str, str | None]:
            self.texts.append(text)
            return {"channel": channel, "thread_ts": thread_ts}

        async def files_upload_v2(self, **kwargs: object) -> dict[str, object]:
            return kwargs

    async def _run() -> list[str]:
        channel = _TestSlackChannel(SlackConfig(enabled=True), MessageBus())
        fake = _FakeWebClient()
        channel.set_web_client(fake)
        await channel.send(
            OutboundMessage(
                channel="slack",
                chat_id="room",
                content=("word " * 9000).strip(),
            )
        )
        return fake.texts

    texts = asyncio.run(_run())
    assert len(texts) > 1
    assert all(len(text) <= SLACK_MAX_MESSAGE_LEN for text in texts)


def test_discord_send_splits_long_messages() -> None:
    async def _run() -> None:
        channel = _TestDiscordChannel(DiscordConfig(enabled=True), MessageBus())
        channel.set_http_client(object())
        await channel.send(
            OutboundMessage(
                channel="discord",
                chat_id="room",
                content=("word " * 700).strip(),
            )
        )

        assert len(channel.payloads) > 1
        assert all(len(payload) <= DISCORD_MAX_MESSAGE_LEN for payload in channel.payloads)

    asyncio.run(_run())


def test_telegram_send_splits_long_messages() -> None:
    class _FakeBot:
        def __init__(self) -> None:
            self.texts: list[str] = []

        async def send_message(
            self,
            *,
            chat_id: int,
            text: str,
            parse_mode: str | None = None,
            reply_parameters: object | None = None,
        ) -> dict[str, object]:
            self.texts.append(text)
            return {
                "chat_id": chat_id,
                "parse_mode": parse_mode,
                "reply_parameters": reply_parameters,
            }

        async def send_message_draft(self, **kwargs: object) -> dict[str, object]:
            self.texts.append(str(kwargs["text"]))
            return kwargs

    class _FakeApp:
        def __init__(self, bot: _FakeBot) -> None:
            self.bot = bot

    async def _run() -> list[str]:
        channel = _TestTelegramChannel(TelegramConfig(enabled=True), MessageBus())
        bot = _FakeBot()
        channel.set_app(_FakeApp(bot))
        await channel.send(
            OutboundMessage(
                channel="telegram",
                chat_id="123",
                content=("word " * 1200).strip(),
            )
        )
        return bot.texts

    texts = asyncio.run(_run())
    assert len(texts) > 1
    assert all(len(text) <= 4000 for text in texts)
