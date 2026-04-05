"""Event types for the message bus."""

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Literal


RenderKind = Literal["default", "progress", "tool_hint", "memory_hint"]
FooterMode = Literal["off", "tokens", "full"]
SplitPolicy = Literal["auto", "preserve_blocks"]
SectionStyle = Literal["plain", "compact"]


@dataclass
class InboundMessage:
    """Message received from a chat channel."""

    channel: str  # telegram, discord, slack, whatsapp, shell
    sender_id: str  # User identifier
    chat_id: str  # Chat/channel identifier
    content: str  # Message text
    timestamp: datetime = field(default_factory=datetime.now)
    media: list[str] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)
    session_key_override: str | None = None  # 스레드 범위 세션을 위한 선택적 오버라이드

    @property
    def session_key(self) -> str:
        """Unique key for session identification."""
        return self.session_key_override or f"{self.channel}:{self.chat_id}"


@dataclass
class RenderHints:
    kind: RenderKind = "default"
    prefer_thread: bool = False
    prefer_reply: bool = False
    footer_mode: FooterMode = "off"
    split_policy: SplitPolicy = "auto"
    section_style: SectionStyle = "plain"


@dataclass
class OutboundMessage:
    """Message to send to a chat channel."""

    channel: str
    chat_id: str
    content: str
    reply_to: str | None = None
    media: list[str] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)
    render_hints: RenderHints = field(default_factory=RenderHints)
