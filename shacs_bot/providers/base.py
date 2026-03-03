"""Base LLM provider interface."""
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any


@dataclass
class ToolCallRequest:
    """A tool call request from the LLM."""
    id: str
    name: str
    arguments: dict[str, Any]


@dataclass
class LLMResponse:
    """Response from an LLM provider."""
    content: str | None
    tool_calls: list[ToolCallRequest] = field(default_factory=list)
    finish_reason: str = "stop"
    usage: dict[str, int] = field(default_factory=dict)
    reasoning_content: str | None = None
    thinking_blocks: list[dict] | None = None

    @property
    def has_tool_calls(self) -> bool:
        """Check if response contains tool calls."""
        return len(self.tool_calls) > 0


class LLMProvider(ABC):
    """
    Abstract base class for LLM providers.

    Implementations should handle the specifics of each provider's API
    while maintaining a consistent interface.
    """

    def __init__(self, api_key: str | None = None, base_url: str | None = None):
        self.api_key = api_key
        self.base_url = base_url

    @staticmethod
    def _sanitize_empty_content(messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
        """
        프로바이더에서 400 오류를 유발하는 빈 텍스트 콘텐츠를 대체합니다.

        MCP 도구가 아무것도 반환하지 않을 때 빈 콘텐츠가 발생할 수 있습니다.
        대부분의 프로바이더는 빈 문자열 콘텐츠나 리스트 형태 콘텐츠 내의
        빈 텍스트 블록을 허용하지 않습니다.
        """
        result: list[dict[str, Any]] = []

        for msg in messages:
            content: Any = msg.get("content")
            if isinstance(content, str) and not content:
                clean: dict = dict(msg)
                clean["content"] = None if (msg.get("role") == "assistant" and msg.get("tool_calls")) else "(empty)"
                result.append(clean)
                continue

            if isinstance(content, list):
                filtered: list[dict[str, Any]] = [
                    item for item in content
                        if not (
                            isinstance(item, dict)
                            and (item.get("type") in ("text", "input_text", "output_text"))
                            and not item.get("text")
                        )
                ]
                if len(filtered) != len(content):
                    clean: dict = dict(msg)

                    if filtered:
                        clean["content"] = filtered

                    elif msg.get("role") == "assistant" and msg.get("tool_calls"):
                        clean["content"] = None

                    else:
                        clean["content"] = "(empty)"

                    result.append(clean)
                    continue

            if isinstance(content, dict):
                clean: dict = dict(msg)
                clean["content"] = [content]
                result.append(clean)
                continue

        return result

    @abstractmethod
    def get_default_model(self) -> str:
        """이 프로바이더의 기본 모델 식별자를 반환합니다."""
        pass

    @abstractmethod
    async def chat(
            self,
            messages: list[dict[str, Any]],
            tools: list[dict[str, Any]] | None = None,
            model: str | None = None,
            max_tokens: int = 4096,
            temperature: float = 0.7,
            reasoning_effort: str | None = None,

            max_retries: int = 3,
    ) -> LLMResponse:
        """
           채팅 완료 요청을 전송합니다.

        Args:
            messages: 'role'과 'content'를 포함하는 메시지 딕셔너리 리스트.
            tools: 선택적 도구 정의 리스트.
            model: 모델 식별자(프로바이더별로 상이).
            max_tokens: 응답에서 생성할 최대 토큰 수.
            temperature: 샘플링 온도 값.

        Returns:
            콘텐츠 및/또는 tool 호출을 포함한 LLMResponse..
        """
        pass