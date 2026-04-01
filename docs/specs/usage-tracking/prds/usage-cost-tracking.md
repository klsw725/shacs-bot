# PRD: Usage & Cost Tracking

---

## 문제

현재 LLM 응답의 토큰 사용량은 OpenTelemetry span 속성(`tokens.prompt`, `tokens.completion`)으로만 기록된다. 이는 Jaeger 같은 외부 트레이싱 시스템을 설정해야만 확인 가능하며, **일반 사용자에게는 비용 가시성이 전혀 없다.**

```
사용자 → Telegram에서 대화 → 토큰을 얼마나 썼는지 모름 → API 대시보드에서 놀라운 청구서 발견
```

또한:
- `_run_agent_loop`에서 여러 번 LLM을 호출하지만(도구 호출 루프), **누적 usage를 추적하지 않는다**.
- 세션별, 일별 사용량을 조회할 방법이 없다.
- 채널 안에서 현재 모델이나 세션 상태를 확인할 방법이 `/help` 뿐이다.

## 해결책

`UsageTracker` 모듈을 추가하여:

1. **턴(turn) 단위 usage 누적**: `_run_agent_loop`의 매 LLM 호출마다 토큰 사용량을 누적
2. **비용 계산**: `litellm.completion_cost()`를 활용한 실시간 비용 산출
3. **응답 footer**: 채널 메시지에 토큰/비용 요약을 선택적으로 표시
4. **슬래시 커맨드**: `/usage`, `/status` 추가

## 사용자 영향

| Before | After |
|---|---|
| 토큰 사용량 확인 불가 | 응답마다 footer에 토큰/비용 표시 (opt-in) |
| 비용 파악을 위해 프로바이더 대시보드 필요 | `/usage` 커맨드로 세션/일별 비용 즉시 조회 |
| 채널에서 현재 모델/세션 정보 확인 불가 | `/status`로 모델, 프로바이더, 세션 토큰 수 확인 |
| OTel 없으면 사용량 데이터 유실 | JSONL 파일에 영구 저장 |

## 기술적 범위

- **신규 파일**: 1개 (`agent/usage.py`)
- **수정 파일**: 4개 (`agent/loop.py`, `config/schema.py`, `bus/events.py`, `providers/base.py`)
- **의존성**: 없음 (litellm은 이미 의존성에 존재)
- **하위 호환성**: `usage.enabled=false`(기본값)이면 기존 동작과 100% 동일

---

## 상세 설계

### 변경 1: UsageTracker 모듈 (`agent/usage.py` 신규)

턴 단위 토큰/비용 누적 + JSONL 영구 저장을 담당한다.

```python
@dataclass
class TurnUsage:
    """하나의 대화 턴(사용자 메시지 → 최종 응답)에서의 누적 사용량."""

    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0
    cache_read_tokens: int = 0
    cache_creation_tokens: int = 0
    llm_calls: int = 0  # 도구 호출 루프에서의 LLM 호출 횟수
    cost_usd: float = 0.0  # 누적 비용 (USD)
    model: str = ""
    provider: str = ""

    def accumulate(self, usage: dict[str, int], model: str, provider: str) -> None:
        """LLMResponse.usage를 누적한다."""
        ...

    def format_footer(self, mode: str) -> str:
        """채널 메시지에 붙일 footer 문자열을 생성한다."""
        # mode == "tokens": "📊 1.2K tokens"
        # mode == "full":   "📊 1.2K tokens · $0.012 · 3 calls · claude-sonnet-4"
        ...


class UsageTracker:
    """세션별/일별 사용량 추적 및 영구 저장."""

    def __init__(self, data_dir: Path) -> None:
        self._data_dir = data_dir / "usage"
        ...

    def record(self, session_key: str, turn: TurnUsage) -> None:
        """턴 사용량을 JSONL 파일에 기록한다."""
        # 파일: {data_dir}/usage/{YYYY-MM-DD}.jsonl
        # 행: {"ts": "...", "session": "...", "model": "...", "prompt": N, "completion": N, "cost": N}
        ...

    def get_session_summary(self, session_key: str) -> dict:
        """현재 세션의 누적 사용량을 반환한다."""
        ...

    def get_daily_summary(self, date: str | None = None) -> dict:
        """일별 전체 사용량을 반환한다. date=None이면 오늘."""
        ...
```

