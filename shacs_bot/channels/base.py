"""채팅 인터페이스를 위한 기본 채널 인터페이스"""
from abc import ABC, abstractmethod
from typing import Any

from loguru import logger

from shacs_bot.bus.events import OutboundMessage, InboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.config.schema import Base


class BaseChannel(ABC):
    """
    채팅 채널 구현을 위한 추상 기본 클래스.

    각 채널(Telegram, Discord 등)은 shacs-bot 메시지 버스와 통합하기 위해 이 인터페이스를 구현해야 한다.
    """
    name: str = "base"

    def __init__(self, config: Base, bus: MessageBus):
        """
        채널 초기화.

        Args:
            config: 채널 스팩 설정.
            bus: 통신을 위한 메시지 버스
        """
        self._config: Base = config
        self._bus: MessageBus = bus
        self._running: bool = False

    @property
    def config(self) -> Any:
        return self._config

    @abstractmethod
    async def start(self) -> None:
        """
        채널을 시작하고 메시지 수신을 시작합니다.

        이 메서드는 다음을 수행하는 장시간 실행되는 비동기 작업이어야 합니다:
        1. 채팅 플랫폼에 연결
        2. 들어오는 메시지를 수신
        3. _handle_message()를 통해 메시지를 버스로 전달
        """
        pass

    @abstractmethod
    async def stop(self) -> None:
        """채널을 중지하고 리소스를 정리합니다."""
        pass

    @abstractmethod
    async def send(self, msg: OutboundMessage) -> None:
        """
        이 채널을 통해 메시지를 전송합니다.

        Args:
            msg: 전송할 메시지.
        """
        pass

    @property
    def is_running(self) -> bool:
        """채널이 동작 중인지 확인"""
        return self._running

    def is_allowed(self, sender_id: str) -> bool:
        """*sender_id*가 허용되어 있는지 확인합니다. 빈 목록이면 모두 거부, '"*"'이면 모두 허용합니다."""
        allow_list: list[str] = getattr(self._config, "allow_from", [])
        if not allow_list:
            logger.warning("{}. allow_from 이 비었습니다. - 모든 접근이 거부되었습니다.", self.name)
            return False

        if "*" in allow_list:
            return True

        sender_str: str = str(sender_id)
        return (sender_str in allow_list) or any((p in allow_list) for p in sender_str.split("|") if p)

    async def _handle_message(
            self,
            sender_id: str,
            chat_id: str,
            content: str,
            media: list[str] | None = None,
            metadata: dict[str, Any] | None = None,
            session_key: str | None = None,
    ) -> None:
        """
        채팅 플랫폼에서 들어온 메시지를 처리합니다.

        이 메서드는 권한을 확인한 후 메시지를 버스(MessageBus)로 전달합니다.

        Args:
            sender_id: 메시지를 보낸 사용자의 식별자.
            chat_id: 채팅/채널 식별자.
            content: 메시지 텍스트 내용.
            media: 선택적 미디어 URL 목록.
            metadata: 선택적 채널별 메타데이터.
            session_key: 선택적 세션 키 재정의(예: 스레드 단위 세션).
        """
        if not self.is_allowed(sender_id=sender_id):
            logger.warning(
                f"""
                “채널 {sender_id}에서 발신자 {self.name}의 접근이 거부되었습니다. 
                접근을 허용하려면 config의 allowFrom 목록에 추가하세요.”
                """
            )
            return

        msg: InboundMessage = InboundMessage(
            channel=self.name,
            sender_id=str(sender_id),
            chat_id=str(chat_id),
            content=content,
            media=media or [],
            metadata=metadata or {},
            session_key_override=session_key,
        )
        await self._bus.publish_inbound(msg)



