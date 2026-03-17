# SPEC: 코드 품질 리팩토링

> **Prompt**: 코드를 지능적으로 리팩토링하고 품질을 개선합니다

---

## 1. 현황 분석

### 1.1 코드베이스 상태 분류: **Transitional**

- 전반적으로 일관된 패턴이 있으나, 빠른 기능 추가 과정에서 중복과 드리프트가 누적됨
- AGENTS.md에 명확한 컨벤션이 있으나 일부 위반 존재
- 테스트 인프라가 아직 미구축 (pyproject.toml에 dev 의존성만 존재, tests/ 디렉토리 없음)

### 1.2 정량 요약

| 지표 | 값 |
|---|---|
| Python 파일 수 | 63 |
| 런타임 버그 | 2 (크래시 유발) |
| 컨벤션 위반 | 4건 (`Union`, `Optional`, `print()`) |
| 코드 중복 | 3건 (split_message, 도구 등록, 채널 초기화) |
| `except Exception:` 남용 | 45개소 |
| 스키마 드리프트 | 5 provider (registry ↔ config 불일치) |
| 복잡 메서드 (50줄+) | ~10개 |

---

## 2. 발견 상세

### 2.1 런타임 버그 (즉시 수정 필요)

#### BUG-1: `agent/memory.py:88` — NameError (정의되지 않은 변수)

`MemoryStore.consolidate()` 메서드 (L66-171)에 두 버전의 consolidation 로직이 공존한다.

```python
async def consolidate(self, messages, provider, model) -> bool:
    # --- 버전 A (L76-85): prompt 빌드 ---
    current_memory = self.read_long_term()
    prompt = f"..."  # messages 기반 prompt

    # --- 버전 B (L88-128): 다른 로직 (정의되지 않은 변수 참조) ---
    if archive_all:                         # NameError: archive_all 미정의
        old_messages = session.messages      # NameError: session 미정의
    else:
        keep_count = memory_window // 2      # NameError: memory_window 미정의
```

**원인**: 리팩토링 과정에서 이전 버전 코드가 삭제되지 않고 남음.
**영향**: `MemoryConsolidator.consolidate_messages()` → `self._store.consolidate()` 호출 시 즉시 크래시.
**수정**: 버전 B (L87-128) 제거. 버전 A의 prompt 빌드 후 바로 L131의 `try: response = await provider.chat_with_retry(...)` 로 연결.

#### BUG-2: `cli/commands.py:442, 471` — `await` 누락

```python
async def _pick_heartbeat_target() -> tuple[str, str]:  # L450: async 함수
    ...

# L442 (on_heartbeat_notify 내부):
channel, chat_id = _pick_heartbeat_target()   # await 없음 → TypeError

# L471 (on_heartbeat_execute 내부):
channel, chat_id = _pick_heartbeat_target()   # await 없음 → TypeError
```

**영향**: heartbeat 트리거 시 `TypeError: cannot unpack non-iterable coroutine object`.
**수정**: 두 호출 지점에 `await` 추가.

---

### 2.2 컨벤션 위반

#### CONV-1: `Union[X, Y]` 사용 — `providers/litellm.py`

AGENTS.md 규칙: "`Optional[X]`, `Union[X, Y]` 사용 금지. `str | None`, `X | Y`만 사용."

| 위치 | 현재 | 수정 |
|---|---|---|
| L7 | `from typing import Any, Union` | `from typing import Any` |
| L161 | `Union[ModelResponse, CustomStreamWrapper]` | `ModelResponse \| CustomStreamWrapper` |
| L283 | `Union[ModelResponse, CustomStreamWrapper]` | `ModelResponse \| CustomStreamWrapper` |
| L285 | `Union[Choices, StreamingChoices]` | `Choices \| StreamingChoices` |

#### CONV-2: `Optional[X]` 사용 — `skills/skill-creator/scripts/quick_validate.py`

L9의 `from typing import Optional` 및 7곳의 `Optional[...]` 사용. 독립 스크립트이므로 우선순위 낮음.

#### CONV-3: `print()` 사용 — `config/loader.py:57-58`

```python
# 현재 (런타임 코드에서 print 사용)
print(f"Warning: Failed to load config from {config_file}: {e}")
print("Using default configuration.")

# 수정 (loguru 사용)
logger.warning("Failed to load config from {}: {}", config_file, e)
logger.warning("Using default configuration.")
```

`config/loader.py`는 부팅 시점에 호출되는 런타임 코드이므로 loguru 사용이 적절.

---

### 2.3 코드 중복

#### DUP-1: `_split_message()` 3중 정의

| 파일 | 함수명 | max_len | codeblock 인식 |
|---|---|---|---|
| `utils/helpers.py:49` | `split_message()` | 2000 | O |
| `channels/telegram.py:85` | `_split_message()` | 4000 | X |
| `channels/discord.py:23` | `_split_message()` | 2000 | X |

**전략**: `utils/helpers.py`의 `split_message()`을 정본으로 사용. Telegram/Discord에서 import하여 `max_len` 매개변수만 전달.

#### DUP-2: 도구 등록 이중화 — `AgentLoop` + `SubagentManager`

- `agent/loop.py:135-161` — `_register_default_tools()`
- `agent/subagent.py:207-221` — 동일 도구 독립 생성

**전략**: 공용 팩토리 함수 `create_default_tools(workspace, ...)` 추출. 양쪽에서 호출.

#### DUP-3: 채널 초기화 반복 — `channels/manager.py:37-153`

10개 채널이 동일한 12줄 패턴 반복. 총 ~120줄 → 데이터 드리븐 루프 ~15줄로 축소.

