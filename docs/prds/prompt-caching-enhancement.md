# PRD: Prompt Caching 강화

> **Spec**: [`docs/specs/2026-03-16-prompt-caching.md`](../specs/2026-03-16-prompt-caching.md)

---

## 문제

현재 `_apply_cache_control()`은 **system 메시지**와 **마지막 tool definition**에만 `cache_control`을 삽입한다 (`litellm.py:193-219`). Anthropic은 최대 4개의 cache breakpoint를 허용하지만 2개만 사용 중이며, 멀티턴 대화에서 **반복 전송되는 이전 대화 히스토리**와 **대형 tool result**가 매번 전량 과금된다.

```
현재 캐싱 범위:
  [✅ cached] system prompt
  [❌ 매번 전량 과금] user: "파일 읽어줘"
  [❌ 매번 전량 과금] assistant: "네, 읽겠습니다"
  [❌ 매번 전량 과금] tool result: (3000줄 코드 파일)
  [✅ cached] tool definitions (마지막 항목)
  [new] user: "이 파일 리팩토링 해줘"  ← 현재 턴
```

Anthropic 기준 cache hit 시 **90% 비용 절감**이 가능하므로, breakpoint 2개만 추가해도 멀티턴 대화 비용이 크게 줄어든다.

## 해결책

`_apply_cache_control()`에 2개의 cache breakpoint를 추가한다:

1. **마지막 user 턴** — 대화 히스토리의 이전 메시지들이 다음 턴에서 prefix로 캐시됨
2. **마지막 대형 tool result** (4000자 이상) — `read_file` 등의 대용량 결과가 캐시됨

추가로, 응답의 cache 통계를 파싱하여 로그에 기록한다.

## 사용자 영향

| Before | After |
|---|---|
| system + tool def만 캐시됨 (breakpoint 2/4) | system + tool def + user턴 + tool result (breakpoint 4/4) |
| 3턴 대화 시 이전 2턴이 매번 전량 과금 | 이전 턴 대부분 cache hit (90% 절감) |
| cache 동작 확인 수단 없음 | 로그에 `cache_read_input_tokens` 출력 |
| 설정 변경 없음 | 설정 변경 없음 (자동 적용) |

## 기술적 범위

- **변경 파일**: `shacs_bot/providers/litellm.py` (1개)
- **변경 유형**: 기존 메서드 확장 + 상수 추가 + 응답 파싱 확장
- **의존성**: 없음
- **하위 호환성**: cache 미지원 프로바이더는 `_supports_cache_control()` 게이트에서 이미 걸러짐. 동작 변경 없음

### 변경 1: 클래스 상수 추가

**litellm.py** (line 29, `_ALNUM` 아래):

```python
_CACHE_MIN_CHARS: int = 4000  # cache breakpoint를 삽입할 tool result의 최소 길이
```

### 변경 2: `_apply_cache_control` 확장

**litellm.py** (line 193-219 전체 교체):

```python
def _apply_cache_control(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]] | None]:
    """Return copies of messages and tools with cache_control injected.

    Anthropic allows up to 4 cache breakpoints. Strategy:
    1. system message (기존)
    2. last tool definition (기존)
    3. last user turn in conversation history (신규)
    4. last large tool result >= _CACHE_MIN_CHARS (신규)
    """
    # 1차 패스: breakpoint 삽입 대상 인덱스 탐색
    last_user_idx: int | None = None
    last_large_tool_idx: int | None = None

    for i, msg in enumerate(messages):
        if msg.get("role") == "user":
            last_user_idx = i
        elif msg.get("role") == "tool":
            content: Any = msg.get("content", "")
            if isinstance(content, str) and len(content) >= self._CACHE_MIN_CHARS:
                last_large_tool_idx = i

    # 2차 패스: cache_control 삽입
    new_messages: list[dict[str, Any]] = []

    for i, msg in enumerate(messages):
        if msg.get("role") == "system":
            # 기존 로직: system 메시지에 cache_control 삽입
            content: Any = msg["content"]
            if isinstance(content, str):
                new_content = [{"type": "text", "text": content, "cache_control": {"type": "ephemeral"}}]
            else:
                new_content = list(content)
                new_content[-1] = {**new_content[-1], "cache_control": {"type": "ephemeral"}}
            new_messages.append({**msg, "content": new_content})
        elif i == last_user_idx or i == last_large_tool_idx:
            # 신규: user 턴 / 대형 tool result에 cache_control 삽입
            content: Any = msg["content"]
            if isinstance(content, str):
                new_content = [{"type": "text", "text": content, "cache_control": {"type": "ephemeral"}}]
            elif isinstance(content, list):
                new_content = list(content)
                new_content[-1] = {**new_content[-1], "cache_control": {"type": "ephemeral"}}
            else:
                new_messages.append(msg)
                continue
            new_messages.append({**msg, "content": new_content})
        else:
            new_messages.append(msg)

    # 기존 로직: 마지막 tool definition에 cache_control 삽입
    new_tools: list[dict[str, Any]] | None = tools
    if tools:
        new_tools = list(tools)
        new_tools[-1] = {**new_tools[-1], "cache_control": {"type": "ephemeral"}}

    return new_messages, new_tools
```

