# PRD: 메모리 통합 알림

---

## 문제

메모리 통합(consolidation)이 백그라운드에서 조용히 수행되어 사용자가 인지할 수 없다:

```
사용자: "오늘 회의 내용 정리해줘"
봇: "정리해드리겠습니다..."
(메모리 통합 발생 — 사용자 모름)
봇: "회의 내용입니다: ..."

사용자: "아까 말한 거 기억해?"
봇: "기억 못합니다"  ← 통합이 실패했는지, 안 됐는지 알 수 없음
```

현재 메모리 통합은 `logger.info()`로만 기록되며, 사용자에게는 어떤 피드백도 없다. 통합이 언제 발생하는지, 성공했는지, 무엇이 저장되었는지 전혀 투명하지 않다.

## 해결책

메모리 통합 발생 시 기존 `_progress` 메타데이터 메커니즘을 활용하여 간결한 알림을 전송한다. `_skill_hint`와 유사한 `_memory_hint` 플래그를 사용하되, 설정으로 비활성화할 수 있다.

## 사용자 영향

| Before | After |
|---|---|
| 메모리 통합 여부를 알 수 없음 | "💾 기억을 정리했어요" 알림 표시 |
| 통합 실패 시 무시됨 | 실패해도 로그에만 남음 (알림은 성공 시에만) |
| 어떤 메모리가 관리되는지 불투명 | 통합이 일어났다는 사실만 간결하게 전달 |

---

## 설계

### 알림 트리거

메모리 통합이 **실제로 발생하고 성공한 경우**에만 알림을 전송한다.

| 트리거 | 위치 | 설명 |
|---|---|---|
| 토큰 압박 통합 | `loop.py:415` `_process_message` 후반 | 응답 생성 후, 토큰 초과 시 자동 발동 |
| 토큰 압박 통합 | `loop.py:371` `_process_message` 전반 | 응답 생성 전, 토큰 초과 시 자동 발동 |
| `/new` 아카이브 | `loop.py:337` `/new` 핸들러 | 세션 초기화 시 미통합 메시지 아카이브 |

**알림 대상**: 토큰 압박 통합만. `/new`은 사용자가 명시적으로 요청한 동작이므로 별도 알림 불필요.

### 알림 메시지

```
💾 기억을 정리했어요
```

- 기술적 용어("메모리 통합", "consolidation") 대신 자연어 사용
- 한 줄로 간결하게

### 알림 경로

기존 `_progress` 메타데이터 인프라를 그대로 활용한다:

```
MemoryConsolidator.maybe_consolidate_by_tokens()
    → 통합 성공 시 콜백 호출
        → OutboundMessage(metadata={_progress: True, _memory_hint: True})
            → manager.py dispatch 필터
                → 채널로 전송
```

### 설정

`ChannelsConfig`에 `send_memory_hints` 옵션을 추가한다.

```python
class ChannelsConfig(Base):
    send_progress: bool = False
    send_tool_hints: bool = False
    send_memory_hints: bool = True   # NEW: 메모리 통합 알림 (기본 활성화)
```

**기본값 `True` 근거**: 메모리 통합은 사용자의 대화 컨텍스트에 직접 영향을 주는 동작이므로 투명성이 중요하다. 빈번하지도 않다 (토큰 압박 시에만 발동).

---

## 기술적 범위

- **변경 파일**: 4개
  - `shacs_bot/config/schema.py` — `send_memory_hints` 설정 추가
  - `shacs_bot/agent/memory.py` — 통합 성공 시 콜백 호출
  - `shacs_bot/agent/loop.py` — 콜백 연결, `_memory_hint` progress 전송
  - `shacs_bot/channels/manager.py` — `_memory_hint` 필터링 로직
  - `shacs_bot/cli/commands.py` — CLI에서 메모리 힌트 표시
- **변경 유형**: Python 코드 추가/수정
- **의존성**: 없음
- **하위 호환성**: 기존 동작에 영향 없음. 알림이 추가되는 것뿐.

### JSON 설정 예시

```json
{
  "channels": {
    "sendMemoryHints": false
  }
}
```

---

## 변경할 코드 요약

### 1. `config/schema.py` — 설정 추가

```python
class ChannelsConfig(Base):
    send_progress: bool = False
    send_tool_hints: bool = False
    send_memory_hints: bool = True   # NEW
    # ... 채널별 설정 ...
```

### 2. `agent/memory.py` — 통합 성공 콜백

`maybe_consolidate_by_tokens`가 통합 실제 수행 여부를 반환하도록 변경한다.

```python
# Before
async def maybe_consolidate_by_tokens(self, session: Session) -> None:

# After
async def maybe_consolidate_by_tokens(self, session: Session) -> bool:
    """루프: ... 통합이 실제로 수행되었으면 True를 반환한다."""
```

