# SPEC: Progress 메시지 UX 개선

> **Prompt**: 봇이 "summarize 스킬로 자막을 먼저 추출한 뒤, 제가 직접 요약해드릴게요. 잠시만요." 처럼 답변하면 백그라운드에서 작업이 돌아가고 있는지 아닌지를 사용자가 알 수 없다. 추가로 스킬을 사용하라고 했을 때 스킬을 사용 중인지도 알 수 없다.

## PRDs

이 SPEC은 다음 PRD들로 분해되어 구현된다:

| PRD | 대응 변경 | 설명 |
|---|---|---|
| [`docs/prds/progress-message-cleanup.md`](../prds/progress-message-cleanup.md) | 변경 1, 2 | send_progress 기본값 변경 + Discord typing 수정 |
| [`docs/prds/skill-hint.md`](../prds/skill-hint.md) | 변경 3 | 스킬 사용 감지 및 친화적 알림 |

---

## TL;DR

> **목적**: (1) LLM의 중간 독백("잠시만요")이 최종 응답처럼 보이는 문제를 해결한다. (2) 스킬 사용 시 사용자에게 친화적 알림을 보낸다.
>
> **Deliverables**:
> - `config/schema.py` — `send_progress` 기본값 `True` → `False`
> - `channels/discord.py` — progress 메시지 전송 시 typing 유지
> - `agent/loop.py` — 스킬 힌트 감지 및 전송
> - `channels/manager.py` — 스킬 힌트 필터 바이패스
> - `cli/commands.py` — CLI에서 스킬 힌트 표시
>
> **Estimated Effort**: Short (1-2시간)

---

## 문제 분석

### 문제 1: LLM 독백이 최종 응답처럼 보임

```
사용자: "이 영상 요약해줘"
     ↓
LLM: text="잠시만요..." + tool_call=exec("summarize ...")
     ↓
① thought "잠시만요..." → _progress=True로 사용자에게 전송  ← 문제
② tool_hint "exec('summarize ...')" → _tool_hint=True (기본 비활성화)
③ 도구 실행 (수초~수분 소요)
④ 최종 응답 전송
```

`send_progress` 기본값이 `True`이므로, LLM이 도구 호출 전에 생성한 "생각(thought)" 텍스트가 사용자에게 그대로 전달된다. 이 텍스트는 시스템 상태 표시가 아니라 LLM의 독백이며, 최종 응답과 시각적으로 구분이 안 된다.

### 문제 2: 스킬 사용 여부를 알 수 없음

스킬 사용 흐름은 LLM이 `read_file`로 SKILL.md를 읽고 → `exec`로 CLI 도구를 실행하는 구조다. 이 과정이 사용자에게 전혀 보이지 않는다:

```
사용자: "summarize 스킬로 요약해줘"
     ↓
LLM: read_file(".../skills/summarize/SKILL.md")   ← 사용자에게 안 보임
     ↓
LLM: exec("summarize 'https://...' --youtube auto") ← 사용자에게 안 보임 (30초)
     ↓
LLM: 최종 응답
```

`tool_hint` 메커니즘이 있지만 기본 비활성화(`send_tool_hints=False`)이고, 활성화해도 `read_file('/path/to/SKILL.md')` 같은 기술적 텍스트라 일반 사용자에게 노이즈.

### 채널별 Typing Indicator 현황

| 채널 | Typing 지원 | progress 수신 시 typing 유지 | 비고 |
|------|:----------:|:---------------------------:|------|
| Telegram | ✅ | ✅ | 정상 동작 |
| Discord | ✅ | ❌ **버그** | `send()` 끝에서 무조건 `_stop_typing()` 호출 |
| Matrix | ✅ | ✅ | 정상 동작 |
| Slack | ❌ | — | typing 미구현 |
| WhatsApp | ❌ | — | typing 미구현 |
| 기타 (Feishu, Mochat, DingTalk, Email, QQ) | ❌ | — | typing 미구현 |

---

## 변경 사항

### 변경 1: `send_progress` 기본값 → `False`

**파일**: `shacs_bot/config/schema.py:204`

```python
# Before
send_progress: bool = True   # stream agent's text progress to the channel

# After
send_progress: bool = False  # stream agent's text progress to the channel
```

**효과**:
- LLM 독백("잠시만요")이 사용자에게 전달되지 않음
- Typing indicator만으로 "작업 중" 상태 전달
- 기존 사용자가 중간 텍스트를 원하면 config에서 `sendProgress: true` 설정 가능
- `send_tool_hints`는 이미 `False`이므로 변경 불필요