```python
# 현재: 10개 if 블록 (각 12줄)
if self._config.channels.telegram.enabled:
    try:
        from shacs_bot.channels.telegram import TelegramChannel
        self._channels["telegram"] = TelegramChannel(config, bus)
    except ImportError as e:
        logger.warning(...)

# 개선: 선언적 채널 레지스트리
_CHANNEL_REGISTRY: tuple[tuple[str, str, str], ...] = (
    ("telegram", "shacs_bot.channels.telegram", "TelegramChannel"),
    ("whatsapp", "shacs_bot.channels.whatsapp", "WhatsAppChannel"),
    ...
)

for name, module_path, class_name in _CHANNEL_REGISTRY:
    config_section = getattr(self._config.channels, name, None)
    if config_section and config_section.enabled:
        ...
```

**주의**: Telegram은 `groq_api_key` 추가 인자가 필요하므로 레지스트리에 `extra_kwargs` 매핑 포함 필요.

---

### 2.4 스키마 드리프트

#### DRIFT-1: Provider registry ↔ ProvidersConfig 불일치

`providers/registry.py`에 ProviderSpec이 존재하나 `config/schema.py`의 `ProvidersConfig`에 필드가 없는 provider:

| Provider | registry.py | ProvidersConfig |
|---|---|---|
| `azure_openai` | O (L82) | X |
| `volcengine_coding_plan` | O (L166) | X |
| `byteplus` | O (L184) | X |
| `byteplus_coding_plan` | O (L202) | X |
| `ollama` | O (L419) | X |

**영향**: `Config._match_provider()` 내 `getattr(self.providers, spec.name, None)` → `None` 반환. config.json으로 이들 provider의 API 키/base_url 설정 불가.

**수정**: `ProvidersConfig`에 누락 필드 추가.

---

### 2.5 구조적 복잡도

#### COMPLEX-1: `AgentLoop._process_message()` — ~160줄

현재 책임:
1. 시스템 메시지 분기 처리
2. 슬래시 명령어 라우팅 (/new, /help)
3. 메모리 consolidation 트리거 (pre/post)
4. 도구 컨텍스트 설정
5. 메시지 빌드 + LLM 호출
6. MessageTool 상태 확인 + 응답 라우팅

**전략** (이번 스코프): 슬래시 명령 핸들러만 별도 메서드로 추출. 나머지는 현 구조 유지.

#### COMPLEX-2: `except Exception:` 45개소

대부분 방어적 프로그래밍으로 의도된 것이나, 디버깅 시 원인 추적이 어려움.

**전략** (이번 스코프): 런타임 핵심 경로(agent/loop.py, agent/memory.py, providers/)의 `except Exception:`만 구체화. 채널 코드는 현 상태 유지 (외부 API 의존 → 예외 범위 예측 어려움).

---

## 3. 스코프 정의

### In-Scope (이번 리팩토링)

| # | 항목 | 유형 | 난이도 |
|---|---|---|---|
| 1 | BUG-1: memory.py 데드 코드 제거 | 버그 수정 | 쉬움 |
| 2 | BUG-2: commands.py await 추가 | 버그 수정 | 쉬움 |
| 3 | CONV-1: litellm.py Union 제거 | 컨벤션 | 쉬움 |
| 4 | CONV-2: quick_validate.py Optional 제거 | 컨벤션 | 쉬움 |
| 5 | CONV-3: loader.py print→loguru | 컨벤션 | 쉬움 |
| 6 | DUP-1: split_message 통합 | 중복 제거 | 쉬움 |
| 7 | DUP-2: 도구 등록 팩토리 추출 | 중복 제거 | 중간 |
| 8 | DUP-3: 채널 초기화 데이터 드리븐 | 중복 제거 | 중간 |
| 9 | DRIFT-1: ProvidersConfig 필드 추가 | 스키마 동기화 | 쉬움 |
| 10 | COMPLEX-2: 핵심 경로 except 구체화 | 에러 처리 | 중간 |

### Out-of-Scope

- COMPLEX-1 (`_process_message` 분할) — 대규모 구조 변경, 별도 작업으로 분리
- 테스트 커버리지 추가 — 테스트 인프라 자체가 미구축
- `ProvidersConfig(BaseModel)` vs `Base` 변경 — 의도적 설계일 가능성, 확인 필요

---

## 4. 리팩토링 원칙

1. **외부 동작 보존**: 모든 변경은 내부 구조 개선만. API, 설정 포맷, 메시지 흐름 변경 없음
2. **외과적 변경**: 각 항목은 독립적으로 적용/롤백 가능한 최소 단위
3. **점진적 적용**: 버그 수정 → 컨벤션 → 중복 제거 → 구조 개선 순서
4. **기존 패턴 존중**: 코드 스타일, 네이밍, 주석 언어(한국어) 모두 기존 관례 따름

---

## 5. 위험 요소

| 위험 | 영향 | 완화 |
|---|---|---|
| DUP-2 (도구 팩토리): SubagentManager 동작 변경 | 서브에이전트 도구 누락 가능 | 기존 도구 목록 정확히 보존, 등록 순서 유지 |
| DUP-3 (채널 레지스트리): Telegram의 groq_api_key 등 특수 인자 | 채널 초기화 실패 | extra_kwargs 매핑으로 처리 |
| DRIFT-1 (스키마 추가): 기존 config.json에 새 필드 출현 | config 직렬화 시 불필요한 빈 필드 | default_factory로 빈 기본값 유지, 기존 동작 변경 없음 |
| COMPLEX-2 (except 구체화): 예상 못한 예외 타입 누락 | 미처리 예외로 크래시 | 핵심 경로만 대상, 구체 타입 + 마지막에 `except Exception` 폴백 유지 |