변경 포인트:
- 반환 타입: `None` → `bool`
- 통합이 실제로 수행되고 성공하면 `True` 반환
- 통합 불필요(토큰 충분) 또는 실패 시 `False` 반환

```python
async def maybe_consolidate_by_tokens(self, session: Session) -> bool:
    if not session.messages or self._context_window_tokens <= 0:
        return False

    lock: asyncio.Lock = self.get_lock(session.key)
    async with lock:
        estimated, source = self.estimate_session_prompt_tokens(session)
        if estimated <= 0:
            return False
        if estimated < self._context_window_tokens:
            # ... 기존 debug 로그 ...
            return False

        consolidated: bool = False
        target: int = self._context_window_tokens // 2
        for round_num in range(self._MAX_CONSOLIDATION_ROUNDS):
            if estimated <= target:
                return consolidated

            boundary = self.pick_consolidation_boundary(session, max(1, estimated - target))
            if boundary is None:
                return consolidated

            end_idx: int = boundary[0]
            chunk = session.messages[session.last_consolidated : end_idx]
            if not chunk:
                return consolidated

            # ... 기존 로그 ...

            if not await self.consolidate_messages(chunk):
                return consolidated

            consolidated = True  # 최소 1회 통합 성공
            session.last_consolidated = end_idx
            self._sessions.save(session)

            estimated, source = self.estimate_session_prompt_tokens(session)
            if estimated <= 0:
                return consolidated

        return consolidated
```

### 3. `agent/loop.py` — 알림 전송

`_process_message`에서 `maybe_consolidate_by_tokens` 반환값을 확인하고, 통합이 발생했으면 progress 메시지를 전송한다.

```python
# _process_message 내부, line 415 근처

# Before
await self._memory_consolidator.maybe_consolidate_by_tokens(session=session)

# After
consolidated: bool = await self._memory_consolidator.maybe_consolidate_by_tokens(session=session)
if consolidated and self._channels_config and self._channels_config.send_memory_hints:
    await self._bus.publish_outbound(
        OutboundMessage(
            channel=msg.channel,
            chat_id=msg.chat_id,
            content="\U0001f4be 기억을 정리했어요",
            metadata={"_progress": True, "_memory_hint": True},
        )
    )
```

**line 371 (전반 통합)도 동일하게 처리**:

```python
# Before
await self._memory_consolidator.maybe_consolidate_by_tokens(session=session)

# After  
consolidated: bool = await self._memory_consolidator.maybe_consolidate_by_tokens(session=session)
if consolidated and self._channels_config and self._channels_config.send_memory_hints:
    await self._bus.publish_outbound(
        OutboundMessage(
            channel=msg.channel,
            chat_id=msg.chat_id,
            content="\U0001f4be 기억을 정리했어요",
            metadata={"_progress": True, "_memory_hint": True},
        )
    )
```

### 4. `channels/manager.py` — 필터링

```python
# Before (line 192-203)
if msg.metadata.get("_progress"):
    if msg.metadata.get("_skill_hint"):
        pass
    elif msg.metadata.get("_tool_hint") and not self._config.channels.send_tool_hints:
        continue
    elif not msg.metadata.get("_tool_hint") and not self._config.channels.send_progress:
        continue

# After
if msg.metadata.get("_progress"):
    if msg.metadata.get("_skill_hint"):
        pass  # 스킬 힌트는 항상 전송
    elif msg.metadata.get("_memory_hint"):
        if not self._config.channels.send_memory_hints:
            continue
        # send_memory_hints=True이면 통과
    elif msg.metadata.get("_tool_hint") and not self._config.channels.send_tool_hints:
        continue
    elif not msg.metadata.get("_tool_hint") and not self._config.channels.send_progress:
        continue
```

### 5. `cli/commands.py` — CLI 표시

**일회성 모드** (`_cli_progress`):

```python
# 메모리 힌트는 send_memory_hints 설정을 따름
async def _cli_progress(content: str, *, tool_hint: bool = False, skill_hint: bool = False) -> None:
    # 기존 로직 유지 — loop.py에서 이미 설정을 체크하고 보내므로 CLI progress에서는 별도 필터 불필요
```

> loop.py에서 `self._channels_config.send_memory_hints`를 체크한 뒤 `_bus.publish_outbound`로 보내므로, CLI 일회성 모드(`process_direct` → `on_progress`)에서는 메모리 힌트가 `_bus_progress`를 거치지 않는다. 따라서 일회성 모드는 변경 불필요.

**인터랙티브 모드** (`_consume_outbound`):

