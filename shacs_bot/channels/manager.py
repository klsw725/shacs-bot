"""채팅 채널을 조정(관리)하기 위한 채널 매니저."""

import asyncio
import importlib
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.bus.events import OutboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.channels.base import BaseChannel
from shacs_bot.config.schema import Config


# (config_attr, module_path, class_name, extra_kwargs: {constructor_kwarg: "dotted.config.path"})
_CHANNEL_DEFS: tuple[tuple[str, str, str, dict[str, str]], ...] = (
    (
        "telegram",
        "shacs_bot.channels.telegram",
        "TelegramChannel",
        {"groq_api_key": "providers.groq.api_key"},
    ),
    ("whatsapp", "shacs_bot.channels.whatsapp", "WhatsAppChannel", {}),
    ("discord", "shacs_bot.channels.discord", "DiscordChannel", {}),
    ("feishu", "shacs_bot.channels.feishu", "FeishuChannel", {}),
    ("mochat", "shacs_bot.channels.mochat", "MochatChannel", {}),
    ("dingtalk", "shacs_bot.channels.dingtalk", "DingTalkChannel", {}),
    ("email", "shacs_bot.channels.email", "EmailChannel", {}),
    ("slack", "shacs_bot.channels.slack", "SlackChannel", {}),
    ("qq", "shacs_bot.channels.qq", "QQChannel", {}),
    ("matrix", "shacs_bot.channels.matrix", "MatrixChannel", {}),
)


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

        self._media_save_dir: Path = Path(config.tools.media.save_dir).expanduser().resolve()

        self._init_channels()

    @property
    def enabled_channels(self) -> list[str]:
        """활성화된 채널 이름들을 가져옵니다."""
        return list(self._channels.keys())

    def _init_channels(self) -> None:
        """설정(_config)을 기반으로 채널을 초기화합니다."""
        for attr, module_path, cls_name, extra_src in _CHANNEL_DEFS:
            cfg = getattr(self._config.channels, attr, None)
            if not cfg or not cfg.enabled:
                continue
            try:
                mod = importlib.import_module(module_path)
                cls = getattr(mod, cls_name)
                extra = self._resolve_extra_kwargs(extra_src)
                self._channels[attr] = cls(cfg, self._bus, **extra)
                logger.info("{} channel enabled", attr)
            except ImportError as e:
                logger.warning("{} channel not available: {}", attr, e)

        self._validate_allow_from()

    def _resolve_extra_kwargs(self, mapping: dict[str, str]) -> dict[str, Any]:
        """dotted path (예: "providers.groq.api_key")를 self._config에서 resolve합니다."""
        result: dict[str, Any] = {}
        for kwarg_name, dotted_path in mapping.items():
            obj: Any = self._config
            for part in dotted_path.split("."):
                obj = getattr(obj, part, None)
                if obj is None:
                    break
            result[kwarg_name] = obj or ""
        return result

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
                    elif msg.metadata.get("_memory_hint"):
                        if not self._config.channels.send_memory_hints:
                            continue
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
                        if msg.media:
                            self._cleanup_generated_media(msg.media)
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

    def _cleanup_generated_media(self, media: list[str]) -> None:
        """전송 완료된 생성 미디어 파일을 삭제합니다 (save_dir 하위만)."""
        for media_path in media:
            p = Path(media_path).resolve()
            try:
                if p.is_relative_to(self._media_save_dir) and p.exists():
                    p.unlink()
                    logger.debug("Generated media cleaned up: {}", media_path)
            except Exception as e:
                logger.warning("Failed to clean up media {}: {}", media_path, e)

    def get_channel(self, name: str) -> BaseChannel | None:
        """name으로 채널 가져오기"""
        return self._channels.get(name)

    def get_status(self) -> dict[str, Any]:
        """모든 채널의 상태 가져오기"""
        return {
            name: {"enabled": True, "running": channel.is_running}
            for name, channel in self._channels.items()
        }
