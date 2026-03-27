# PRD: 코드 품질 리팩토링

> **Spec**: [`docs/specs/2026-03-17-code-quality-refactoring.md`](../spec.md)

---

## 문제

빠른 기능 추가 과정에서 코드베이스에 아래 문제가 누적되었다:

1. **런타임 크래시를 유발하는 2개 버그** — 메모리 통합과 heartbeat 경로에 잠복
2. **AGENTS.md 컨벤션 위반** — `Union`, `Optional`, `print()` 사용
3. **코드 중복 3건** — 동일 로직이 2-3곳에 분산, 수정 시 누락 위험
4. **Provider 스키마 드리프트** — 5개 provider가 config.json으로 설정 불가
5. **광범위한 `except Exception:`** — 45개소에서 예외 원인 추적 어려움

이 상태로는 기능 추가 시마다 버그와 불일치가 가속된다.

## 해결책

10개 항목을 **버그 수정 → 컨벤션 통일 → 중복 제거 → 스키마 동기화 → 에러 처리 개선** 순서로 점진 적용한다. 모든 변경은 외부 동작 보존.

## 사용자 영향

| Before | After |
|---|---|
| 메모리 consolidation 호출 시 크래시 | 정상 동작 |
| heartbeat 알림 전달 시 크래시 | 정상 동작 |
| 5개 provider config 설정 불가 | config.json으로 모두 설정 가능 |
| 코드 일관성 없음 (Union, print 혼재) | AGENTS.md 컨벤션 100% 준수 |
| split_message 수정 시 3곳 변경 필요 | 1곳만 수정 |
| 도구 추가 시 2곳 등록 필요 | 1곳만 등록 |
| 새 채널 추가 시 ~15줄 보일러플레이트 | 레지스트리에 1줄 추가 |

## 기술적 범위

- **변경 파일**: ~12개
- **변경 유형**: 버그 수정, 리팩토링 (기능 추가 아님)
- **의존성 변경**: 없음
- **하위 호환성**: config.json 포맷 변경 없음. 새 provider 필드는 빈 기본값

---

## 변경 상세

### Phase 1: 런타임 버그 수정

#### 변경 1: memory.py — 데드 코드 제거

**파일**: `shacs_bot/agent/memory.py`

`MemoryStore.consolidate()` 메서드에서 리팩토링 잔재(L87-128)를 제거한다. 정의되지 않은 변수(`archive_all`, `session`, `memory_window`)를 참조하는 코드 블록과 중복된 첫 번째 prompt 빌드(L76-85)를 정리하여, 메서드가 정상적으로 LLM 호출까지 도달하도록 한다.

변경 후 메서드 흐름:
```
consolidate(messages, provider, model)
  ├── 빈 메시지 체크 → return True
  ├── _format_messages()로 prompt 빌드
  ├── provider.chat_with_retry() 호출
  ├── save_memory 도구 응답 파싱
  └── MEMORY.md / HISTORY.md 저장
```

#### 변경 2: commands.py — await 추가

**파일**: `shacs_bot/cli/commands.py`

L442, L471의 `_pick_heartbeat_target()` 호출에 `await` 키워드를 추가한다.

```python
# Before
channel, chat_id = _pick_heartbeat_target()

# After
channel, chat_id = await _pick_heartbeat_target()
```

---

### Phase 2: 컨벤션 통일

#### 변경 3: litellm.py — Union 제거

**파일**: `shacs_bot/providers/litellm.py`

`from typing import Any, Union` → `from typing import Any` 로 변경하고, `Union[X, Y]` 타입 힌트 3곳을 `X | Y`로 교체한다. Python 3.11+ 타겟이므로 `__future__` import 불필요.

#### 변경 4: quick_validate.py — Optional 제거

**파일**: `shacs_bot/skills/skill-creator/scripts/quick_validate.py`

`from typing import Optional` → 삭제. `Optional[X]` 7곳을 `X | None`으로 교체.

#### 변경 5: loader.py — print → loguru

**파일**: `shacs_bot/config/loader.py`

`print()` 2줄을 `logger.warning()`으로 교체. loguru import 추가.

---

### Phase 3: 코드 중복 제거

#### 변경 6: split_message 통합

**변경 파일**: `shacs_bot/channels/telegram.py`, `shacs_bot/channels/discord.py`