**영향 범위**: 채널 매니저 dispatch 로직(manager.py:192-196)과 CLI progress 출력(cli/commands.py:565-573) 모두 이 설정을 참조하므로, 기본값 변경만으로 전체 적용.

### 변경 2: Discord typing indicator 버그 수정

**파일**: `shacs_bot/channels/discord.py:98-123`

```python
# Before (send 메서드 마지막)
finally:
    await self._stop_typing(msg.chat_id)

# After (Telegram/Matrix 패턴과 동일하게)
finally:
    if not msg.metadata.get("_progress", False):
        await self._stop_typing(msg.chat_id)
```

**효과**:
- Progress 메시지 전송 시 typing indicator가 계속 유지됨
- 최종 응답 전송 시에만 typing 중지
- Telegram(line 241)과 Matrix(line 451)의 기존 패턴과 일관성 확보

### 변경 3: 스킬 사용 감지 및 친화적 알림

스킬 사용을 자동 감지하여 "🔧 summarize 스킬 사용 중" 같은 친화적 메시지를 전송한다. `send_progress`나 `send_tool_hints` 설정과 독립적으로 **항상 전송**된다.

#### 3-1. 스킬 힌트 감지 — `agent/loop.py`

`_run_agent_loop`에서 tool call 목록을 분석하여 SKILL.md 읽기를 감지한다.

```python
# 새 메서드 추가
@staticmethod
def _detect_skill_hint(tool_calls: list[ToolCallRequest]) -> str | None:
    """도구 호출에서 스킬 사용을 감지하여 친화적 힌트를 반환합니다."""
    for tc in tool_calls:
        if tc.name == "read_file":
            path: str = (tc.arguments or {}).get("path", "")
            if "/skills/" in path and path.endswith("/SKILL.md"):
                skill_name: str = path.split("/skills/")[-1].split("/")[0]
                return f"🔧 {skill_name} 스킬 사용 중"
    return None
```

감지 로직: `read_file` 호출의 `path` 인자가 `/skills/{name}/SKILL.md` 패턴에 매칭되면 스킬 이름을 추출한다. 워크스페이스 스킬(`{workspace}/skills/{name}/SKILL.md`)과 빌트인 스킬(`{builtin_dir}/{name}/SKILL.md`) 모두 이 패턴에 해당.

#### 3-2. 스킬 힌트 전송 — `agent/loop.py`

`_run_agent_loop`의 tool call 처리 블록에서 스킬 힌트를 전송한다.

```python
# _run_agent_loop 내부, 기존 tool_hint 전송 직후
if response.has_tool_calls:
    if on_progress:
        thought = self._strip_think(response.content)
        if thought:
            await on_progress(thought)
        await on_progress(self._tool_hint(response.tool_calls), tool_hint=True)

        # NEW: 스킬 사용 감지 및 알림
        skill_msg: str | None = self._detect_skill_hint(response.tool_calls)
        if skill_msg:
            await on_progress(skill_msg, skill_hint=True)
```

#### 3-3. `_bus_progress` 콜백 확장 — `agent/loop.py`

`_bus_progress`에 `skill_hint` 키워드 인자를 추가한다.

```python
# Before
async def _bus_progress(content: str, *, tool_hint: bool = False) -> None:
    meta["_progress"] = True
    meta["_tool_hint"] = tool_hint

# After
async def _bus_progress(content: str, *, tool_hint: bool = False, skill_hint: bool = False) -> None:
    meta["_progress"] = True
    meta["_tool_hint"] = tool_hint
    meta["_skill_hint"] = skill_hint
```

#### 3-4. 스킬 힌트 필터 바이패스 — `channels/manager.py`

채널 매니저의 dispatch에서 `_skill_hint`는 `send_progress`/`send_tool_hints` 설정과 무관하게 항상 통과시킨다.

```python
# Before
if msg.metadata.get("_progress"):
    if msg.metadata.get("_tool_hint") and not self._config.channels.send_tool_hints:
        continue
    elif not msg.metadata.get("_tool_hint") and not self._config.channels.send_progress:
        continue

# After
if msg.metadata.get("_progress"):
    if msg.metadata.get("_skill_hint"):
        pass  # 스킬 힌트는 항상 전송
    elif msg.metadata.get("_tool_hint") and not self._config.channels.send_tool_hints:
        continue
    elif not msg.metadata.get("_tool_hint") and not self._config.channels.send_progress:
        continue
```

#### 3-5. CLI 스킬 힌트 표시 — `cli/commands.py`

CLI의 두 progress 경로(일회성 모드, 인터랙티브 모드)에서도 스킬 힌트를 표시한다.

