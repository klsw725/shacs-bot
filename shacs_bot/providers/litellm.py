"""LiteLLM provider implementation for multi-provider support."""
import os
from typing import Any

import litellm

from shacs_bot.providers.base import LLMProvider, LLMResponse


class LiteLLMProvider(LLMProvider):
    """
    LLM provider using LiteLLM for multi-provider support.

    Supports OpenRouter, Anthropic, OpenAI, Gemini, and many other providers through a unifed interface.
    """

    def __init__(
            self,
            api_key: str | None = None,
            base_url: str | None = None,
            model: str = "anthropic/claude-opus-4.6",
            extra_headers: dict[str, str] | None = None,
    ):
        super().__init__(api_key, base_url)
        self.model: str = model
        self.extra_headers: dict[str, str] = extra_headers or {}

        # Detect OpenRouter by api_key prefix or explicit base_url
        self.is_openrouter: bool = (
                (api_key and api_key.startswith("sk-or-")) or (base_url and "openrouter" in base_url)
        )

        # Detect AiHubMix by base_url
        self.is_aihubmix = bool(base_url and "aihubmix" in base_url)

        # Tract if using custom endpoint (vLLM, etc.)
        self.is_vllm: bool = bool(base_url) and not self.is_openrouter and not self.is_aihubmix

        # Configure LiteLLM based on provider
        if api_key:
            if self.is_openrouter:
                # OpenRouter mode - set key
                os.environ["OPENROUTER_API_KEY"] = api_key
            elif self.is_aihubmix:
                # AiHubMix gateway - OpenAI-compatible
                os.environ["OPENAI_API_KEY"] = api_key
            elif self.is_vllm:
                # vLLM/custom endpoint - uses OpenAI-compatible API
                os.environ["HOSTED_VLLM_API_KEY"] = api_key
            elif "deepseek" in model:
                os.environ.setdefault("DEEPSEEK_API_KEY", api_key)
            elif "anthropic" in model:
                os.environ.setdefault("ANTHROPIC_API_KEY", api_key)
            elif "openai" in model or "gpt" in model:
                os.environ.setdefault("OPENAI_API_KEY", api_key)
            elif "gemini" in model.lower():
                os.environ.setdefault("GEMINI_API_KEY", api_key)
            elif "zhipu" in model or "glm" in model or "zai" in model:
                os.environ.setdefault("ZAI_API_KEY", api_key)
                os.environ.setdefault("ZHIPUAI_API_KEY", api_key)
            elif "dashscope" in model or "qwen" in model.lower():
                os.environ.setdefault("DASHSCOPE_API_KEY", api_key)
            elif "groq" in model:
                os.environ.setdefault("GROQ_API_KEY", api_key)
            elif "moonshot" in model or "kimi" in model:
                os.environ.setdefault("MOONSHOT_API_KEY", api_key)
                os.environ.setdefault("MOONSHOT_API_BASE", base_url or "https://api.moonshot.cn/v1")

        if base_url:
            litellm.base_url = base_url

        # Disable LiteLLM logging noise
        litellm.suppress_debug_info = True

    async def chat(self, messages: list[dict[str, Any]], tools: list[dict[str, Any]] | None = None,
                   model: str | None = None, max_tokens: int = 4096, temperature: float = 0.7) -> LLMResponse:
        pass

    def get_default_model(self) -> str:
        pass