"""유저에게 메시지 보내는 메세지 도구입니다. """
from typing import Any, Callable, Awaitable

from shacs_bot.agent.tools.base import Tool
from shacs_bot.bus.events import OutboundMessage


class MessageTool(Tool):
    """채팅 채널에서 사용자에게 메시지를 전송하는 도구.”"""

    name = "message"
    description = "사용자에게 메시지를 보냅니다. 어떤 내용을 전달하고 싶을 때 이 기능을 사용하세요."
    parameters = {
        "type": "object",
        "properties": {
            "content": {
                "type": "string",
                "description": "보낼 메시지 내용"
            },
            "channel": {
                "type": "string",
                "description": "Optional: 대상 채널 (telegram, discord 등)"
            },
            "chat_id": {
                "type": "string",
                "description": "Optional: 대상 채팅/유저 ID"
            },
            "media": {
                "type": "array",
                "items": {"type", "string"},
                "description": "Optional: 메시지에 첨부할 미디어 경로 (images, audio, documents)"
            }
        },
        "required": ["content"]
    }

    def __init__(
            self,
            send_callback: Callable[[OutboundMessage], Awaitable[None]] | None = None,
            default_channel: str = "",
            default_chat_id: str = "",
            default_message_id: str | None = None,
    ):
        self._send_callback = send_callback
        self._default_channel = default_channel
        self._default_chat_id = default_chat_id
        self._default_message_id = default_message_id
        self._sent_in_turn: bool = False

    async def execute(
            self,
            content: str,
            channel: str | None = None,
            chat_id: str | None = None,
            message_id: str | None = None,
            media: list[str] | None = None,
            **kwargs: Any
    ) -> str:
        channel = channel or self._default_channel
        chat_id = chat_id or self._default_chat_id
        message_id = message_id or self._default_message_id

        if not channel or not chat_id:
            return "에러: 메시지를 보낼 채널과 chat_id가 필요합니다."

        if not self._send_callback:
            return "에러: 메시지 전송 콜백이 설정되지 않았습니다."

        msg: OutboundMessage = OutboundMessage(
            channel=channel,
            chat_id=chat_id,
            content=content,
            media=media or [],
            metadata={
                "message_id": message_id
            }
        )

        try:
            await self._send_callback(msg)

            self._sent_in_turn = True
            media_info: str = f" with {len(media)} attachments" if media else ""
            return f"{channel} 메시지 전송: {chat_id}{media_info}"
        except Exception as e:
            return f"메시지 전송 중 에러 발생: {str(e)}"
