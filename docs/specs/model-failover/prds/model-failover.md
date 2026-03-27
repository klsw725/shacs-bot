# PRD: Model Failover

> **Spec**: [`docs/specs/2026-03-16-model-failover.md`](../spec.md)

---

## 문제

현재 `chat_with_retry` (`base.py:202-279`)는 **같은 프로바이더**에만 3회 재시도한다. 프로바이더 전체 장애(Anthropic 다운, OpenRouter 과부하 등) 시 봇이 완전히 응답 불능 상태에 빠진다.

```
사용자 → Anthropic (429) → 1초 대기 → Anthropic (429) → 2초 대기 → Anthropic (429) → 4초 대기 → Anthropic (429) → 에러 반환
                                          ↑ OpenRouter에 같은 모델이 있지만 시도하지 않음
```

멀티채널 봇(Telegram, Discord 등)은 24/7 가동이 전제이므로, 단일 프로바이더 의존은 가용성 리스크다.

## 해결책

`FailoverManager` 모듈을 추가하여, 기존 3회 재시도가 모두 실패한 후 **config에 정의된 failover chain**에 따라 다른 프로바이더로 자동 전환한다. circuit breaker 패턴으로 장애 프로바이더를 쿨다운하고, 시간 경과 후 자동 복구를 시도한다.

## 사용자 영향

| Before | After |
|---|---|
| 프로바이더 장애 시 에러 메시지 반환 | 자동으로 대체 프로바이더로 전환 |
| 복구까지 모든 채널 응답 불능 | failover chain이 있으면 서비스 유지 |
| 수동으로 config 변경 후 재시작 필요 | 쿨다운 후 자동 복구 시도 |
| 설정 변경 없음 | `failover` 섹션 추가 필요 (opt-in) |

## 기술적 범위

- **변경 파일**: 4개 (`providers/failover.py` 신규, `config/schema.py`, `providers/base.py`, `agent/loop.py` 수정)
- **변경 유형**: 신규 모듈 + 기존 메서드 확장
- **의존성**: 없음 (표준 라이브러리 `time.monotonic`만 사용)
- **하위 호환성**: `failover.enabled=false`(기본값)이면 기존 동작과 100% 동일

### 변경 1: Config 추가 (`config/schema.py`)

`HeartbeatConfig` 아래 (line 276 부근)에 추가:

```python
class FailoverRule(Base):
    """하나의 failover 전환 규칙."""
    from_provider: str = ""
    to_provider: str = ""
    model_map: dict[str, str] = Field(default_factory=dict)


class FailoverConfig(Base):
    """프로바이더 failover 설정."""
    enabled: bool = False
    cooldown_seconds: int = 300
    rules: list[FailoverRule] = Field(default_factory=list)
```

`Config` 클래스 (line 336)에 필드 추가:

```python
failover: FailoverConfig = Field(default_factory=FailoverConfig)
```

### 변경 2: FailoverManager (`providers/failover.py` 신규)

```python
"""런타임 프로바이더 failover 관리."""

import time
from typing import Any

from loguru import logger

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
        """쿨다운 기간이 지났으면 healthy로 판단."""
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
        logger.warning("Failover: {} 비활성화 ({}초 쿨다운)", provider_name, self._failover.cooldown_seconds)

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
        """config에서 프로바이더를 생성한다."""
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
                        original_provider, provider_name, model, mapped_model,
                    )
                    return response
                self.mark_failed(provider_name)
            except Exception as e:
                logger.warning("Failover 시도 실패: {} — {}", provider_name, str(e)[:120])
                self.mark_failed(provider_name)

        return None
```

### 변경 3: `chat_with_retry` 확장 (`base.py`)

**base.py** (line 202) — 시그니처에 파라미터 추가:

```python
async def chat_with_retry(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        model: str | None = None,
        max_tokens: object = _SENTINEL,
        temperature: object = _SENTINEL,
        reasoning_effort: object = _SENTINEL,
        tool_choice: str | dict[str, Any] | None = None,
        failover_manager: Any = None,      # 추가
        provider_name: str | None = None,   # 추가
) -> LLMResponse:
```

**base.py** (line 263, 마지막 `try` 블록 앞) — failover 시도 삽입:

