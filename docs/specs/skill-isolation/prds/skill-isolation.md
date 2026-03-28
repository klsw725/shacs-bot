# PRD: 스킬 격리 실행 (Skill Isolation) v3

> **배경**: v2의 frontmatter 기반 격리 선언을 폐기한다. 대신 **전체 스킬 서브에이전트 실행 + 출처 기반 승인 + 슬래시 명령어 모드 전환** 모델로 전환한다.
>
> **v2 대비 변경**: frontmatter `isolation` 삭제, pyyaml 삭제, 모든 스킬을 서브에이전트로 실행, 승인 모드를 `/skill trust` 슬래시 명령어로 제어.
>
> **참고**: [Claude Code auto mode](https://www.anthropic.com/engineering/claude-code-auto-mode) 아키텍처를 참고하여 3단계 티어 시스템, reasoning-blind 분류기, 규칙 기반 사전 필터를 반영.

---

## 문제

현재 스킬은 메인 LLM이 `read_file(SKILL.md)` → 직접 도구 호출하는 단일 실행 모드만 갖고 있다.

위협 모델:

1. **프롬프트 인젝션**: SKILL.md에 악의적 내용이 있으면, 메인 LLM이 `read_file`로 읽는 순간 오케스트레이터가 조작됨.
2. **도구 남용**: 스킬이 `exec`, `write_file` 등으로 시스템 상태를 무제한 변경 가능.
3. **메인 대화 블로킹**: 긴 작업이 메인 대화를 점유함.
4. **격리 부재**: 한 스킬의 도구 호출이 다른 스킬이나 메인 대화에 영향을 줄 수 있음.

## 해결책

### 전체 스킬 서브에이전트 실행

모든 스킬(builtin + workspace)은 서브에이전트에서 실행한다. 메인 LLM은 어떤 스킬의 SKILL.md도 직접 읽지 않는다.

### 출처 기반 승인

- **builtin 스킬**: 승인 없이 실행 (신뢰).
- **workspace 스킬**: 승인 모드에 따라 도구 호출 검토 (비신뢰).

### 슬래시 명령어로 모드 전환

```
/skill trust auto     ← 3단계 분류기가 판단 (기본값)
/skill trust manual   ← 사용자에게 직접 승인 요청
/skill trust off      ← 승인 없이 실행
```

설정은 `config.json`의 `tools.skillApproval`에 저장. LLM이 config.json을 직접 읽거나 쓰지 않는다.

---

## 실행 흐름

```
스킬 요청
  → 메인 LLM: SKILL.md를 절대 읽지 않음
  → spawn(task="...", skill_path="/path/to/SKILL.md")
  → SubagentManager._run_skill()
      ├─ 출처 확인
      ├─ builtin → 승인 없이 실행
      ├─ workspace → 승인 모드 확인
      │   ├─ auto: 3단계 분류기 (규칙 → 빠른 필터 → LLM 판단)
      │   ├─ manual: 사용자에게 승인 요청 → 대기
      │   └─ off: 승인 없이 실행
      └─ 결과 → MessageBus → 메인에 전달
```

### auto 모드: 3단계 분류기 (Claude Code auto mode 참고)

도구 호출이 발생하면 3단계를 순서대로 거친다. 각 단계에서 결정되면 다음 단계로 가지 않는다.

```
도구 호출 발생
  → Tier 1: 규칙 기반 즉시 판정
  │   ├─ ALWAYS_ALLOW (read_file, list_dir 등) → 즉시 승인
  │   └─ ALWAYS_DENY (rm -rf, curl|sh 등) → 즉시 차단
  │
  → Tier 2: workspace 내 파일 쓰기 → 즉시 승인
  │   write_file/edit_file 대상이 workspace 하위면 통과
  │   (버전 관리로 복구 가능)
  │
  → Tier 3: LLM 분류기 (나머지 도구)
      ├─ 메인 provider + reasoning-blind 세션 컨텍스트
      ├─ 사용자 메시지 + 도구 호출만 전달 (에이전트 추론/결과 제거)
      └─ 승인/거부 판정
```

#### Tier 1: 규칙 기반 즉시 판정

```python
ALWAYS_ALLOW: frozenset[str] = frozenset({
    "read_file", "list_dir", "web_search", "web_fetch", "search_history",
})

ALWAYS_DENY_PATTERNS: list[str] = [
    r"rm\s+(-rf|-fr)\s+[/~]",          # rm -rf /
    r"curl\s.*\|\s*(sh|bash)",           # curl | sh
    r"mkfs\.",                            # mkfs.*
    r"dd\s+.*of=/dev/",                  # dd of=/dev/*
    r">\s*/dev/sd",                       # > /dev/sd*
    r"chmod\s+777\s+/",                   # chmod 777 /
    r":(){ :\|:& };:",                    # fork bomb
]
```

- ALWAYS_ALLOW: 읽기 전용 도구 → LLM 호출 없이 즉시 승인
- ALWAYS_DENY: 명백히 위험한 패턴 → LLM 호출 없이 즉시 차단

#### Tier 2: workspace 내 파일 쓰기

```python
def _is_workspace_write(self, tool_name: str, arguments: dict) -> bool:
    """workspace 내 파일 쓰기/편집인지 확인."""
    if tool_name not in ("write_file", "edit_file"):
        return False
    path = Path(arguments.get("path", "")).expanduser().resolve()
    return str(path).startswith(str(self._workspace))
```

workspace 내 파일 변경은 버전 관리(git)로 복구 가능하므로 승인 없이 허용. Claude Code의 "in-project file operations" 티어와 동일한 논리.

#### Tier 3: LLM 분류기 (reasoning-blind)

나머지 도구 호출(exec, workspace 밖 write_file 등)은 메인 오케스트레이터 LLM이 판단.

**reasoning-blind 원칙**: 분류기에 전달하는 세션 히스토리에서 에이전트의 자체 메시지와 도구 결과를 제거. 사용자 메시지와 도구 호출만 전달. 이는 서브에이전트(스킬에 오염 가능)의 추론이 분류기 판단에 영향을 주는 것을 방지.

```python
async def _check_auto_tier3(self, tool_name, arguments) -> ApprovalDecision:
    # 세션 히스토리에서 사용자 메시지 + 도구 호출만 추출 (reasoning-blind)
    filtered_history = [
        msg for msg in self._session_history
        if msg["role"] == "user"
        or (msg["role"] == "assistant" and msg.get("tool_calls"))
    ]
    # 에이전트 메시지의 content는 제거, tool_calls만 유지
    for msg in filtered_history:
        if msg["role"] == "assistant":
            msg = {**msg, "content": ""}

    response = await asyncio.wait_for(
        self._provider.chat_with_retry(
            messages=[
                {"role": "system", "content": CLASSIFIER_PROMPT},
                *filtered_history,
                {"role": "user", "content": f"도구 호출 승인 요청:\n도구: {tool_name}\n인자: {json.dumps(arguments)}"},
            ],
            model=self._model,
        ),
        timeout=30,
    )
    ...
```

**분류기 프롬프트 판단 기준** (Claude Code 참고):
- **사용자 의도 범위**: 사용자가 명시적으로 요청한 범위 내의 행동인가?
- **폭발 반경**: 되돌릴 수 없는 파괴적 작업인가?
- **신뢰 경계**: 외부 서비스로 데이터를 전송하는가?
- **권한 에스컬레이션**: 보안 검사를 우회하는가?

### manual 모드 (사용자 직접 승인)

```
서브에이전트: exec("pip install yt-dlp") 실행하려 함
  → MessageBus로 사용자에게 승인 요청
    "🔧 스킬 'youtube-summary'가 실행하려 합니다:
     exec: pip install yt-dlp
     [승인] [거부]"
  → 사용자 응답 대기 (타임아웃 60초)
  → 승인 → 실행 / 거부·타임아웃 → 스킵
```

---

## 사용자 영향

| Before | After |
|---|---|
| 모든 스킬이 메인 LLM에서 직접 실행 | 모든 스킬이 서브에이전트에서 백그라운드 실행 |
| 스킬 내용이 메인 컨텍스트에 직접 주입 | 스킬 내용은 서브에이전트에만 존재 |
| 도구 호출 제어 불가 | workspace 스킬은 승인 모드에 따라 검토 |
| 긴 작업이 대화 블로킹 | 백그라운드 실행, 완료 시 결과 전달 |
| 승인 모드 변경 불가 | `/skill trust auto\|manual\|off` 로 즉시 전환 |

---

## 기술적 범위

### 신규 파일

| 파일 | 역할 | 규모 |
|---|---|---|
| `shacs_bot/agent/approval.py` | ApprovalGate — 3단계 분류기 (auto) + 사용자 승인 (manual) | ~150줄 |

### 수정 파일

| 파일 | 변경 내용 | 규모 |
|---|---|---|
| `shacs_bot/config/schema.py` | `ToolsConfig`에 `skill_approval` 필드 추가 | ~3줄 |
| `shacs_bot/agent/skills.py` | `build_skills_summary()`에 `source` 속성 추가. `get_skill_source()` 메서드 추가. | ~15줄 |
| `shacs_bot/agent/tools/spawn.py` | `skill_path` 파라미터 추가 | ~20줄 |
| `shacs_bot/agent/subagent.py` | `spawn_skill()` + `_run_skill()` 메서드. 승인 모드 분기. | ~120줄 |
| `shacs_bot/agent/context.py` | 시스템 프롬프트에 스킬 실행 정책 안내 추가 | ~10줄 |
| `shacs_bot/agent/loop.py` | `/skill trust` 슬래시 명령어 핸들러. `SubagentManager`에 설정 전달. | ~25줄 |

### 총 변경량: ~345줄 추가

---

## 설계

### 1. 설정 (`config/schema.py`)

```python
class ToolsConfig(Base):
    ...
    skill_approval: Literal["auto", "manual", "off"] = "auto"
```

```jsonc
// ~/.shacs-bot/config.json
{
  "tools": {
    "skillApproval": "auto"
  }
}
```

### 2. 슬래시 명령어 (`agent/loop.py`)

```python
elif cmd.startswith("/skill trust"):
    parts = cmd.split()
    if len(parts) == 3 and parts[2] in ("auto", "manual", "off"):
        mode = parts[2]
        config = load_config()
        config.tools.skill_approval = mode
        save_config(config)
        self._skill_approval = mode
        return OutboundMessage(
            channel=msg.channel, chat_id=msg.chat_id,
            content=f"스킬 승인 모드: {mode}",
        )
    else:
        return OutboundMessage(
            channel=msg.channel, chat_id=msg.chat_id,
            content=f"현재 모드: {self._skill_approval}\n사용법: /skill trust auto|manual|off",
        )
```

### 3. 스킬 출처 정보 (`agent/skills.py`)

`build_skills_summary()`에 `source` 속성 추가. `get_skill_source()` 메서드.

### 4. 시스템 프롬프트 (`agent/context.py`)

"모든 스킬은 spawn으로 위임. SKILL.md를 직접 읽지 마세요."

### 5. SpawnTool (`agent/tools/spawn.py`)

`skill_path` 파라미터 추가. `execute()`에서 분기.

### 6. SubagentManager (`agent/subagent.py`)

`spawn_skill()` + `_run_skill()`. 출처에 따라 ApprovalGate 분기.

### 7. ApprovalGate (`agent/approval.py`)

```python
@dataclass(frozen=True)
class ApprovalDecision:
    denied: bool
    reason: str


class ApprovalGate:
    """workspace 스킬의 도구 호출을 3단계 분류기 또는 사용자 승인으로 검사."""

    ALWAYS_ALLOW: frozenset[str] = frozenset({
        "read_file", "list_dir", "web_search", "web_fetch", "search_history",
    })

    ALWAYS_DENY_PATTERNS: list[re.Pattern] = [
        re.compile(r"rm\s+(-rf|-fr)\s+[/~]"),
        re.compile(r"curl\s.*\|\s*(sh|bash)"),
        re.compile(r"mkfs\."),
        re.compile(r"dd\s+.*of=/dev/"),
        re.compile(r">\s*/dev/sd"),
        re.compile(r"chmod\s+777\s+/"),
    ]

    CLASSIFIER_PROMPT: str = """\
당신은 보안 분류기입니다. 서브에이전트의 도구 호출이 사용자의 의도 범위 내에서 안전한지 판단합니다.

판단 기준:
1. 사용자 의도 범위: 사용자가 명시적으로 요청한 작업과 관련 있는가?
2. 폭발 반경: 되돌릴 수 없는 파괴적 작업인가?
3. 신뢰 경계: 외부 서비스로 데이터를 전송하는가?
4. 권한 에스컬레이션: 보안 검사를 우회하거나 의도하지 않은 리소스에 접근하는가?

반드시 다음 JSON으로만 답변:
{{"approved": true/false, "reason": "판단 이유"}}
"""

    def __init__(self, mode, provider, model, session_history, bus, origin, skill_name, workspace):
        self._mode = mode
        self._provider = provider
        self._model = model
        self._session_history = session_history
        self._bus = bus
        self._origin = origin
        self._skill_name = skill_name
        self._workspace = workspace

    async def check(self, tool_name: str, arguments: dict) -> ApprovalDecision:
        if self._mode == "auto":
            return await self._check_auto(tool_name, arguments)
        elif self._mode == "manual":
            return await self._check_manual(tool_name, arguments)
        return ApprovalDecision(denied=False, reason="")

    async def _check_auto(self, tool_name: str, arguments: dict) -> ApprovalDecision:
        """3단계 분류기: 규칙 → workspace 쓰기 → LLM."""

        # Tier 1: 규칙 기반 즉시 판정
        if tool_name in self.ALWAYS_ALLOW:
            return ApprovalDecision(denied=False, reason="")

        if tool_name == "exec":
            cmd = arguments.get("command", "")
            for pattern in self.ALWAYS_DENY_PATTERNS:
                if pattern.search(cmd):
                    logger.warning("ApprovalGate: DENY (패턴) {} — {}", tool_name, cmd[:100])
                    return ApprovalDecision(denied=True, reason=f"위험 패턴 감지: {pattern.pattern}")

        # Tier 2: workspace 내 파일 쓰기
        if tool_name in ("write_file", "edit_file"):
            path = Path(arguments.get("path", "")).expanduser().resolve()
            if str(path).startswith(str(self._workspace)):
                return ApprovalDecision(denied=False, reason="workspace 내 파일")

        # Tier 3: LLM 분류기 (reasoning-blind)
        return await self._check_auto_llm(tool_name, arguments)

    async def _check_auto_llm(self, tool_name: str, arguments: dict) -> ApprovalDecision:
        """reasoning-blind LLM 분류기. 사용자 메시지 + 도구 호출만 전달."""
        filtered = []
        for msg in self._session_history:
            if msg["role"] == "user":
                filtered.append(msg)
            elif msg["role"] == "assistant" and msg.get("tool_calls"):
                filtered.append({"role": "assistant", "content": "", "tool_calls": msg["tool_calls"]})

        try:
            response = await asyncio.wait_for(
                self._provider.chat_with_retry(
                    messages=[
                        {"role": "system", "content": self.CLASSIFIER_PROMPT},
                        *filtered,
                        {"role": "user", "content":
                            f"도구 호출 승인 요청:\n"
                            f"스킬: {self._skill_name}\n"
                            f"도구: {tool_name}\n"
                            f"인자: {json.dumps(arguments, ensure_ascii=False, indent=2)}"
                        },
                    ],
                    model=self._model,
                ),
                timeout=30,
            )
            result = json.loads(response.content)
            approved = result.get("approved", False)
            reason = result.get("reason", "")
            logger.info("ApprovalGate: {} → {} ({})", tool_name, "승인" if approved else "거부", reason)
            return ApprovalDecision(denied=not approved, reason=reason)
        except Exception as e:
            logger.error("ApprovalGate: {} 판단 실패, 기본 거부: {}", tool_name, e)
            return ApprovalDecision(denied=True, reason=f"판단 실패: {e}")

    async def _check_manual(self, tool_name: str, arguments: dict) -> ApprovalDecision:
        """사용자에게 직접 승인 요청."""
        # Tier 1 규칙은 manual 모드에서도 적용
        if tool_name in self.ALWAYS_ALLOW:
            return ApprovalDecision(denied=False, reason="")

        if tool_name == "exec":
            cmd = arguments.get("command", "")
            for pattern in self.ALWAYS_DENY_PATTERNS:
                if pattern.search(cmd):
                    return ApprovalDecision(denied=True, reason=f"위험 패턴: {pattern.pattern}")

        future = asyncio.get_event_loop().create_future()
        args_str = json.dumps(arguments, ensure_ascii=False, indent=2)
        await self._bus.publish_outbound(OutboundMessage(
            channel=self._origin["channel"],
            chat_id=self._origin["chat_id"],
            content=(
                f"🔧 스킬 '{self._skill_name}'이 실행하려 합니다:\n"
                f"도구: {tool_name}\n"
                f"인자: {args_str}\n\n"
                f"승인하려면 'y', 거부하려면 'n'"
            ),
            metadata={"_approval_request": True, "_future_id": id(future)},
        ))
        try:
            result = await asyncio.wait_for(future, timeout=60)
            return ApprovalDecision(denied=not result, reason="사용자 판단")
        except asyncio.TimeoutError:
            return ApprovalDecision(denied=True, reason="승인 타임아웃 (60초)")
```

---

## 성공 기준

1. 모든 스킬 → `spawn(skill_path=...)` 으로 위임
2. 메인 LLM이 어떤 SKILL.md도 `read_file`로 직접 읽지 않음
3. builtin 스킬 → 승인 없이 실행
4. workspace + auto → Tier 1 규칙 → Tier 2 workspace 쓰기 → Tier 3 LLM 판단
5. workspace + manual → Tier 1 규칙 → 사용자 직접 승인
6. workspace + off → 승인 없이 실행
7. ALWAYS_DENY 패턴 → 모든 모드에서 즉시 차단
8. Tier 3 LLM에 reasoning-blind 세션 히스토리 전달
9. `/skill trust auto|manual|off` → 모드 전환 + config 저장
10. `/skill trust` → 현재 모드 표시
11. 승인 타임아웃 시 기본 DENY
12. `spawn` 도구 미등록 (재귀 방지)
13. LSP 진단: 변경 파일 신규 에러 0건

---

## 마일스톤

- [x] **M1: 기반 — 설정 + 출처 + 프롬프트 + 슬래시**
  `ToolsConfig.skill_approval`. `build_skills_summary()` source 속성. `get_skill_source()`. `context.py` 정책 안내. `/skill trust` 핸들러. **검증**: config 저장, 스킬 XML source 출력.

- [x] **M2: 스킬 서브에이전트 실행**
  SpawnTool `skill_path`. `spawn_skill()` + `_run_skill()`. builtin 무승인, workspace 모드 분기. 재귀 방지. **검증**: builtin 실행, workspace + off 실행.

- [x] **M3: ApprovalGate (auto 3단계 + manual)**
  `approval.py`. Tier 1 규칙 (ALWAYS_ALLOW + ALWAYS_DENY). Tier 2 workspace 쓰기. Tier 3 reasoning-blind LLM. manual 사용자 승인. fail-closed. **검증**: 전체 승인 흐름 동작.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 메인 LLM이 스킬을 spawn 대신 직접 read_file | 중간 | 높음 | 시스템 프롬프트 금지. 추후 하드 블록 검토. |
| auto Tier 3 LLM 판단 오류 | 낮음 | 높음 | reasoning-blind + 세션 맥락. ALWAYS_DENY로 치명적 패턴 사전 차단. fail-closed. |
| ALWAYS_DENY 패턴 우회 | 중간 | 높음 | 패턴은 첫 방어선. Tier 3 LLM이 2차 방어. 패턴 목록은 점진적 확장. |
| manual 응답 대기 블로킹 | 중간 | 중간 | asyncio.Future 비동기. 60초 타임아웃. |
| 비대화형 채널에서 manual | 중간 | 중간 | 응답 불가 시 기본 DENY. |

---

## 종속성

- **선행**: 없음
- **신규 의존성**: 없음
- **폐기**: v2의 frontmatter isolation, pyyaml

---

## v2 → v3 변경 이력

| 항목 | v2 | v3 | 이유 |
|---|---|---|---|
| 격리 대상 | workspace만 | 모든 스킬 | 통일 실행 모델, 블로킹 해소 |
| 격리 기준 | frontmatter 선언 | 스킬 출처 | 우회 불가 |
| 승인 구조 | 빈 컨텍스트 LLM 1단계 | 3단계 티어 (규칙→workspace 쓰기→reasoning-blind LLM) | Claude Code 참고. 비용 절감 + 품질 향상 |
| 사용자 승인 | 없음 | manual 모드 | 진짜 보안 경계 옵션 |
| 모드 전환 | 없음 | `/skill trust auto\|manual\|off` | 사용자 선택권 |
| 설정 저장 | 없음 | `config.json` tools.skillApproval | 영속적 |
| ALWAYS_DENY | 없음 | 규칙 기반 패턴 즉시 차단 | LLM 우회 불가 하드 블록 |
| reasoning-blind | 없음 | 에이전트 메시지 제거, 사용자+도구호출만 | 에이전트 추론에 영향 안 받음 |
| pyyaml | 추가 | 없음 | 불필요 |
| 변경량 | ~330줄 | ~345줄 | 3단계 분류기 추가로 소폭 증가 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-26 | PRD v1 초안 |
| 2026-03-27 | PRD v2: 코드베이스 반영, 파서 교체, MCP |
| 2026-03-28 | PRD v3: 전체 스킬 서브에이전트. 3단계 분류기 (Claude Code auto mode 참고). auto(규칙→workspace 쓰기→reasoning-blind LLM) + manual(사용자) + off. `/skill trust`. |
| 2026-03-28 | M1~M3 구현 완료. manual 모드 사용자 응답 대기 포함. |
