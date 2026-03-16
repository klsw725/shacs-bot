"""런타임 프로바이더 failover 관리."""

from __future__ import annotations

import time
from typing import Any, TYPE_CHECKING

from loguru import logger

if TYPE_CHECKING:
    from shacs_bot.config.schema import Config, FailoverConfig, FailoverRule, ProviderConfig
    from shacs_bot.providers.base import LLMProvider, LLMResponse


class FailoverManager:
    """circuit breaker 패턴 기반 프로바이더 failover."""

    def __init__(self, config: Config):
        self._config: Config = config
        self._failover: FailoverConfig = config.failover
        self._circuit_breakers: dict[str, float] = {}

    @property
    def enabled(self) -> bool:
        return self._failover.enabled and bool(self._failover.rules)

    def is_healthy(self, provider_name: str) -> bool:
        ts: float | None = self._circuit_breakers.get(provider_name)
        if ts is None:
            return True
        if (time.monotonic() - ts) >= self._failover.cooldown_seconds:
            del self._circuit_breakers[provider_name]
            logger.info("Failover: {} 쿨다운 종료, 복구 시도", provider_name)
            return True
        return False

    def mark_failed(self, provider_name: str) -> None:
        self._circuit_breakers[provider_name] = time.monotonic()
        logger.warning(
            "Failover: {} 비활성화 ({}초 쿨다운)", provider_name, self._failover.cooldown_seconds
        )

    def get_chain(self, from_provider: str, model: str) -> list[tuple[str, str]]:
        """(provider_name, mapped_model) 체인을 반환."""
        chain: list[tuple[str, str]] = []
        current: str = from_provider
        visited: set[str] = {from_provider}

        for _ in range(len(self._failover.rules)):
            rule: FailoverRule | None = next(
                (r for r in self._failover.rules if r.from_provider == current), None
            )
            if not rule or rule.to_provider in visited:
                break
            mapped: str = rule.model_map.get(model, model)
            chain.append((rule.to_provider, mapped))
            visited.add(rule.to_provider)
            current = rule.to_provider

        return chain

    def _create_provider(self, provider_name: str) -> LLMProvider:
        from shacs_bot.providers.litellm import LiteLLMProvider

        pc: ProviderConfig | None = getattr(self._config.providers, provider_name, None)
        if not pc or not pc.api_key:
            raise ValueError(f"Failover: {provider_name}에 API 키가 설정되지 않음")

        return LiteLLMProvider(
            api_key=pc.api_key,
            base_url=pc.base_url,
            extra_headers=pc.extra_headers,
            provider_name=provider_name,
        )

    async def try_failover(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None,
        model: str,
        original_provider: str,
        **chat_kwargs: Any,
    ) -> LLMResponse | None:
        """failover chain을 순회하며 성공할 때까지 시도. 모두 실패 시 None."""
        chain: list[tuple[str, str]] = self.get_chain(original_provider, model)
        if not chain:
            return None

        for provider_name, mapped_model in chain:
            if not self.is_healthy(provider_name):
                continue

            try:
                provider: LLMProvider = self._create_provider(provider_name)
                response: LLMResponse = await provider.chat(
                    messages=messages,
                    tools=tools,
                    model=mapped_model,
                    **chat_kwargs,
                )
                if response.finish_reason != "error":
                    logger.info(
                        "Failover 성공: {} → {} (model: {} → {})",
                        original_provider,
                        provider_name,
                        model,
                        mapped_model,
                    )
                    return response
                self.mark_failed(provider_name)
            except Exception as e:
                logger.warning("Failover 시도 실패: {} — {}", provider_name, str(e)[:120])
                self.mark_failed(provider_name)

        return None
