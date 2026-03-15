# PRD: 스킬 사용 힌트 알림

---

## 문제

사용자가 스킬 사용을 요청해도 봇이 해당 스킬을 실제로 사용 중인지 알 수 없다:

```
사용자: "summarize 스킬로 이 영상 요약해줘"
(typing... 30초)
봇: "영상 요약입니다: ..."
```

스킬 사용 흐름은 LLM이 `read_file`으로 SKILL.md를 읽고 → `exec`로 CLI 도구를 실행하는 구조인데, 이 과정이 사용자에게 전혀 노출되지 않는다.

기존 `tool_hint` 메커니즘(`send_tool_hints` 설정)이 있지만:
1. 기본 비활성화(`False`)
2. 활성화해도 `read_file('/path/to/skills/summarize/SKILL.md')` 같은 기술적 텍스트
3. 일반 도구 호출과 스킬 사용이 구분되지 않음

## 해결책

스킬 SKILL.md 읽기를 자동 감지하여 "🔧 summarize 스킬 사용 중" 같은 친화적 알림을 전송한다. `send_progress`/`send_tool_hints` 설정과 독립적으로 **항상 전송**된다.

## 사용자 영향

| Before | After |
|---|---|
| 스킬 사용 여부를 알 수 없음 | "🔧 summarize 스킬 사용 중" 알림 표시 |
| tool_hint 활성화해도 기술적 텍스트 | 스킬명만 추출한 친화적 메시지 |
| 스킬 알림과 일반 도구 알림이 혼재 | 스킬 알림은 별도 경로로 항상 전송 |

## 기술적 범위

- **변경 파일**: `shacs_bot/agent/loop.py`, `shacs_bot/channels/manager.py`, `shacs_bot/cli/commands.py` (3개)
- **변경 유형**: Python 코드 추가/수정
- **의존성**: 없음
- **하위 호환성**: 기존 동작에 영향 없음. 스킬 힌트가 추가되는 것뿐.

### 변경할 코드 요약

**loop.py**:
- `_detect_skill_hint(tool_calls)` 정적 메서드 추가 — `read_file` 호출에서 `/skills/{name}/SKILL.md` 패턴 감지
- `_bus_progress` 콜백에 `skill_hint: bool = False` 키워드 인자 추가, `_skill_hint` 메타데이터 설정
- `_run_agent_loop`에서 tool_hint 전송 직후 `_detect_skill_hint` 호출, 감지 시 `on_progress(msg, skill_hint=True)` 전송

```python
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

**manager.py** (line 192-196):
- `_dispatch_outbound`에서 `_skill_hint` 메타데이터 확인 — `True`이면 필터 바이패스

```python
if msg.metadata.get("_progress"):
    if msg.metadata.get("_skill_hint"):
        pass  # 스킬 힌트는 항상 전송
    elif msg.metadata.get("_tool_hint") and not self._config.channels.send_tool_hints:
        continue
    elif not msg.metadata.get("_tool_hint") and not self._config.channels.send_progress:
        continue
```

**cli/commands.py**:
- 일회성 모드 `_cli_progress` (line 565): `skill_hint` 인자 추가, 스킬 힌트는 필터 바이패스
- 인터랙티브 모드 `_consume_outbound` (line 625): `_skill_hint` 메타데이터 확인, 항상 표시

### 감지 로직 상세

| 조건 | 감지 | 메시지 |
|---|---|---|
| `read_file(path="...skills/summarize/SKILL.md")` | ✅ | `🔧 summarize 스킬 사용 중` |
| `read_file(path="...skills/weather/SKILL.md")` | ✅ | `🔧 weather 스킬 사용 중` |
| `read_file(path="...memory/MEMORY.md")` | ❌ | — |
| `exec("summarize 'https://...'")` | ❌ | — (미래 고려사항) |
| `always=true` 스킬 (시스템 프롬프트에 이미 로드됨) | ❌ | — (`read_file` 안 거침) |

## 성공 기준

1. 스킬 SKILL.md 읽기 시 "🔧 {name} 스킬 사용 중" 메시지가 사용자에게 전송된다
2. 스킬 힌트는 `send_progress=False`, `send_tool_hints=False`여도 전송된다
3. 비스킬 `read_file` 호출에는 스킬 힌트가 발생하지 않는다
4. CLI 일회성/인터랙티브 양쪽에서 스킬 힌트가 표시된다
5. 기존 tool_hint, progress 동작에 영향 없다

---

## 마일스톤

- [x] **M1: 스킬 힌트 감지 및 전송 구현**
  `_detect_skill_hint` 메서드 추가, `_bus_progress` 확장, `_run_agent_loop`에서 호출. manager.py 바이패스 추가.

- [x] **M2: CLI 스킬 힌트 표시**
  `cli/commands.py`의 일회성/인터랙티브 양쪽에서 스킬 힌트 표시.

- [x] **M3: 동작 검증**
  summarize 등 스킬 사용 시 "🔧 스킬 사용 중" 알림 확인. `send_progress=False` 상태에서도 스킬 힌트만 전송되는지 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| `always=true` 스킬은 감지 불가 | 중간 | 낮음 | 대부분의 스킬은 on-demand 로드. 별도 설계 필요 시 추후 대응 |
| 경로 패턴 매칭의 false positive | 낮음 | 낮음 | `/skills/` + `/SKILL.md` 이중 조건으로 충분히 정밀 |
| `on_progress` 콜백 시그니처 변경 | 낮음 | 낮음 | `**kwargs` 호환. 기존 콜백은 `skill_hint` 무시 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | M1+M2+M3 완료. loop.py: `_detect_skill_hint` 정적 메서드 추가 (read_file에서 /skills/{name}/SKILL.md 패턴 감지), `_bus_progress`에 `skill_hint` 키워드 인자 추가, `_run_agent_loop`에서 tool_hint 전송 직후 스킬 힌트 호출. manager.py: `_skill_hint` 메타데이터 확인 시 필터 바이패스 (항상 전송). cli/commands.py: 일회성 `_cli_progress`에 `skill_hint` 파라미터 추가 + 바이패스, 인터랙티브 `_consume_outbound`에 `_skill_hint` 메타데이터 확인 + 항상 표시. LSP 진단: 변경 관련 신규 에러 없음 (기존 타입 이슈만 존재). |