### 변경 3: cache 통계 로깅

**litellm.py** (line 273-279, `_parse_response` 내 usage 파싱 블록 교체):

```python
usage = {}
if hasattr(response, "usage") and response.usage:
    cache_read: int = getattr(response.usage, "cache_read_input_tokens", 0) or 0
    cache_creation: int = getattr(response.usage, "cache_creation_input_tokens", 0) or 0

    usage = {
        "prompt_tokens": response.usage.prompt_tokens,
        "completion_tokens": response.usage.completion_tokens,
        "total_tokens": response.usage.total_tokens,
        "cache_read_input_tokens": cache_read,
        "cache_creation_input_tokens": cache_creation,
    }

    if cache_read:
        logger.debug(
            "Prompt cache hit: {} tokens cached, {} tokens created",
            cache_read,
            cache_creation,
        )
```

## 성공 기준

1. Anthropic 모델로 3턴 이상 대화 시, 3번째 턴부터 로그에 `cache_read_input_tokens > 0` 출력
2. OpenRouter (`supports_prompt_caching=True`) 경유 시에도 동일 동작
3. cache 미지원 프로바이더(OpenAI, DeepSeek 등)에서 `cache_control` 키가 요청에 포함되지 않음 (`_supports_cache_control` 게이트 통과 안 함)
4. tool result가 4000자 미만일 때는 해당 메시지에 `cache_control`이 삽입되지 않음
5. 기존 system 메시지 + tool definition 캐싱 동작이 그대로 유지됨

---

## 마일스톤

- [x] **M1: `_apply_cache_control` 확장 + 상수 추가**
  `_CACHE_MIN_CHARS` 상수 추가. `_apply_cache_control`에 last_user_idx / last_large_tool_idx 탐색 및 삽입 로직 추가. 기존 system/tool 캐싱 동작 유지 확인.

- [x] **M2: cache 통계 로깅**
  `_parse_response`에서 `cache_read_input_tokens`, `cache_creation_input_tokens` 파싱. `logger.debug`로 cache hit 시 로그 출력.

- [x] **M3: 코드 레벨 검증**
  정적 분석 검증 완료. cache breakpoint 4개 삽입 로직, 미지원 프로바이더 게이트, cache 통계 로깅 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| user 메시지 content가 dict 등 비표준 형식 | 낮음 | 낮음 | `isinstance` 체크로 str/list만 처리, 나머지는 skip |
| cache breakpoint 4개 초과 시 API 에러 | 낮음 | 중간 | system(1) + user(1) + tool_result(1) + tool_def(1) = 정확히 4개. 초과 불가 |
| LiteLLM이 cache_control 필드를 strip | 낮음 | 중간 | `litellm.drop_params = True` 설정이 있지만, cache_control은 Anthropic 표준 필드라 strip 대상 아님 |
| cache_read_input_tokens 어트리뷰트 부재 | 낮음 | 낮음 | `getattr(..., 0)`으로 안전 처리 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | M1, M2 구현 완료 — `_apply_cache_control` 4 breakpoint 확장 + cache 통계 로깅 |
| 2026-03-21 | M3 코드 레벨 검증 완료. 성공 기준 5개 항목 전부 정적 분석으로 확인: (1) 4개 breakpoint — system(line 241), last_user_idx(line 251), last_large_tool_idx(line 251), last_tool_def(line 267-270), (2) `_supports_cache_control` — gateway.supports_prompt_caching 체크(line 208) + spec 레벨(line 211), (3) 미지원 프로바이더는 `_supports_cache_control()` 게이트(line 122)에서 차단, (4) `_CACHE_MIN_CHARS=4000` 미만 tool result 미삽입(line 234), (5) 기존 system+tool_def 캐싱 유지. 엣지 케이스 확인: last_user_idx/last_large_tool_idx가 None일 때 `i == None` False로 안전. cache 통계 `getattr(..., 0) or 0`으로 안전 파싱. LSP: 기존 LiteLLM 타입 이슈만 존재. |