두 채널의 모듈 수준 `_split_message()` 함수를 삭제하고, `shacs_bot/utils/helpers.py`의 `split_message()`를 import하여 사용한다. Telegram은 `max_len=4000`, Discord는 `max_len=2000`으로 호출.

```python
# telegram.py
from shacs_bot.utils.helpers import split_message
# 사용: split_message(content, max_len=4000)

# discord.py  
from shacs_bot.utils.helpers import split_message
# 사용: split_message(content, max_len=2000)
```

#### 변경 7: 도구 등록 팩토리 추출

**변경 파일**: `shacs_bot/agent/tools/registry.py` (팩토리 함수 추가), `shacs_bot/agent/loop.py`, `shacs_bot/agent/subagent.py`

`create_default_tools()` 팩토리 함수를 추출하여 `tools/registry.py`에 배치한다. `AgentLoop._register_default_tools()`와 `SubagentManager._run_subagent()`에서 이를 호출한다.

```python
# tools/registry.py에 추가
def create_default_tools(
    workspace: Path,
    restrict_to_workspace: bool = False,
    exec_config: ExecToolConfig | None = None,
    brave_api_key: str | None = None,
    web_proxy: str | None = None,
) -> list[Tool]:
    """공용 기본 도구 세트를 생성한다."""
    allowed_dir = workspace if restrict_to_workspace else None
    exec_cfg = exec_config or ExecToolConfig()
    return [
        ReadFileTool(workspace=workspace, allowed_dir=allowed_dir),
        WriteFileTool(workspace=workspace, allowed_dir=allowed_dir),
        EditFileTool(workspace=workspace, allowed_dir=allowed_dir),
        ListDirTool(workspace=workspace, allowed_dir=allowed_dir),
        ExecTool(working_dir=str(workspace), timeout=exec_cfg.timeout, ...),
        WebSearchTool(api_key=brave_api_key, proxy=web_proxy),
        WebFetchTool(proxy=web_proxy),
        SearchHistoryTool(workspace=workspace),
    ]
```

AgentLoop은 이 리스트를 받아 추가 도구(MessageTool, SpawnTool, CronTool)를 등록한다.
SubagentManager는 역할(role)의 `allowed_tools` 필터를 적용하여 등록한다.

#### 변경 8: 채널 초기화 데이터 드리븐

**변경 파일**: `shacs_bot/channels/manager.py`

~120줄의 반복 if 블록을 선언적 레지스트리 + 루프로 교체한다.

```python
_CHANNEL_DEFS: tuple[tuple[str, str, str, dict[str, str]], ...] = (
    # (config_attr, module, class_name, extra_kwargs_source)
    ("telegram", "shacs_bot.channels.telegram", "TelegramChannel",
     {"groq_api_key": "providers.groq.api_key"}),
    ("whatsapp", "shacs_bot.channels.whatsapp", "WhatsAppChannel", {}),
    ("discord", "shacs_bot.channels.discord", "DiscordChannel", {}),
    ("feishu", "shacs_bot.channels.feishu", "FeishuChannel", {}),
    ("mochat", "shacs_bot.channels.mochat", "MochatChannel", {}),
    ("dingtalk", "shacs_bot.channels.dingtalk", "DingTalkChannel", {}),
    ("email", "shacs_bot.channels.email", "EmailChannel", {}),
    ("slack", "shacs_bot.channels.slack", "SlackChannel", {}),
    ("qq", "shacs_bot.channels.qq", "QQChannel", {}),
    ("matrix", "shacs_bot.channels.matrix", "MatrixChannel", {}),
)

def _init_channels(self) -> None:
    for attr, module, cls_name, extra_src in _CHANNEL_DEFS:
        cfg = getattr(self._config.channels, attr, None)
        if not cfg or not cfg.enabled:
            continue
        try:
            mod = importlib.import_module(module)
            cls = getattr(mod, cls_name)
            kwargs = self._resolve_extra_kwargs(extra_src)
            self._channels[attr] = cls(cfg, self._bus, **kwargs)
            logger.info("{} channel enabled", attr)
        except ImportError as e:
            logger.warning("{} channel not available: {}", attr, e)
    self._validate_allow_from()
```

---

### Phase 4: 스키마 동기화

#### 변경 9: ProvidersConfig 필드 추가

**파일**: `shacs_bot/config/schema.py`

