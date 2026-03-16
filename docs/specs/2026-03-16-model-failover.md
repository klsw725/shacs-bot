# SPEC: Model Failover

> **Prompt**: HKUDS/nanobot, OpenClaw 분석 후 shacs-bot에 추가할 기능 — Model Failover

## PRDs

| PRD | 설명 |
|---|---|
| [`docs/prds/model-failover.md`](../prds/model-failover.md) | FailoverManager + circuit breaker + chat_with_retry 통합 |

## TL;DR

> **목적**: 프로바이더 장애 시 자동으로 다른 프로바이더로 전환하는 failover 체인을 도입하여, 멀티채널 봇의 24/7 가용성을 보장한다.
>
> **Deliverables**:
> - `config/schema.py` — `FailoverConfig` 추가
> - `providers/base.py` — `chat_with_retry`에 failover 로직 추가
> - `providers/failover.py` — `FailoverManager` 신규 모듈
>
> **Estimated Effort**: Medium (3-5시간)

## 현재 상태 분석

### 현재 retry 로직 (`base.py:202-279`)

```python
_CHAT_RETRY_DELAYS = (1, 2, 4)  # 3회 재시도, 같은 프로바이더

for attempt, delay in enumerate(self._CHAT_RETRY_DELAYS, start=1):
    response = await self.chat(...)
    if response.finish_reason != "error":
        return response
    if not self._is_transient_error(response.content):
        return response  # 비일시적 오류는 재시도 안 함
    await asyncio.sleep(delay)
```

**한계**:
- 같은 프로바이더에만 재시도 — 프로바이더 전체 장애 시 무력
- 3회 실패 후 에러 반환 — 다른 프로바이더 시도 안 함
- 설정의 `_match_provider` 폴백은 **초기화 시점**에만 동작 — 런타임 전환 불가

### Config의 프로바이더 매칭 (`schema.py:355-397`)

`_match_provider()`가 이미 PROVIDERS 순서대로 폴백하지만, 이는 **초기화 시점에 한 번**만 호출된다. 런타임에 프로바이더가 죽으면 전환 메커니즘이 없다.

## 설계

### 아키텍처

```
AgentLoop._run_agent_loop
    ↓
provider.chat_with_retry(messages, tools, model)
    ↓ (3회 재시도 실패)
FailoverManager.try_failover(messages, tools, model, original_error)
    ↓
failover chain에서 다음 프로바이더로 LiteLLMProvider 생성 → chat()
    ↓ (성공 시 반환, 실패 시 다음 프로바이더)
모든 failover 소진 → 원래 에러 반환
```

### 핵심 원칙

1. **명시적 failover chain**: 사용자가 config에 순서를 지정. 암묵적 전환 없음
2. **모델 매핑 필수**: Anthropic → OpenAI 전환 시 `claude-opus-4-5` → `gpt-4o` 매핑
3. **일시적 오류만 failover**: 인증 실패(401), 잘못된 요청(400)은 failover 하지 않음
4. **쿨다운 기반 복구**: 장애 프로바이더는 일정 시간 후 자동 복구 시도

### 변경 사항

#### 1. Config 추가 (`config/schema.py`)

```python
class FailoverRule(Base):
    """하나의 failover 전환 규칙."""
    from_provider: str = ""          # 원본 프로바이더 이름 (e.g. "anthropic")
    to_provider: str = ""            # 대체 프로바이더 이름 (e.g. "openrouter")
    model_map: dict[str, str] = Field(default_factory=dict)
    # e.g. {"claude-opus-4-5": "claude-opus-4-5"}  (OpenRouter 경유)
    # e.g. {"claude-opus-4-5": "gpt-4o"}  (다른 모델로 전환)


class FailoverConfig(Base):
    """프로바이더 failover 설정."""
    enabled: bool = False
    cooldown_seconds: int = 300       # 장애 프로바이더 복구 대기 시간
    rules: list[FailoverRule] = Field(default_factory=list)
```

Config JSON 예시:

```json
{
  "failover": {
    "enabled": true,
    "cooldownSeconds": 300,
    "rules": [
      {
        "fromProvider": "anthropic",
        "toProvider": "openrouter",
        "modelMap": {
          "claude-opus-4-5": "anthropic/claude-opus-4-5"
        }
      },
      {
        "fromProvider": "openrouter",
        "toProvider": "openai",
        "modelMap": {
          "anthropic/claude-opus-4-5": "gpt-4o"
        }
      }
    ]
  }
}
```

#### 2. FailoverManager (`providers/failover.py`)

```python
class FailoverManager:
    """런타임 프로바이더 failover를 관리한다."""

    def __init__(self, config: Config, failover_config: FailoverConfig):
        self._config = config
        self._failover = failover_config
        self._circuit_breakers: dict[str, float] = {}  # provider_name -> failure_timestamp

    def is_provider_healthy(self, provider_name: str) -> bool:
        """쿨다운 기간이 지났으면 healthy로 판단."""
        if provider_name not in self._circuit_breakers:
            return True
        elapsed = time.monotonic() - self._circuit_breakers[provider_name]
        if elapsed >= self._failover.cooldown_seconds:
            del self._circuit_breakers[provider_name]
            return True
        return False

    def mark_failed(self, provider_name: str) -> None:
        """프로바이더를 일시적으로 비활성화."""
        self._circuit_breakers[provider_name] = time.monotonic()

    def get_failover_chain(self, from_provider: str, model: str) -> list[tuple[str, str]]:
        """(provider_name, mapped_model) 리스트를 반환."""
        chain = []
        current = from_provider
        visited = {from_provider}
        for _ in range(len(self._failover.rules)):  # 무한루프 방지
            rule = next((r for r in self._failover.rules if r.from_provider == current), None)
            if not rule or rule.to_provider in visited:
                break
            mapped_model = rule.model_map.get(model, model)
            chain.append((rule.to_provider, mapped_model))
            visited.add(rule.to_provider)
            current = rule.to_provider
        return chain

    async def try_failover(self, messages, tools, model, original_provider) -> LLMResponse | None:
        """failover chain을 순회하며 성공할 때까지 시도."""
        chain = self.get_failover_chain(original_provider, model)
        for provider_name, mapped_model in chain:
            if not self.is_provider_healthy(provider_name):
                continue
            try:
                provider = self._create_provider(provider_name)
                response = await provider.chat(messages=messages, tools=tools, model=mapped_model)
                if response.finish_reason != "error":
                    logger.info("Failover 성공: {} → {} (model: {})", original_provider, provider_name, mapped_model)
                    return response
                self.mark_failed(provider_name)
            except Exception as e:
                logger.warning("Failover 실패: {} — {}", provider_name, str(e)[:100])
                self.mark_failed(provider_name)
        return None
```

#### 3. `chat_with_retry` 통합 (`base.py`)

```python
async def chat_with_retry(self, ..., failover_manager=None, provider_name=None):
    # ... 기존 3회 재시도 로직 ...

    # 모든 재시도 실패 후 failover 시도
    if failover_manager and provider_name:
        failover_response = await failover_manager.try_failover(
            messages=messages, tools=tools, model=model, original_provider=provider_name
        )
        if failover_response:
            return failover_response
        failover_manager.mark_failed(provider_name)

    # failover도 실패 시 원래 에러 반환
    return await self.chat(...)  # 기존 마지막 시도
```

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `config/schema.py` | 수정 | `FailoverRule`, `FailoverConfig` 추가, `Config`에 `failover` 필드 |
| `providers/failover.py` | 신규 | `FailoverManager` 구현 |
| `providers/base.py` | 수정 | `chat_with_retry`에 failover 파라미터 추가 |
| `agent/loop.py` | 수정 | `FailoverManager` 인스턴스 생성 및 전달 |

## 검증 기준

- [ ] 프로바이더 A에 `ANTHROPIC_API_KEY=invalid` 설정 → 프로바이더 B로 자동 전환 확인
- [ ] failover chain 전체 실패 시 원래 에러 메시지 사용자에게 반환 확인
- [ ] 쿨다운 후 원래 프로바이더 자동 복구 확인
- [ ] `failover.enabled=false`일 때 기존 동작과 완전 동일 확인