```python
# Before
if msg.metadata.get("_progress"):
    is_skill_hint: bool = msg.metadata.get("_skill_hint", False)
    is_tool_hint: bool = msg.metadata.get("_tool_hint", False)
    # ...

# After
if msg.metadata.get("_progress"):
    is_skill_hint: bool = msg.metadata.get("_skill_hint", False)
    is_memory_hint: bool = msg.metadata.get("_memory_hint", False)
    is_tool_hint: bool = msg.metadata.get("_tool_hint", False)
    ch: ChannelsConfig = agent_loop.channels_config
    if is_skill_hint:
        console.print(f"  [dim]↳ {msg.content}[/dim]")
    elif is_memory_hint:
        if ch and ch.send_memory_hints:
            console.print(f"  [dim]↳ {msg.content}[/dim]")
    elif ch and is_tool_hint and not ch.send_tool_hints:
        pass
    elif ch and not is_tool_hint and not ch.send_progress:
        pass
    else:
        console.print(f"  [dim]↳ {msg.content}[/dim]")
```

---

## 타이밍 다이어그램

```
사용자: "오늘 회의 정리해줘"
    │
    ▼
_process_message()
    │
    ├─ maybe_consolidate_by_tokens()  ← 전반 통합 (토큰 초과 시)
    │   └─ consolidated=True → "💾 기억을 정리했어요" 전송
    │
    ├─ _run_agent_loop()  ← LLM 호출 + 도구 실행
    │
    ├─ _save_turn()  ← 이번 턴 저장
    │
    ├─ maybe_consolidate_by_tokens()  ← 후반 통합 (새 메시지 추가 후 토큰 재초과 시)
    │   └─ consolidated=True → "💾 기억을 정리했어요" 전송
    │
    └─ return OutboundMessage("회의 내용입니다: ...")  ← 최종 응답
```

**메시지 순서 (사용자 시점)**:
```
💾 기억을 정리했어요     ← (전반 통합 발생 시)
🔧 summarize 스킬 사용 중  ← (스킬 사용 시)
회의 내용입니다: ...      ← 최종 응답
💾 기억을 정리했어요     ← (후반 통합 발생 시, 드묾)
```

> 전반/후반 모두 발생하는 경우는 극히 드물다. 일반적으로 한 턴에 최대 1회.

---

## 변경하지 않는 것

- **`/new` 명령 알림** — 사용자가 명시적으로 요청한 동작이므로 "새로운 세션이 시작되었습니다" 응답으로 충분
- **통합 실패 알림** — 실패 시 사용자에게 알리면 불안감만 줌. 로그로 충분.
- **통합 내용 표시** — "무엇이 저장되었는지"는 이번 스코프 아님. 사실 여부만 전달.
- **새로운 채널별 설정** — `ChannelsConfig` 레벨에서 통합 관리. 채널별 개별 설정 불필요.

---

## 성공 기준

1. 토큰 압박으로 메모리 통합이 발생하면 "💾 기억을 정리했어요" 메시지가 사용자에게 전송된다
2. `sendMemoryHints: false` 설정 시 알림이 전송되지 않는다
3. `/new` 명령 시에는 별도 메모리 알림이 전송되지 않는다
4. 통합이 발생하지 않으면 (토큰 충분) 알림이 전송되지 않는다
5. CLI 인터랙티브 모드에서 메모리 힌트가 `[dim]` 스타일로 표시된다
6. 기존 `send_progress`, `send_tool_hints`, `_skill_hint` 동작에 영향 없다

---

## 마일스톤

- [x] **M1: 설정 + 통합 반환값 변경**
  `config/schema.py`에 `send_memory_hints` 추가. `memory.py`의 `maybe_consolidate_by_tokens` 반환 타입을 `bool`로 변경.

- [x] **M2: 알림 전송 구현**
  `loop.py`에서 통합 결과 확인 후 `_memory_hint` progress 메시지 전송. `manager.py`와 `cli/commands.py`에서 필터링 로직 추가.

- [x] **M3: 검증**
  메모리 통합 발생 시 알림 확인. `sendMemoryHints: false` 시 알림 미전송 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 통합이 빈번하면 알림 노이즈 | 낮음 | 중간 | 토큰 압박 시에만 발동. 일반 대화에서는 거의 발생 안 함 |
| 후반 통합 알림이 최종 응답 뒤에 도착 | 중간 | 낮음 | 자연스러운 순서. "응답 → 정리" 흐름 |
| `maybe_consolidate_by_tokens` 반환 타입 변경 | 낮음 | 낮음 | 기존 호출부가 반환값을 사용하지 않았으므로 호환성 문제 없음 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | M1+M2+M3 구현 완료. `schema.py`에 `send_memory_hints: bool = True` 추가 (line 208). `memory.py`의 `maybe_consolidate_by_tokens` 반환 타입을 `bool`로 변경 (line 244). `loop.py`에서 통합 성공 시 `_memory_hint` 메타데이터와 함께 "💾 기억을 정리했어요" progress 전송 (lines 375-383, 434-440). `manager.py`에서 `_memory_hint` 필터링 (line 126-127). `commands.py` 인터랙티브 모드에서 메모리 힌트 표시 (lines 660-666). |
