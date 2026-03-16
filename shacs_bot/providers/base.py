"""Base LLM provider interface."""

import asyncio
import json
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any

from loguru import logger


@dataclass
class ToolCallRequest:
    """A tool call request from the LLM."""

    id: str
    name: str
    arguments: dict[str, Any]
    provider_specific_fields: dict[str, Any] | None = None
    function_provider_specific_fields: dict[str, Any] | None = None

    def to_openai_tool_call(self) -> dict[str, Any]:
        """OpenAI 스타일 tool call 페이로드 직렬화."""
        tool_call: dict[str, Any] = {
            "id": self.id,
            "type": "function",
            "function": {
                "name": self.name,
                "arguments": json.dumps(self.arguments, ensure_ascii=False),
            },
        }

        if self.provider_specific_fields:
            tool_call["provider_specific_fields"] = self.provider_specific_fields
        if self.function_provider_specific_fields:
            tool_call["function"]["provider_specific_fields"] = (
                self.function_provider_specific_fields
            )

        return tool_call


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


@dataclass
class GenerationSettings:
    """
    LLM 호출을 위한 기본 생성 파라미터.

    provider에 저장되어 모든 호출 지점(call site)이 동일한 기본값을
    상속하도록 하여, temperature / max_tokens / reasoning_effort를
    여러 레이어를 거치며 매번 전달할 필요가 없게 한다.

    개별 호출 지점에서는 chat() 또는 chat_with_retry()에 명시적으로
    키워드 인자를 전달하여 여전히 값을 재정의(override)할 수 있다.
    """

    temperature: float = 0.7
    max_tokens: int = 4096
    reasoning_effort: str | None = None


class LLMProvider(ABC):
    """
    Abstract base class for LLM providers.

    Implementations should handle the specifics of each provider's API
    while maintaining a consistent interface.
    """

    _CHAT_RETRY_DELAYS = (1, 2, 4)
    _TRANSIENT_ERROR_MARKERS = (
        "429",
        "rate limit",
        "500",
        "502",
        "503",
        "504",
        "overloaded",
        "timeout",
        "timed out",
        "connection",
        "server error",
        "temporarily unavailable",
    )
    _SENTINEL = object()

    def __init__(self, api_key: str | None = None, base_url: str | None = None):
        self.api_key = api_key
        self.base_url = base_url
        self.generation: GenerationSettings = GenerationSettings()

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
                clean["content"] = (
                    None
                    if (msg.get("role") == "assistant" and msg.get("tool_calls"))
                    else "(empty)"
                )
                result.append(clean)
                continue
            elif isinstance(content, list):
                filtered: list[dict[str, Any]] = [
                    item
                    for item in content
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
            elif isinstance(content, dict):
                clean: dict = dict(msg)
                clean["content"] = [content]
                result.append(clean)
                continue

            result.append(msg)
        return result

    @staticmethod
    def _sanitize_request_message(
        messages: list[dict[str, Any]],
        allowed_keys: frozenset[str],
    ) -> list[dict[str, Any]]:
        """provider에서 안전하게 사용할 수 있는 메시지 키만 유지하고, assistant의 content 형식을 정규화한다."""
        sanitized: list[Any] = []

        for msg in messages:
            clean: dict[str, Any] = {k: v for k, v in msg.items() if k in allowed_keys}
            if clean.get("role") == "assistant" and "content" not in clean:
                clean["content"] = None

            sanitized.append(clean)

        return sanitized

    @classmethod
    def _is_transient_error(cls, content: str | None) -> bool:
        err: str = (content or "").lower()
        return any(maker in err for maker in cls._TRANSIENT_ERROR_MARKERS)

    @abstractmethod
    async def chat(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        model: str | None = None,
        max_tokens: int = 4096,
        temperature: float = 0.7,
        reasoning_effort: str | None = None,
        tool_choice: str | dict[str, Any] | None = None,
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

    @abstractmethod
    def get_default_model(self) -> str:
        """Get the default model for this provider."""
        pass

    async def chat_with_retry(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        model: str | None = None,
        max_tokens: object = _SENTINEL,
        temperature: object = _SENTINEL,
        reasoning_effort: object = _SENTINEL,
        tool_choice: str | dict[str, Any] | None = None,
        failover_manager: Any = None,
        provider_name: str | None = None,
    ) -> LLMResponse:
        """
        일시적인(provider) 오류가 발생했을 때 재시도하면서 chat()을 호출합니다.

        매개변수가 명시적으로 전달되지 않으면 기본값으로 self.generation의 설정을 사용합니다.
        따라서 호출하는 쪽에서 temperature, max_tokens, reasoning_effort 값을 여러 계층에 걸쳐 계속 전달할 필요가 없습니다.
        """
        if max_tokens is self._SENTINEL:
            max_tokens = self.generation.max_tokens

        if temperature is self._SENTINEL:
            temperature = self.generation.temperature

        if reasoning_effort is self._SENTINEL:
            reasoning_effort = self.generation.reasoning_effort

        for attempt, delay in enumerate(self._CHAT_RETRY_DELAYS, start=1):
            try:
                response: LLMResponse = await self.chat(
                    messages=messages,
                    tools=tools,
                    model=model,
                    max_tokens=max_tokens,
                    temperature=temperature,
                    reasoning_effort=reasoning_effort,
                    tool_choice=tool_choice,
                )
            except asyncio.CancelledError:
                raise
            except Exception as e:
                response: LLMResponse = LLMResponse(
                    content=f"LLM 호출 에러: {e}", finish_reason="error"
                )

            if response.finish_reason != "error":
                return response

            if not self._is_transient_error(response.content):
                return response

            err: str = (response.content or "").lower()
            logger.warning(
                "LLM 전송 에러 (시도 {}/{}), 재시도 {}초: {}",
                attempt,
                len(self._CHAT_RETRY_DELAYS),
                delay,
                err[:120],
            )

            await asyncio.sleep(delay)

        if failover_manager and provider_name:
            failover_response = await failover_manager.try_failover(
                messages=messages,
                tools=tools,
                model=model or self.get_default_model(),
                original_provider=provider_name,
                max_tokens=max_tokens,
                temperature=temperature,
            )
            if failover_response:
                return failover_response
            failover_manager.mark_failed(provider_name)

        try:
            return await self.chat(
                messages=messages,
                tools=tools,
                model=model,
                max_tokens=max_tokens,
                temperature=temperature,
                reasoning_effort=reasoning_effort,
                tool_choice=tool_choice,
            )
        except asyncio.CancelledError:
            raise
        except Exception as e:
            return LLMResponse(content=f"LLM 에러 호출: {e}", finish_reason="error")