#### 비용 계산

litellm의 `completion_cost()`를 사용한다. 이미 프로젝트 의존성이며, 모델별 가격 테이블을 내장하고 있다.

```python
from litellm import completion_cost

cost = completion_cost(
    model=model,
    prompt_tokens=usage["prompt_tokens"],
    completion_tokens=usage["completion_tokens"],
)
```

단, `completion_cost()`가 지원하지 않는 모델(custom, vllm, ollama 등)은 비용을 0으로 처리하고, footer에 비용 대신 토큰 수만 표시한다.

#### 저장 형식

```jsonl
{"ts":"2026-03-21T14:30:00","session":"telegram:12345","model":"anthropic/claude-sonnet-4","provider":"anthropic","prompt":1200,"completion":350,"cache_read":800,"cost":0.0042,"calls":2}
{"ts":"2026-03-21T14:31:00","session":"discord:67890","model":"openrouter/anthropic/claude-opus-4","provider":"openrouter","prompt":3500,"completion":1200,"cache_read":0,"cost":0.0891,"calls":5}
```

파일 위치: `~/.shacs-bot/data/usage/{YYYY-MM-DD}.jsonl`

### 변경 2: Config 추가 (`config/schema.py`)

`ObservabilityConfig` 아래에 추가:

```python
class UsageConfig(Base):
    """Usage tracking configuration."""

    enabled: bool = True  # 사용량 추적 활성화 (기록은 항상, 표시만 제어)
    footer: str = "off"   # 응답 footer 모드: "off" | "tokens" | "full"
```

`Config` 클래스에 필드 추가:

```python
usage: UsageConfig = Field(default_factory=UsageConfig)
```

config.json 예시:
```json
{
  "usage": {
    "enabled": true,
    "footer": "full"
  }
}
```

### 변경 3: _run_agent_loop 수정 (`agent/loop.py`)

#### 3-1. TurnUsage 누적

현재 `_run_agent_loop`는 `(final_content, tools_used, messages)`를 반환한다. 여기에 `TurnUsage`를 추가한다.

```python
async def _run_agent_loop(
    self,
    init_messages: list[dict[str, Any]],
    on_progress: Callable[..., Awaitable[None]] | None = None,
) -> tuple[str | None, list[str], list[dict[str, Any]], TurnUsage]:
    """에이전트 반복 루프를 실행합니다. (final_content, tools_used, messages, usage)를 반환합니다."""

    turn_usage = TurnUsage(model=self._model, provider=self._provider_name or "")
    ...

    for _ in range(self._max_iterations):
        response = await self._provider.chat_with_retry(...)

        # 기존 OTel span 로깅 유지
        if llm_span and response.finish_reason != "error":
            llm_span.set_attribute("tokens.prompt", response.usage.get("prompt_tokens", 0))
            ...

        # 신규: 누적
        turn_usage.accumulate(response.usage, self._model, self._provider_name or "")
        ...

    return final_content, tools_used, messages, turn_usage
```

#### 3-2. handle_message에서 footer 추가 + 기록

```python
async def handle_message(self, msg: InboundMessage, ...) -> OutboundMessage | None:
    ...
    final_content, _, all_msg, turn_usage = await self._run_agent_loop(...)

    # 사용량 기록 (항상)
    if self._usage_tracker:
        self._usage_tracker.record(session_key=key, turn=turn_usage)

    # footer 추가 (설정에 따라)
    if self._usage_config and self._usage_config.footer != "off":
        footer = turn_usage.format_footer(mode=self._usage_config.footer)
        if footer:
            final_content = f"{final_content}\n\n{footer}"
    ...
```

### 변경 4: 슬래시 커맨드 추가 (`agent/loop.py`)

기존 `/new`, `/help` 분기 뒤에 추가:

```python
elif cmd == "/usage":
    if not self._usage_tracker:
        return OutboundMessage(channel=msg.channel, chat_id=msg.chat_id,
            content="사용량 추적이 비활성화되어 있습니다.")

    session_summary = self._usage_tracker.get_session_summary(key)
    daily_summary = self._usage_tracker.get_daily_summary()

    content = f"""📊 사용량 요약

**현재 세션** ({key})
• 토큰: {session_summary['prompt']:,} prompt + {session_summary['completion']:,} completion
• 비용: ${session_summary['cost']:.4f}
• LLM 호출: {session_summary['calls']}회

**오늘 전체**
• 토큰: {daily_summary['total']:,}
• 비용: ${daily_summary['cost']:.4f}
• 세션 수: {daily_summary['sessions']}"""

    return OutboundMessage(channel=msg.channel, chat_id=msg.chat_id, content=content)

elif cmd == "/status":
    session = self._sessions.get_or_create(key=key)
    msg_count = len(session.messages) - session.last_consolidated
    total_count = len(session.messages)

    content = f"""🤖 상태

• 모델: {self._model}
• 프로바이더: {self._provider_name or 'auto'}
• 세션: {key}
• 메시지: {msg_count}개 (미통합) / {total_count}개 (전체)
• 메모리 윈도우: {self._memory_window}"""

    return OutboundMessage(channel=msg.channel, chat_id=msg.chat_id, content=content)
```

`/help` 응답 텍스트도 갱신:

```python
content = """
    🦈 shacs-bot 명령어:
    /new — 새 대화를 시작합니다
    /stop — 현재 작업을 중지합니다
    /restart — 봇을 재시작합니다
    /usage — 토큰 사용량과 비용을 확인합니다
    /status — 현재 모델, 세션 상태를 확인합니다
    /help — 사용 가능한 명령어를 표시합니다
"""
```

---

## Footer 표시 예시

### `footer: "tokens"`
```
안녕하세요! 무엇을 도와드릴까요?

📊 1.2K tokens
```

### `footer: "full"`
```
파일을 수정했습니다. 변경 사항을 확인해주세요.

📊 8.5K tokens · $0.034 · 5 calls · claude-sonnet-4
```

### `footer: "off"` (기본값)
```
파일을 수정했습니다. 변경 사항을 확인해주세요.
```

---

## 고려사항

### 1. 서브에이전트 비용

`SubagentManager.spawn()`에서 실행되는 서브에이전트 LLM 호출은 별도 `AgentLoop`에서 실행된다. 현재 PRD 범위에서는 **메인 에이전트의 직접 호출만 추적**한다. 서브에이전트 비용 추적은 후속 작업으로 분리.

### 2. Heartbeat 비용

`HeartbeatService`의 LLM 호출도 별도 루프이다. 마찬가지로 현재 범위에서 제외. 추후 동일한 `UsageTracker`를 주입하여 추적 가능.

### 3. 비용 미지원 모델

Ollama, vLLM, custom 프로바이더 등 `litellm.completion_cost()`가 가격을 모르는 모델은:
- 비용 = 0으로 기록
- footer에서 비용 부분 생략, 토큰 수만 표시
- `/usage`에서 `비용: 해당 없음` 표시

### 4. JSONL 파일 로테이션

일별 파일(`YYYY-MM-DD.jsonl`)이므로 자연 로테이션된다. 별도 정리 로직은 필요 없다. 디스크 사용량이 우려되면 후속 작업으로 retention 정책 추가.

### 5. 캐시 토큰 비용

Anthropic prompt caching 사용 시 `cache_read_input_tokens`은 일반 prompt 토큰 대비 90% 할인이다. `litellm.completion_cost()`가 이를 자동 처리하므로 별도 로직 불필요.

---

## 구현 순서

| 단계 | 작업 | 의존성 | 상태 |
|------|------|--------|------|
| 1 | `UsageConfig` 추가 (`config/schema.py`) | 없음 | ✅ |
| 2 | `TurnUsage` + `UsageTracker` 구현 (`agent/usage.py`) | 1 | ✅ |
| 3 | `_run_agent_loop` 반환값에 `TurnUsage` 추가 | 2 | ✅ |
| 4 | `handle_message`에서 기록 + footer 추가 | 3 | ✅ |
| 5 | `/usage`, `/status` 슬래시 커맨드 추가 | 4 | ✅ |
| 6 | `/help` 텍스트 갱신 | 5 | ✅ |

**완료**: 2026-03-21 구현 완료.