누락된 5개 provider 필드를 `ProvidersConfig`에 추가한다:

```python
class ProvidersConfig(BaseModel):
    # ... 기존 필드 ...
    azure_openai: ProviderConfig = Field(default_factory=ProviderConfig)
    volcengine_coding_plan: ProviderConfig = Field(default_factory=ProviderConfig)
    byteplus: ProviderConfig = Field(default_factory=ProviderConfig)
    byteplus_coding_plan: ProviderConfig = Field(default_factory=ProviderConfig)
    ollama: ProviderConfig = Field(default_factory=ProviderConfig)
```

---

### Phase 5: 에러 처리 개선

#### 변경 10: 핵심 경로 except 구체화

**변경 파일**: `shacs_bot/agent/loop.py`, `shacs_bot/agent/memory.py`, `shacs_bot/config/loader.py`

핵심 실행 경로의 `except Exception:` 중 구체화 가능한 것만 대상:

| 파일 | 위치 | 현재 | 수정 |
|---|---|---|---|
| `loop.py:213` | MCP 연결 cleanup | `except Exception: pass` | `except (RuntimeError, OSError): pass` |
| `loop.py:343` | /new 아카이브 | `except Exception:` | `except Exception:` 유지 (logger.exception 있음) |
| `memory.py:169` | consolidation 실패 | `except Exception:` | `except Exception:` 유지 (logger.exception 있음) |
| `loader.py:56` | config 로드 | `except (json.JSONDecodeError, ValueError)` | 유지 (이미 구체적) |

이 phase에서는 보수적으로 접근하여, 이미 `logger.exception()`이 있는 곳은 유지하고, `pass`로 무시되는 곳만 구체화한다.

---

## 검증 기준

| # | 항목 | 검증 방법 |
|---|---|---|
| 1 | memory.py 버그 수정 | LSP diagnostics 클린 |
| 2 | commands.py await | LSP diagnostics 클린 |
| 3-5 | 컨벤션 통일 | `grep -r 'Union\[' --include=*.py`, `grep -r 'Optional\[' --include=*.py`, `grep -r 'print(' shacs_bot/config/` 모두 0건 |
| 6 | split_message 통합 | `grep -rn '_split_message' --include=*.py` → 0건 (private 함수 제거 확인) |
| 7 | 도구 팩토리 | `grep -rn 'create_default_tools' --include=*.py` → loop.py, subagent.py, registry.py에서 사용 |
| 8 | 채널 레지스트리 | `gateway` 명령으로 모든 채널 정상 초기화 (enabled=false 기본값이므로 에러 없이 통과) |
| 9 | 스키마 동기화 | `Config._match_provider()` 에서 5개 provider의 `getattr` → non-None |
| 10 | except 구체화 | 대상 파일 LSP diagnostics 클린 |

## 실행 순서

```
Phase 1 (크리티컬 버그)    → 변경 1, 2
Phase 2 (컨벤션)          → 변경 3, 4, 5
Phase 3 (중복 제거)       → 변경 6, 7, 8
Phase 4 (스키마)          → 변경 9
Phase 5 (에러 처리)       → 변경 10
```

각 Phase는 독립적으로 적용/롤백 가능.

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-17 | PRD 초안 작성 |
| 2026-03-17 | Phase 1 완료. 변경 1: `memory.py` 데드 코드 제거. 변경 2: `commands.py` `_pick_heartbeat_target()` await 추가. |
| 2026-03-17 | Phase 2 완료. 변경 3: `litellm.py` Union 제거 → PEP 604. 변경 4: `quick_validate.py` Optional 제거. 변경 5: `loader.py` print → loguru. |
| 2026-03-17 | Phase 3 완료. 변경 6: `split_message` 중복 제거 → `utils/helpers.py`로 통합. 변경 7: `create_default_tools` 팩토리 추출 → `tools/registry.py`. 변경 8: `channels/manager.py` `_CHANNEL_DEFS` 선언적 레지스트리 전환. |
| 2026-03-17 | Phase 4 완료. 변경 9: `ProvidersConfig`에 azure_openai, volcengine_coding_plan, byteplus, byteplus_coding_plan, ollama 필드 추가. |
| 2026-03-17 | Phase 5 완료. 변경 10: `loop.py` `except Exception: pass` → `except (RuntimeError, BaseExceptionGroup): pass` 구체화. |