**일회성 모드** (`_cli_progress`, line 565):
```python
# Before
async def _cli_progress(content: str, *, tool_hint: bool = False) -> None:
    ch: ChannelsConfig = agent_loop.channels_config
    if ch and tool_hint and not ch.send_tool_hints:
        return
    if ch and not tool_hint and not ch.send_progress:
        return
    console.print(f"  [dim]↳ {content}[/dim]")

# After
async def _cli_progress(content: str, *, tool_hint: bool = False, skill_hint: bool = False) -> None:
    ch: ChannelsConfig = agent_loop.channels_config
    if not skill_hint:  # 스킬 힌트는 항상 표시
        if ch and tool_hint and not ch.send_tool_hints:
            return
        if ch and not tool_hint and not ch.send_progress:
            return
    console.print(f"  [dim]↳ {content}[/dim]")
```

**인터랙티브 모드** (`_consume_outbound`, line 625):
```python
# Before
if msg.metadata.get("_progress"):
    is_tool_hint: bool = msg.metadata.get("_tool_hint", False)
    ch: ChannelsConfig = agent_loop.channels_config
    if ch and is_tool_hint and not ch.send_tool_hints:
        pass
    elif ch and not is_tool_hint and not ch.send_progress:
        pass
    else:
        console.print(f"  [dim]↳ {msg.content}[/dim]")

# After
if msg.metadata.get("_progress"):
    is_skill_hint: bool = msg.metadata.get("_skill_hint", False)
    is_tool_hint: bool = msg.metadata.get("_tool_hint", False)
    ch: ChannelsConfig = agent_loop.channels_config
    if is_skill_hint:
        console.print(f"  [dim]↳ {msg.content}[/dim]")  # 항상 표시
    elif ch and is_tool_hint and not ch.send_tool_hints:
        pass
    elif ch and not is_tool_hint and not ch.send_progress:
        pass
    else:
        console.print(f"  [dim]↳ {msg.content}[/dim]")
```

---

## 변경하지 않는 것

- `send_tool_hints` 기본값 — 이미 `False`. 스킬 힌트는 별도 경로로 처리.
- Typing indicator가 없는 채널(Slack, WhatsApp 등) — 별도 작업으로 분리.
- Subagent 결과 알림 방식 — 이번 스코프 아님.
- `always=true` 스킬의 사용 감지 — 시스템 프롬프트에 이미 포함되어 `read_file`을 거치지 않으므로 감지 불가. 별도 설계 필요.
- 새로운 config 옵션 — 스킬 힌트는 항상 전송. 비활성화 옵션을 추가하지 않음 (필요 시 추후 추가).

---

## 사용자 경험 변화

```
# Before
사용자: "summarize 스킬로 이 영상 요약해줘"
봇: "summarize 스킬로 자막을 먼저 추출하겠습니다. 잠시만요."  ← 이게 끝인가?
(Discord: typing 중지)                                       ← 끝난 것 같은데?
(30초 침묵)
봇: "영상 요약입니다: ..."

# After
사용자: "summarize 스킬로 이 영상 요약해줘"
봇: "🔧 summarize 스킬 사용 중"   ← 스킬 사용 알림 (항상 전송)
(typing... typing...)              ← 봇이 작업 중
봇: "영상 요약입니다: ..."          ← 최종 응답만 전달
```

---

## 미래 고려사항 (이번 스코프 아님)

- **장시간 작업 시 시스템 메시지**: 30초 이상 걸리는 작업에서 typing만으로 부족할 수 있음. "도구 실행 N초 경과" 같은 시스템 레벨 상태 메시지를 별도 설계.
- **Typing 미지원 채널**: Slack, WhatsApp 등에서 typing indicator 구현.
- **Subagent 진행 알림**: 서브에이전트 실행 중 중간 상태 표시.
- **always 스킬 감지**: 시스템 프롬프트에 pre-load된 스킬은 `read_file`을 거치지 않아 현재 감지 불가.
- **exec 기반 스킬 감지**: `exec` 호출에서 스킬 바이너리(예: `summarize`)를 감지하여 "🔧 summarize 실행 중" 알림 추가. 스킬 메타데이터의 `requires.bins`와 매칭 필요.

---

## 검증 기준

1. `send_progress=False`(기본값)일 때 LLM 독백이 사용자에게 전달되지 않는다
2. Discord에서 도구 실행 중 typing indicator가 유지된다
3. 스킬 SKILL.md 읽기 시 "🔧 {name} 스킬 사용 중" 메시지가 전송된다
4. 스킬 힌트는 `send_progress=False`, `send_tool_hints=False`여도 전송된다
5. 최종 응답은 정상적으로 전달된다
6. `sendProgress: true`로 설정하면 기존처럼 중간 텍스트가 전달된다
