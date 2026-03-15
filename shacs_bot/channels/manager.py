"""채팅 채널을 조정(관리)하기 위한 채널 매니저."""

import asyncio
from typing import Any

from loguru import logger

from shacs_bot.bus.events import OutboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.channels.base import BaseChannel
from shacs_bot.config.schema import Config


class ChannelManager:
    """
    채팅 채널을 관리하고 메시지 라우팅을 조정합니다.

    주요 역할:
    - 활성화된 채널(Telegram, WhatsApp 등) 초기화
    - 채널 시작 및 중지
    - 발신(outbound) 메시지 라우팅
    """

    def __init__(self, config: Config, bus: MessageBus):
        self._config = config
        self._bus = bus
        self._channels: dict[str, BaseChannel] = {}
        self._dispatch_task: asyncio.Task | None = None

        self._init_channels()

    @property
    def enabled_channels(self) -> list[str]:
        """활성화된 채널 이름들을 가져옵니다."""
        return list(self._channels.keys())

    def _init_channels(self) -> None:
        """설정(_config)을 기반으로 채널을 초기화합니다."""

        # Telegram channel
        if self._config.channels.telegram.enabled:
            try:
                from shacs_bot.channels.telegram import TelegramChannel

                self._channels["telegram"] = TelegramChannel(
                    self._config.channels.telegram,
                    self._bus,
                    groq_api_key=self._config.providers.groq.api_key,
                )
                logger.info("Telegram channel enabled")
            except ImportError as e:
                logger.warning("Telegram channel not available: {}", e)

        # WhatsApp channel
        if self._config.channels.whatsapp.enabled:
            try:
                from shacs_bot.channels.whatsapp import WhatsAppChannel

                self._channels["whatsapp"] = WhatsAppChannel(
                    self._config.channels.whatsapp, self._bus
                )
                logger.info("WhatsApp channel enabled")
            except ImportError as e:
                logger.warning("WhatsApp channel not available: {}", e)

        # Discord channel
        if self._config.channels.discord.enabled:
            try:
                from shacs_bot.channels.discord import DiscordChannel

                self._channels["discord"] = DiscordChannel(self._config.channels.discord, self._bus)
                logger.info("Discord channel enabled")
            except ImportError as e:
                logger.warning("Discord channel not available: {}", e)

        # Feishu channel
        if self._config.channels.feishu.enabled:
            try:
                from shacs_bot.channels.feishu import FeishuChannel

                self._channels["feishu"] = FeishuChannel(self._config.channels.feishu, self._bus)
                logger.info("Feishu channel enabled")
            except ImportError as e:
                logger.warning("Feishu channel not available: {}", e)

        # Mochat channel
        if self._config.channels.mochat.enabled:
            try:
                from shacs_bot.channels.mochat import MochatChannel

                self._channels["mochat"] = MochatChannel(self._config.channels.mochat, self._bus)
                logger.info("Mochat channel enabled")
            except ImportError as e:
                logger.warning("Mochat channel not available: {}", e)

        # DingTalk channel
        if self._config.channels.dingtalk.enabled:
            try:
                from shacs_bot.channels.dingtalk import DingTalkChannel

                self._channels["dingtalk"] = DingTalkChannel(
                    self._config.channels.dingtalk, self._bus
                )
                logger.info("DingTalk channel enabled")
            except ImportError as e:
                logger.warning("DingTalk channel not available: {}", e)

        # Email channel
        if self._config.channels.email.enabled:
            try:
                from shacs_bot.channels.email import EmailChannel

                self._channels["email"] = EmailChannel(self._config.channels.email, self._bus)
                logger.info("Email channel enabled")
            except ImportError as e:
                logger.warning("Email channel not available: {}", e)

        # Slack channel
        if self._config.channels.slack.enabled:
            try:
                from shacs_bot.channels.slack import SlackChannel

                self._channels["slack"] = SlackChannel(self._config.channels.slack, self._bus)
                logger.info("Slack channel enabled")
            except ImportError as e:
                logger.warning("Slack channel not available: {}", e)

        # QQ channel
        if self._config.channels.qq.enabled:
            try:
                from shacs_bot.channels.qq import QQChannel

                self._channels["qq"] = QQChannel(
                    self._config.channels.qq,
                    self._bus,
                )
                logger.info("QQ channel enabled")
            except ImportError as e:
                logger.warning("QQ channel not available: {}", e)

        # Matrix channel
        if self._config.channels.matrix.enabled:
            try:
                from shacs_bot.channels.matrix import MatrixChannel

                self._channels["matrix"] = MatrixChannel(
                    self._config.channels.matrix,
                    self._bus,
                )
                logger.info("Matrix channel enabled")
            except ImportError as e:
                logger.warning("Matrix channel not available: {}", e)

        self._validate_allow_from()

    def _validate_allow_from(self) -> None:
        for name, ch in self._channels.items():
            if getattr(ch.config, "allow_from", None) == []:
                raise SystemExit(
                    f"에러: '{name}'의 allowFrom이 비어 있습니다 (모든 접근이 거부됩니다)."
                    f"모든 사용자를 허용하려면 ['*']로 설정하거나, 특정 사용자 ID를 추가하세요."
                )

    async def start_all(self) -> None:
        """모든 채널과 아웃바운드 디스패처를 시작합니다."""
        if not self._channels:
            logger.warning("활성화된 채널이 없습니다.")
            return

        # 아웃바운드 디스패처 시작
        self._dispatch_task = asyncio.create_task(self._dispatch_outbound())

        # 채널 시작
        tasks: list[asyncio.Task] = []

        for name, channel in self._channels.items():
            logger.info("{} 채널 시작 중...", name)
            tasks.append(asyncio.create_task(self._start_channel(name=name, channel=channel)))

        # 모든 작업이 완료될 때까지 대기합니다 (이들은 계속 실행되어야 합니다).
        await asyncio.gather(*tasks, return_exceptions=True)

    async def _dispatch_outbound(self) -> None:
        """아웃바운드 메시지를 적절한 채널로 전달합니다."""
        logger.info("아웃바운드 디스패처 시작되었습니다.")

        while True:
            try:
                msg: OutboundMessage = await asyncio.wait_for(
                    fut=self._bus.consume_outbound(), timeout=1.0
                )
                if msg.metadata.get("_progress"):
                    if msg.metadata.get("_skill_hint"):
                        pass
                    elif (
                        msg.metadata.get("_tool_hint") and not self._config.channels.send_tool_hints
                    ):
                        continue
                    elif (
                        not msg.metadata.get("_tool_hint")
                        and not self._config.channels.send_progress
                    ):
                        continue

                channel: BaseChannel | None = self._channels.get(msg.channel)
                if channel:
                    try:
                        await channel.send(msg)
                    except Exception as e:
                        logger.error("{}에게 에러 전송: {}", msg.channel, e)
                else:
                    logger.warning("알수 없는 채널: {}", msg.channel)
            except asyncio.TimeoutError:
                continue
            except asyncio.CancelledError:
                break

    async def _start_channel(self, name: str, channel: BaseChannel) -> None:
        """채널을 시작하고 발생하는 모든 예외를 로그로 기록합니다."""
        try:
            await channel.start()
        except Exception as e:
            logger.error("{} 채널 시작하는데 실패했습니다: {}", name, e)

    async def stop_all(self) -> None:
        """모든 채널과 디스패처를 중지합니다."""
        logger.info("모든 채널을 중지하는 중...")

        # 디스패처 중지
        if self._dispatch_task:
            self._dispatch_task.cancel()
            try:
                await self._dispatch_task
            except asyncio.CancelledError:
                pass

        # 모든 채널 중지
        for name, channel in self._channels.items():
            try:
                await channel.stop()
                logger.info("{} 채널이 정지되었습니다.", name)
            except Exception as e:
                logger.error("{} 채널을 정지하는데 에러 발생: {}", name, e)

    def get_channel(self, name: str) -> BaseChannel | None:
        """name으로 채널 가져오기"""
        return self._channels.get(name)

    def get_status(self) -> dict[str, Any]:
        """모든 채널의 상태 가져오기"""
        return {
            name: {"enabled": True, "running": channel.is_running}
            for name, channel in self._channels.items()
        }
