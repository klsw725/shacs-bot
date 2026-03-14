"""LiteLLM provider implementation for multi-provider support."""
import hashlib
import os
import secrets
import string
from typing import Any, Union

import json_repair
import litellm
from litellm import acompletion
from litellm.litellm_core_utils.streaming_handler import CustomStreamWrapper
from litellm.types.utils import ModelResponse, Choices, StreamingChoices, Message, ChatCompletionMessageToolCall
from loguru import logger

from shacs_bot.providers.base import LLMProvider, LLMResponse, ToolCallRequest
from shacs_bot.providers.registry import find_gateway, ProviderSpec, find_by_model


class LiteLLMProvider(LLMProvider):
    """
    LLM provider using LiteLLM for multi-provider support.

    Supports OpenRouter, Anthropic, OpenAI, Gemini, and many other providers through a unifed interface.
    """
    # Standard chat-completion message keys.
    _ALLOWED_MSG_KEYS = frozenset({"role", "content", "tool_calls", "tool_call_id", "name", "reasoning_content"})
    _ANTHROPIC_EXTRA_KEYS = frozenset({"thinking_blocks"})
    _ALNUM = string.ascii_letters + string.digits

    def __init__(
            self,
            api_key: str | None = None,
            base_url: str | None = None,
            default_model: str = "anthropic/claude-opus-4-5",
            extra_headers: dict[str, str] | None = None,
            provider_name: str | None = None,
    ):
        super().__init__(api_key, base_url)
        self._model: str = default_model
        self._extra_headers: dict[str, str] = extra_headers or {}

        # 게이트웨이 / 로컬 배포 여부를 감지한다.
        # provider_name(설정 키에서 전달됨)이 주요 판단 기준이며,
        # api_key / api_base는 자동 감지를 위한 보조 기준으로 사용된다.
        self._gateway: ProviderSpec | None = find_gateway(provider_name=provider_name, api_key=api_key, base_url=base_url)

        if api_key:
            self._setup_env(api_key=api_key, base_url=base_url, model=default_model)

        if base_url:
            litellm.api_base = base_url

        litellm.suppress_debug_info = True
        litellm.drop_params = True

    def _setup_env(self, api_key: str, base_url: str, model: str) -> None:
        """감지된 provider에 따라 환경 변수를 설정한다."""
        spec: ProviderSpec | None = self._gateway or find_by_model(model=model)
        if not spec:
            return

        if not spec.env_key:
            # OAuth/provider-only specs (for example: openai_codex)
            return

        # Gateway/로컬 모드에서는 기존 환경 변수를 덮어쓰고, 표준 provider 모드에서는 덮어쓰지 않는다.
        if self._gateway:
            os.environ[spec.env_key] = api_key
        else:
            os.environ.setdefault(spec.env_key, api_key)

        # Resolve env_extras placeholders:
        #   {api_key}  → user's API key
        #   {base_url} → user's base_url, falling back to spec.default_api_base
        effective_base: str = base_url or spec.default_base_url

        for env_name, env_val in spec.env_extras:
            resolved = env_val.replace("{api_key}", api_key)
            resolved = resolved.replace("{api_base}", effective_base)
            os.environ.setdefault(env_name, resolved)

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
        Send a chat completion request via LiteLLM.

        Args:
            messages: List of message dicts with 'role' and 'content'.
            tools: Optional list of tool definitions in OpenAI format.
            model: Model identifier (e.g., 'anthropic/claude-sonnet-4-5').
            max_tokens: Maximum tokens in response.
            temperature: Sampling temperature.

        Returns:
            LLMResponse with content and/or tool calls.
        """
        original_model: str = model or self._model
        model: str = self._resolve_model(original_model)
        extra_msg_keys = self._extra_msg_keys(original_model, model)

        if self._supports_cache_control(original_model):
            messages, tools = self._apply_cache_control(messages, tools)

        # Clamp max_tokens to at least 1 — negative or zero values cause
        # LiteLLM to reject the request with "max_tokens must be at least 1".
        max_tokens = max(1, max_tokens)
        kwargs: dict[str, Any] = {
            "model": model,
            "messages": self._sanitize_messages(self._sanitize_empty_content(messages), extra_keys=extra_msg_keys),
            "max_tokens": max_tokens,
            "temperature": temperature,
        }

        # Apply model-specific overrides (e.g. kimi-k2.5 temperature)
        self._apply_model_overrides(model, kwargs)

        # Pass api_key directly — more reliable than env vars alone
        if self.api_key:
            kwargs["api_key"] = self.api_key

        # Pass api_base for custom endpoints
        if self.base_url:
            kwargs["api_base"] = self.base_url

        # Pass extra headers (e.g. APP-Code for AiHubMix)
        if self._extra_headers:
            kwargs["extra_headers"] = self._extra_headers

        if reasoning_effort:
            kwargs["reasoning_effort"] = reasoning_effort
            kwargs["drop_params"] = True

        if tools:
            kwargs["tools"] = tools
            kwargs["tool_choice"] = "auto"

        try:
            response: Union[ModelResponse, CustomStreamWrapper] = await acompletion(**kwargs)
            return self._parse_response(response)
        except Exception as e:
            # Return error as content for graceful handling
            return LLMResponse(
                content=f"LLM 호출 실패: {str(e)}",
                finish_reason="error",
            )

    def _resolve_model(self, model: str) -> str:
        """Resolve model name by applying provider/gateway prefixes."""
        if self._gateway:
            # Gateway mode: apply gateway prefix, skip provider-specific prefixes
            if self._gateway.strip_model_prefix:
                model = model.split("/")[-1]

            prefix: str = self._gateway.litellm_prefix
            if prefix and not model.startswith(f"{prefix}/"):
                model = f"{prefix}/{model}"

            return model

        # Standard mode: auto-prefix for known providers
        spec: ProviderSpec | None = find_by_model(model)
        if spec and spec.litellm_prefix:
            model: str = self._canonicalize_explicit_prefix(model, spec.name, spec.litellm_prefix)
            if not any(model.startswith(s) for s in spec.skip_prefixes):
                model = f"{spec.litellm_prefix}/{model}"

        return model

    def _extra_msg_keys(self, original_model: str, resolved_model: str) -> frozenset[str]:
        """Return provider-specific extra keys to preserve in request messages."""
        spec: ProviderSpec | None = find_by_model(original_model) or find_by_model(resolved_model)
        if (spec and spec.name == "anthropic") or ("claude" in original_model.lower()) or resolved_model.startswith("anthropic/"):
            return self._ANTHROPIC_EXTRA_KEYS

        return frozenset()

    def _supports_cache_control(self, model: str) -> bool:
        """Return True when the provider supports cache_control on content blocks."""
        if self._gateway is not None:
            return self._gateway.supports_prompt_caching

        spec: ProviderSpec | None = find_by_model(model)
        return spec is not None and spec.supports_prompt_caching

    def _apply_cache_control(
            self,
            messages: list[dict[str, Any]],
            tools: list[dict[str, Any]] | None,
    ) -> tuple[list[dict[str, Any]], list[dict[str, Any]] | None]:
        """Return copies of messages and tools with cache_control injected."""
        new_messages: list[dict[str, Any]] = []

        for msg in messages:
            if msg.get("role") == "system":
                content: Any = msg["content"]
                if isinstance(content, str):
                    new_content = [{"type": "text", "text": content, "cache_control": {"type": "ephemeral"}}]
                else:
                    new_content = list(content)
                    new_content[-1] = {**new_content[-1], "cache_control": {"type": "ephemeral"}}

                new_messages.append({**msg, "content": new_content})
            else:
                new_messages.append(msg)

        new_tools: list[dict[str, Any]] = tools
        if tools:
            new_tools = list(tools)
            new_tools[-1] = {**new_tools[-1], "cache_control": {"type": "ephemeral"}}

        return new_messages, new_tools

    def _apply_model_overrides(self, model: str, kwargs: dict[str, Any]) -> None:
        """Apply model-specific parameter overrides from the registry."""
        model_lower: str = model.lower()
        spec: ProviderSpec | None = find_by_model(model)
        if spec:
            for pattern, overrides in spec.model_overrides:
                if pattern in model_lower:
                    kwargs.update(overrides)
                    return

    def _parse_response(self, response: Union[ModelResponse, CustomStreamWrapper]) -> LLMResponse:
        """Parse LiteLLM response into our standard format."""
        choice: Union[Choices, StreamingChoices] = response.choices[0]
        message: Message = choice.message
        content: str = message.content
        finish_reason: str = choice.finish_reason

        raw_tool_calls: list[ChatCompletionMessageToolCall] = []

        for ch in response.choices:
            msg: Message = ch.message
            if hasattr(msg, "tool_calls") and msg.tool_calls:
                raw_tool_calls.extend(msg.tool_calls)
                if ch.finish_reason in ("tool_calls", "stop"):
                    finish_reason = ch.finish_reason

            if not content and msg.content:
                content = msg.content

        if len(response.choices) > 1:
            logger.debug("LiteLLM 응답에 {}개의 선택(choice)이 있었고, 그중에서 {}개의 tool_call을 병합했습니다.", len(response.choices), len(raw_tool_calls))

        tool_calls: list = []
        for tc in raw_tool_calls:
            # 필요한 경우 JSON 문자열에서 arguments를 파싱합니다.
            args: Any = tc.function.arguments
            if isinstance(args, str):
                args = json_repair.loads(args)

            provider_specific_fields = getattr(tc, "provider_specific_fields", None) or None
            function_provider_specific_fields = (
                    getattr(tc.function, "provider_specific_fields", None) or None
            )

            tool_calls.append(ToolCallRequest(
                id=self._short_tool_id(),
                name=tc.function.name,
                arguments=args,
                provider_specific_fields=provider_specific_fields,
                function_provider_specific_fields=function_provider_specific_fields,
            ))

        usage = {}
        if hasattr(response, "usage") and response.usage:
            usage = {
                "prompt_tokens": response.usage.prompt_tokens,
                "completion_tokens": response.usage.completion_tokens,
                "total_tokens": response.usage.total_tokens,
            }

        reasoning_content = getattr(message, "reasoning_content", None) or None
        thinking_blocks = getattr(message, "thinking_blocks", None) or None

        return LLMResponse(
            content=message.content,
            tool_calls=tool_calls,
            finish_reason=choice.finish_reason or "stop",
            usage=usage,
            reasoning_content=reasoning_content,
            thinking_blocks=thinking_blocks,
        )

    def _canonicalize_explicit_prefix(self, model: str, spec_name: str, canonical_prefix: str) -> str:
        """Normalize explicit provider prefixes like `github-copilot/...`."""
        if "/" not in model:
            return model

        prefix, remainder = model.split("/", 1)
        if prefix.lower().replace("-", "_") != spec_name:
            return model

        return f"{canonical_prefix}/{remainder}"

    def _normalize_tool_call_id(self, tool_call_id: Any) -> Any:
        """tool_call_id를 제공자(provider)에서 안전하게 사용할 수 있는 9자리 영숫자 형식으로 정규화합니다."""
        if not isinstance(tool_call_id, str):
            return tool_call_id

        if len(tool_call_id) == 9 and tool_call_id.isalnum():
            return tool_call_id

        return hashlib.sha1(tool_call_id.encode()).hexdigest()[:9]

    def _sanitize_messages(self, messages: list[dict[str, Any]], extra_keys: frozenset[str] = frozenset()) -> list[dict[str, Any]]:
        """Strip non-standard keys and ensure assistant messages have a content key."""
        allowed: frozenset[str] = self._ALLOWED_MSG_KEYS | extra_keys
        sanitized: list[Any] = LLMProvider._sanitize_request_message(messages, allowed)
        id_map: dict[str, str] = {}


        def map_id(value: Any) -> Any:
            if not isinstance(value, str):
                return value

            return id_map.setdefault(value, LiteLLMProvider._normalize_tool_call_id(value))


        for clean in sanitized:
            # assistant의 tool_calls[].id와 tool의 tool_call_id가 축약(shortening)된 이후에도 서로 일치하도록 유지합니다.
            # 그렇지 않으면 엄격한(provider) 구현에서는 이 연결(linkage)이 깨진 것으로 판단해 요청을 거부합니다.
            if isinstance(clean.get("tool_calls"), list):
                normalized_tool_calls: list[Any] = []
                for tool_call in clean["tool_calls"]:
                    if not isinstance(tool_call, dict):
                        normalized_tool_calls.append(tool_call)
                        continue

                    tc_clean: dict = dict(tool_call)
                    tc_clean["id"] = map_id(tool_call.get("id"))
                    normalized_tool_calls.append(tc_clean)

                clean["tool_calls"] = normalized_tool_calls

            if "tool_call_id" in clean and clean["tool_call_id"]:
                clean["tool_call_id"] = map_id(clean["tool_call_id"])

        return sanitized

    def get_default_model(self) -> str:
        """Get the default model."""
        return self._model

    def _short_tool_id(self):
        """모든 제공자(Mistral 포함)와 호환되는 9자리 영숫자 ID를 생성한다."""
        return "".join(secrets.choice(self._ALNUM) for _ in range(9))
