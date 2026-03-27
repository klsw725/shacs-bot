# SPEC: Prompt Caching 강화

> **Prompt**: HKUDS/nanobot, OpenClaw 분석 후 shacs-bot에 추가할 기능 — Prompt Caching 강화

## PRDs

| PRD | 설명 |
|---|---|
| [`prompt-caching-enhancement.md`](./prds/prompt-caching-enhancement.md) | `_apply_cache_control` 확장 + cache 통계 로깅 |

## TL;DR

> **목적**: 현재 system 메시지 + 마지막 tool definition에만 적용되는 `cache_control`을 **사용자 메시지 턴**, **긴 tool result**로 확장하여 Anthropic/OpenRouter 비용을 추가 절감한다.
>
> **Deliverables**:
> - `providers/litellm.py` — `_apply_cache_control` 확장
> - `config/schema.py` — `PromptCachingConfig` 추가
> - `providers/registry.py` — `supports_prompt_caching` 활용 확인
>
> **Estimated Effort**: Short (1-2시간)

## 현재 상태 분석

### 이미 구현된 것

`litellm.py:193-219`에서 `_apply_cache_control()`이 다음에만 `cache_control: {type: "ephemeral"}`을 삽입:

1. **system 메시지** — content를 `[{type: "text", text: ..., cache_control: ...}]`로 변환
2. **마지막 tool definition** — `tools[-1]`에 `cache_control` 추가

```python
# litellm.py:193-219 (현재)
def _apply_cache_control(self, messages, tools):
    for msg in messages:
        if msg.get("role") == "system":
            # system 메시지에 cache_control 삽입
    if tools:
        new_tools[-1] = {**new_tools[-1], "cache_control": {"type": "ephemeral"}}
```

### 누락된 캐싱 기회

Anthropic의 prompt caching은 **1024 토큰 이상의 연속 prefix**가 동일할 때 캐시 히트가 발생한다. 현재 놓치는 구간:

| 구간 | 설명 | 예상 캐시 절감 |
|------|------|:---:|
| 이전 사용자/어시스턴트 턴 | 멀티턴 대화에서 이전 메시지는 매번 동일 | 30-50% |
| 긴 tool result | `read_file` 등의 대용량 결과 (코드 파일 전체) | 10-20% |
| 스킬 시스템 프롬프트 | SKILL.md 내용이 system prompt에 포함될 때 | 5-10% |

## 설계

### 전략: Breakpoint 기반 캐싱

Anthropic은 최대 4개의 cache breakpoint를 허용한다. 최적 배치:

```
[breakpoint 1] system prompt (이미 구현)
[breakpoint 2] tool definitions의 마지막 (이미 구현)
[breakpoint 3] 대화 히스토리의 마지막 user 턴 ← 신규
[breakpoint 4] 마지막 긴 tool result (1024+ tokens) ← 신규
```

### 변경 사항

#### 1. `_apply_cache_control` 확장 (`litellm.py`)

```python
def _apply_cache_control(self, messages, tools):
    """Return copies of messages and tools with cache_control injected.

    Anthropic allows up to 4 cache breakpoints. Strategy:
    1. system message (existing)
    2. last tool definition (existing)
    3. last user turn in history (NEW)
    4. last large tool result >= _CACHE_MIN_TOKENS (NEW)
    """
    new_messages = []
    last_user_idx = None
    last_large_tool_idx = None

    # 1차 패스: 인덱스 탐색
    for i, msg in enumerate(messages):
        if msg.get("role") == "user":
            last_user_idx = i
        if msg.get("role") == "tool":
            content = msg.get("content", "")
            if isinstance(content, str) and len(content) >= self._CACHE_MIN_CHARS:
                last_large_tool_idx = i

    # 2차 패스: cache_control 삽입
    for i, msg in enumerate(messages):
        if msg.get("role") == "system":
            # 기존 로직 유지
            new_messages.append(self._inject_cache_control(msg))
        elif i == last_user_idx:
            new_messages.append(self._inject_cache_control(msg))
        elif i == last_large_tool_idx:
            new_messages.append(self._inject_cache_control(msg))
        else:
            new_messages.append(msg)

    # tool definitions (기존 로직 유지)
    new_tools = tools
    if tools:
        new_tools = list(tools)
        new_tools[-1] = {**new_tools[-1], "cache_control": {"type": "ephemeral"}}

    return new_messages, new_tools
```

#### 2. 설정 추가 (`config/schema.py`)

현재는 설정 불필요. `_CACHE_MIN_CHARS = 4000`을 `litellm.py` 클래스 상수로 정의하되, 필요 시 config로 이동할 수 있게 한다.

#### 3. 캐시 통계 로깅

LLMResponse의 `usage` dict에 Anthropic이 반환하는 `cache_creation_input_tokens`, `cache_read_input_tokens` 필드를 파싱하여 로깅한다.

```python
# litellm.py _parse_response 확장
if hasattr(response, "usage") and response.usage:
    usage = {
        "prompt_tokens": response.usage.prompt_tokens,
        "completion_tokens": response.usage.completion_tokens,
        "total_tokens": response.usage.total_tokens,
        # cache stats (Anthropic-specific)
        "cache_creation_input_tokens": getattr(response.usage, "cache_creation_input_tokens", 0),
        "cache_read_input_tokens": getattr(response.usage, "cache_read_input_tokens", 0),
    }
    if usage.get("cache_read_input_tokens"):
        logger.debug("Prompt cache hit: {} tokens read from cache", usage["cache_read_input_tokens"])
```

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `providers/litellm.py` | 수정 | `_apply_cache_control` 확장, `_CACHE_MIN_CHARS` 상수, cache stats 로깅 |
| `providers/base.py` | 수정 | `LLMResponse.usage`에 cache 필드 문서화 |

## 검증 기준

- [ ] Anthropic 모델로 3턴 이상 대화 시, 3번째 턴부터 `cache_read_input_tokens > 0` 확인
- [ ] OpenRouter 경유 시에도 동일 동작 확인
- [ ] cache 미지원 프로바이더(OpenAI, DeepSeek 등)에서 `cache_control` 키가 요청에 포함되지 않음 확인