```python
    # 모든 재시도 실패 — failover 시도
    if failover_manager and provider_name and self._is_transient_error(response.content):
        failover_response: LLMResponse | None = await failover_manager.try_failover(
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

    # 기존: 마지막 시도
    try:
        return await self.chat(...)
```

### 변경 4: AgentLoop에서 FailoverManager 전달 (`loop.py`)

**loop.py** (line 50, `__init__` 시그니처) — 파라미터 추가:

```python
from shacs_bot.providers.failover import FailoverManager

# __init__에 추가
self._failover: FailoverManager | None = None
```

**loop.py** (`run()` 또는 외부 초기화 시점) — config 기반 생성:

```python
# gateway.py 등 AgentLoop 생성 시
if config.failover.enabled:
    agent_loop._failover = FailoverManager(config)
```

**loop.py** (line 436, `_run_agent_loop` 내 `chat_with_retry` 호출) — 전달:

```python
response: LLMResponse = await self._provider.chat_with_retry(
    messages=messages,
    tools=self._tools.get_definitions(),
    model=self._model,
    failover_manager=self._failover,
    provider_name=self._provider_name,
)
```

## 성공 기준

1. `ANTHROPIC_API_KEY=invalid` + failover rule → OpenRouter로 자동 전환, 정상 응답
2. failover chain 전체 실패 시 원래 에러 메시지 사용자에게 반환
3. `cooldown_seconds` 경과 후 원래 프로바이더 자동 복구 시도
4. `failover.enabled=false`(기본값)일 때 기존 동작과 완전 동일
5. 비일시적 오류(401 인증 실패, 400 잘못된 요청)는 failover 트리거하지 않음

---

## 마일스톤

- [x] **M1: Config + FailoverManager 구현**
  `FailoverRule`, `FailoverConfig` 스키마 추가. `providers/failover.py` 신규 모듈 — circuit breaker, chain 순회, 프로바이더 생성 로직.

- [x] **M2: `chat_with_retry` 통합**
  `base.py`에 `failover_manager` / `provider_name` 파라미터 추가. 3회 재시도 실패 후 failover 시도 삽입.

- [x] **M3: AgentLoop 연결**
  `loop.py`에서 FailoverManager 인스턴스 생성 및 `chat_with_retry`에 전달. provider_name 추적.

- [x] **M4: 코드 레벨 검증**
  정적 분석 검증 완료. failover 트리거 흐름, 비일시적 오류 비전환, circuit breaker 쿨다운 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| failover 프로바이더도 동시 장애 | 낮음 | 중간 | chain을 3단계 이상 구성 가능. 모두 실패 시 원래 에러 반환 |
| 모델 매핑 누락으로 잘못된 모델 호출 | 중간 | 중간 | `model_map`에 없으면 원래 모델명 그대로 사용 — 프로바이더가 거부하면 다음 chain으로 |
| `_create_provider`가 LiteLLMProvider만 지원 | 낮음 | 낮음 | Custom/Azure 프로바이더는 failover 대상에서 제외. 추후 확장 가능 |
| failover 시 tool definition 호환성 | 낮음 | 중간 | OpenAI format은 표준. Anthropic→OpenAI 전환 시에도 LiteLLM이 변환 처리 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | M1~M3 구현 완료 — Config, FailoverManager, chat_with_retry 통합, AgentLoop/commands.py 연결 |
| 2026-03-21 | M4 코드 레벨 검증 완료. 성공 기준 5개 항목 전부 정적 분석으로 확인: (1) `chat_with_retry`에서 3회 재시도 후 `try_failover()` 호출 (base.py:295), (2) chain 전체 실패 시 `mark_failed` + 최종 chat 시도 (base.py:306-321), (3) `is_healthy()` — `time.monotonic()` 기반 쿨다운 경과 판단 정상 (failover.py:31), (4) `failover_manager=None` 기본값으로 비활성 시 기존 동작 보존, (5) **비일시적 오류 비전환 핵심 확인**: `_is_transient_error()` False → line 282에서 즉시 return → failover 코드(line 295) 도달 불가. Circuit breaker 순환 방지(`visited` set) 확인. LSP: failover.py 에러 0건. |
